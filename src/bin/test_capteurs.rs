//! Test combiné DS18B20 (1-Wire GP15) + BME280 (I²C GP4/GP5).
//! Sortie via USB série (CDC ACM) — même moniteur qu'avant.
//!
//! Branchements :
//!   DS18B20 : VCC→3V3, GND→GND, DATA→GP15, pull-up 4.7kΩ entre DATA et 3V3
//!   BME280  : VCC→3V3, GND→GND, SDA→GP4, SCL→GP5, SDO→GND, CSB→VCC
//!
//! Flasher  : cargo run --bin test_capteurs  (glisser-déposer UF2)
//! Moniteur : PuTTY, minicom, screen…

#![no_std]
#![no_main]

use core::fmt::Write as FmtWrite;

use rp2040_hal as hal;
use hal::pac;
use hal::Clock;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{ErrorType, InputPin, OutputPin};
use core::convert::Infallible;

use usb_device::{class_prelude::*, prelude::*};
use usbd_serial::SerialPort;
use heapless::String;

/// Mettre à true et brancher le BME280 sur GP4/GP5 pour activer les mesures I²C.
const WITH_BME280: bool = true;

use cloud_chamber_firmware::sensors::ds18b20::Ds18b20Bus;
#[allow(unused_imports)]
use cloud_chamber_firmware::sensors::bme280::Bme280Driver;

use panic_halt as _;
use defmt_rtt as _;

// ════════════════════════════════════════════════════════════════════════════
// Boot2 — obligatoire RP2040
// ════════════════════════════════════════════════════════════════════════════

#[link_section = ".boot2"]
#[used]
pub static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

// ════════════════════════════════════════════════════════════════════════════
// Délai CPU (≈125 MHz) — utilisé pour les capteurs
//
// Le timer rp2040-hal reste libre pour mesurer le temps écoulé
// pendant la conversion DS18B20 (pendant laquelle on polle USB).
// ════════════════════════════════════════════════════════════════════════════

// ════════════════════════════════════════════════════════════════════════════
// Délai matériel (Timer hardware RP2040) — utilisé pour le 1-Wire DS18B20
//
// hal::Timer::get_counter() → compteur 64 bits à 1 MHz (1 tick = 1 µs).
// Beaucoup plus précis que cortex_m::asm::delay dont la durée dépend
// du pipeline (Cortex-M0+ : N×4 cycles, pas N×3 comme supposé avant).
// ════════════════════════════════════════════════════════════════════════════

struct TimerDelay<'a> {
    timer: &'a hal::Timer,
}

impl<'a> TimerDelay<'a> {
    fn new(timer: &'a hal::Timer) -> Self { Self { timer } }
}

impl<'a> DelayNs for TimerDelay<'a> {
    fn delay_ns(&mut self, ns: u32) {
        // Arrondi au µs supérieur (le timer est à 1 µs de résolution)
        let us = ((ns as u64) + 999) / 1000;
        if us == 0 { return; }
        let end = self.timer.get_counter().ticks() + us;
        while self.timer.get_counter().ticks() < end {
            cortex_m::asm::nop();
        }
    }
}

// CortexDelay conservé uniquement pour le BME280 (délai ~15ms peu critique)
struct CortexDelay;

impl DelayNs for CortexDelay {
    fn delay_ns(&mut self, ns: u32) {
        // RP2040 @ 125 MHz : 1 cycle ≈ 8 ns, asm::delay(N) ≈ N×4 cycles ≈ N×32 ns
        cortex_m::asm::delay(ns / 32 + 1);
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Broche open-drain GP15 pour le protocole 1-Wire
//
// Le RP2040 n'a pas de GPIO open-drain natif.
//   set_low()  → OE=1, OUT=0 → tire la ligne à GND
//   set_high() → OE=0        → haute-impédance, pull-up remonte la ligne
//   is_high/low() → lit GPIO_IN (buffer d'entrée toujours actif)
// ════════════════════════════════════════════════════════════════════════════

const OW_PIN: u8 = 15;

// Le _owner est un pin OUTPUT — into_push_pull_output() configure correctement
// OEOVER=NORMAL dans IO_BANK0, OD=0 dans le pad, et SIO_OE=1.
// On repasse ensuite en haute-Z via SIO_OE_CLR, ce qui donne un open-drain.
struct OpenDrainPin {
    mask: u32,
    _owner: hal::gpio::Pin<
        hal::gpio::bank0::Gpio15,
        hal::gpio::FunctionSio<hal::gpio::SioOutput>,
        hal::gpio::PullNone,
    >,
}

impl OpenDrainPin {
    fn new(
        pin: hal::gpio::Pin<
            hal::gpio::bank0::Gpio15,
            hal::gpio::FunctionSio<hal::gpio::SioInput>,
            hal::gpio::PullNone,
        >,
    ) -> Self {
        // Reconfiguration HAL en sortie push-pull.
        // Cela garantit que tous les registres nécessaires sont corrects :
        //   IO_BANK0 GPIO15_CTRL : OEOVER=NORMAL (0b00) → SIO contrôle réellement OE
        //   PADS_BANK0 GPIO15    : OD=0 (driver de sortie actif), IE=1 (lecture active)
        //   SIO                  : FUNCSEL=5, OUT+OE gérés par SIO
        let out_pin = pin.into_push_pull_output();
        let mask = 1u32 << OW_PIN;
        unsafe {
            let sio = &*pac::SIO::ptr();
            sio.gpio_out_clr().write(|w| w.bits(mask)); // OUT=0 (tire bas quand OE=1)
            sio.gpio_oe_clr().write(|w| w.bits(mask));  // OE=0  (haute-Z initialement)
        }
        Self { mask, _owner: out_pin }
    }
}

impl ErrorType for OpenDrainPin { type Error = Infallible; }

impl OutputPin for OpenDrainPin {
    fn set_high(&mut self) -> Result<(), Infallible> {
        unsafe { (*pac::SIO::ptr()).gpio_oe_clr().write(|w| w.bits(self.mask)) };
        Ok(())
    }
    fn set_low(&mut self) -> Result<(), Infallible> {
        unsafe { (*pac::SIO::ptr()).gpio_oe_set().write(|w| w.bits(self.mask)) };
        Ok(())
    }
}

impl InputPin for OpenDrainPin {
    fn is_high(&mut self) -> Result<bool, Infallible> {
        Ok(unsafe { (*pac::SIO::ptr()).gpio_in().read().bits() } & self.mask != 0)
    }
    fn is_low(&mut self) -> Result<bool, Infallible> {
        Ok(unsafe { (*pac::SIO::ptr()).gpio_in().read().bits() } & self.mask == 0)
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Utilitaires 1-Wire bruts — diagnostic, sans passer par le crate onewire
// Toutes les durées sont en µs, mesurées via le timer hardware (1 tick = 1 µs).
// ════════════════════════════════════════════════════════════════════════════

const OW_MASK: u32 = 1u32 << 15;

/// Attend `us` µs en polant le timer hardware.
#[inline(always)]
fn ow_wait(timer: &hal::Timer, us: u64) {
    let end = timer.get_counter().ticks() + us;
    while timer.get_counter().ticks() < end { cortex_m::asm::nop(); }
}

/// Reset 1-Wire → true si une impulsion de présence est détectée.
fn ow_reset_raw(timer: &hal::Timer) -> bool {
    unsafe {
        let sio = &*pac::SIO::ptr();
        // Relâcher d'abord (haute-Z)
        sio.gpio_oe_clr().write(|w| w.bits(OW_MASK));
        ow_wait(timer, 5);
        // Reset pulse : 480 µs bas
        sio.gpio_oe_set().write(|w| w.bits(OW_MASK));
        ow_wait(timer, 480);
        // Relâcher, attendre 70 µs
        sio.gpio_oe_clr().write(|w| w.bits(OW_MASK));
        ow_wait(timer, 70);
        // Échantillonner (présence = ligne basse)
        let presence = (sio.gpio_in().read().bits() >> 15) & 1 == 0;
        // Attendre la fin de la fenêtre de présence
        ow_wait(timer, 410);
        presence
    }
}

/// Écriture d'un octet sur le bus 1-Wire (bit 0 en premier).
fn ow_write_byte_raw(timer: &hal::Timer, byte: u8) {
    unsafe {
        let sio = &*pac::SIO::ptr();
        for i in 0..8u32 {
            let bit = (byte >> i) & 1;
            // Début du slot : impulsion basse
            sio.gpio_oe_set().write(|w| w.bits(OW_MASK));
            if bit == 1 {
                // Write '1' : bas 6 µs, haut 64 µs
                ow_wait(timer, 6);
                sio.gpio_oe_clr().write(|w| w.bits(OW_MASK));
                ow_wait(timer, 64);
            } else {
                // Write '0' : bas 60 µs, haut 10 µs
                ow_wait(timer, 60);
                sio.gpio_oe_clr().write(|w| w.bits(OW_MASK));
                ow_wait(timer, 10);
            }
        }
    }
}

/// Lecture d'un octet depuis le bus 1-Wire (bit 0 en premier).
///
/// Spec DS18B20 : le slave tient la ligne basse pendant MAX 15µs depuis le
/// début du slot pour un bit '0'. Le master doit échantillonner AVANT 15µs.
///
/// Timing : bas 2µs → relâche → attend 8µs → échantillonne à ~10µs (< 15µs)
///          → attend 50µs → total slot ≈ 60µs.
fn ow_read_byte_raw(timer: &hal::Timer) -> u8 {
    let mut byte = 0u8;
    unsafe {
        let sio = &*pac::SIO::ptr();
        for i in 0..8u32 {
            // Début du slot : impulsion basse 2 µs (déclenchement du slot DS18B20)
            sio.gpio_oe_set().write(|w| w.bits(OW_MASK));
            ow_wait(timer, 2);
            // Relâcher immédiatement pour que le DS18B20 puisse répondre
            sio.gpio_oe_clr().write(|w| w.bits(OW_MASK));
            // Attendre 8 µs → échantillonnage à ~10µs depuis début slot
            // DS18B20 tient la ligne basse jusqu'à 15µs → on est dans la fenêtre
            ow_wait(timer, 8);
            // Échantillonner : 0=ligne basse (bit '0'), 1=ligne haute (bit '1')
            let bit = (sio.gpio_in().read().bits() >> 15) & 1;
            byte |= (bit as u8) << i;
            // Attendre la fin du slot (total ≈ 60µs depuis le début)
            ow_wait(timer, 50);
        }
    }
    byte
}

// ════════════════════════════════════════════════════════════════════════════
// Utilitaires USB
// ════════════════════════════════════════════════════════════════════════════

/// Attente `ms` millisecondes en polant USB — garde la connexion active
/// pendant les longues conversions DS18B20 (800ms).
fn wait_ms_usb(
    timer:   &hal::Timer,
    ms:      u64,
    usb_dev: &mut UsbDevice<hal::usb::UsbBus>,
    serial:  &mut SerialPort<hal::usb::UsbBus>,
) {
    let end_us = timer.get_counter().ticks() + ms * 1_000;
    while timer.get_counter().ticks() < end_us {
        if usb_dev.poll(&mut [serial]) {
            let mut buf = [0u8; 64];
            serial.read(&mut buf).ok();
        }
    }
}

/// Envoie des données via USB série (gère les envois partiels).
fn usb_write(serial: &mut SerialPort<hal::usb::UsbBus>, data: &[u8]) {
    let mut pos = 0;
    while pos < data.len() {
        match serial.write(&data[pos..]) {
            Ok(n)  => pos += n,
            Err(_) => break,
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Point d'entrée
// ════════════════════════════════════════════════════════════════════════════

#[hal::entry]
fn main() -> ! {
    let mut pac      = pac::Peripherals::take().unwrap();
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);

    let clocks = hal::clocks::init_clocks_and_plls(
        12_000_000u32,
        pac.XOSC, pac.CLOCKS, pac.PLL_SYS, pac.PLL_USB,
        &mut pac.RESETS, &mut watchdog,
    ).ok().unwrap();

    // Timer — utilisé uniquement pour mesurer le temps (USB polling)
    let timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

    let sio  = hal::Sio::new(pac.SIO);
    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0, pac.PADS_BANK0, sio.gpio_bank0, &mut pac.RESETS,
    );

    // ── USB CDC ACM ──────────────────────────────────────────────────────────
    // SAFETY : initialisé une seule fois ici, avant tout accès concurrent.
    static mut USB_BUS: Option<UsbBusAllocator<hal::usb::UsbBus>> = None;

    let usb_alloc = unsafe {
        USB_BUS = Some(UsbBusAllocator::new(hal::usb::UsbBus::new(
            pac.USBCTRL_REGS, pac.USBCTRL_DPRAM,
            clocks.usb_clock, true, &mut pac.RESETS,
        )));
        (*core::ptr::addr_of!(USB_BUS)).as_ref().unwrap()
    };

    let mut serial  = SerialPort::new(usb_alloc);
    let mut usb_dev = UsbDeviceBuilder::new(usb_alloc, UsbVidPid(0x2e8a, 0x0005))
        .device_class(0x02)
        .build();

    // ── Drivers capteurs ──────────────────────────────────────────────────────
    let mut ds_bus = Ds18b20Bus::new(OpenDrainPin::new(pins.gpio15.into_floating_input()));

    // I²C et BME280 : initialisés seulement si WITH_BME280 = true
    // (sans le BME280 branché, bme.init() bloque le bus I²C indéfiniment)
    #[allow(unused_variables)]
    let (mut bme_opt, bme_ok_init) = if WITH_BME280 {
        use rp2040_hal::fugit::RateExtU32;

        // Réinitialisation bus I²C : 9 impulsions SCL + condition STOP.
        // Libère un esclave (BME280) coincé en milieu de transaction après un
        // reset du Pico sans power-cycle du capteur.
        unsafe {
            let sio = &*pac::SIO::ptr();
            const SDA: u32 = 1 << 4;   // GP4
            const SCL: u32 = 1 << 5;   // GP5

            // SDA et SCL en sortie haute (via SIO — pins encore en mode SIO par défaut)
            sio.gpio_out_set().write(|w| w.bits(SDA | SCL));
            sio.gpio_oe_set().write(|w|  w.bits(SDA | SCL));
            ow_wait(&timer, 10);

            // 9 impulsions d'horloge — déverrouille tout esclave coincé
            for _ in 0..9u8 {
                sio.gpio_out_clr().write(|w| w.bits(SCL));
                ow_wait(&timer, 5);
                sio.gpio_out_set().write(|w| w.bits(SCL));
                ow_wait(&timer, 5);
            }

            // Condition STOP : SDA bas → haut pendant que SCL est haut
            sio.gpio_out_clr().write(|w| w.bits(SDA));
            ow_wait(&timer, 5);
            sio.gpio_out_set().write(|w| w.bits(SDA));
            ow_wait(&timer, 10);

            // Relâcher les deux broches (haute impédance) avant de les passer à I²C
            sio.gpio_oe_clr().write(|w| w.bits(SDA | SCL));
            ow_wait(&timer, 100);
        }

        let i2c = hal::I2C::new_controller(
            pac.I2C0,
            pins.gpio4.into_function::<hal::gpio::FunctionI2C>(),
            pins.gpio5.into_function::<hal::gpio::FunctionI2C>(),
            100u32.kHz(),
            &mut pac.RESETS,
            clocks.system_clock.freq(),
        );
        (Some(Bme280Driver::new(i2c)), false)
    } else {
        (None, false)
    };

    // ── Enumération USB + délai ouverture moniteur (5 s) ─────────────────────
    // On poll USB pendant 5 secondes pour laisser le temps d'ouvrir le port.
    wait_ms_usb(&timer, 5_000, &mut usb_dev, &mut serial);

    // ── Découverte DS18B20 ────────────────────────────────────────────────────
    let count = ds_bus.discover(&mut TimerDelay::new(&timer));

    if count > 0 {
        usb_write(&mut serial, b"DS18B20 OK  (GP15)\r\n");
    } else {
        usb_write(&mut serial, b"DS18B20 non detecte -- verifier GP15 et pull-up 4.7k\r\n");
    }

    // ── Init BME280 (optionnel — contrôlé par WITH_BME280) ───────────────────
    let bme_ok = if WITH_BME280 {
        let mut ok = false;
        if let Some(ref mut bme) = bme_opt {
            for _ in 0..3u8 {
                match bme.init() {
                    Ok(()) => {
                        usb_write(&mut serial, b"BME280 OK   (I2C GP4/GP5)\r\n");
                        ok = true;
                        break;
                    }
                    Err(_) => {
                        usb_write(&mut serial, b"BME280 non detecte -- nouvel essai...\r\n");
                        wait_ms_usb(&timer, 1_000, &mut usb_dev, &mut serial);
                    }
                }
            }
            if !ok { usb_write(&mut serial, b"BME280 absent\r\n"); }
        }
        ok
    } else {
        usb_write(&mut serial, b"BME280 desactive (WITH_BME280 = false)\r\n");
        false
    };
    usb_write(&mut serial, b"\r\n");

    // ── DS18B20 → résolution 9-bit (93.75 ms max) ────────────────────────────
    // Config 0x1F = bits 5-6 = 00 → 9-bit (±0.5 °C, 93.75 ms max)
    if count > 0 && ow_reset_raw(&timer) {
        ow_write_byte_raw(&timer, 0xCC); // SKIP ROM
        ow_write_byte_raw(&timer, 0x4E); // WRITE SCRATCHPAD
        ow_write_byte_raw(&timer, 0x55); // TH register
        ow_write_byte_raw(&timer, 0x05); // TL register
        ow_write_byte_raw(&timer, 0x1F); // Config → 9-bit
    }

    // ════════════════════════════════════════════════════════════════════════
    // Boucle de mesure
    // ════════════════════════════════════════════════════════════════════════
    loop {
        usb_dev.poll(&mut [&mut serial]);

        // ── DS18B20 : mesure via fonctions 1-Wire brutes (SKIP ROM) ─────────────
        // Le driver générique (Ds18b20Bus) présente un problème de compatibilité
        // avec ce clone spécifique. Les fonctions brutes ci-dessous (ow_*_raw),
        // qui accèdent directement aux registres SIO, fonctionnent correctement.
        let t_ds_start = timer.get_counter().ticks();
        let conv_started = if count > 0 {
            ow_reset_raw(&timer) && {
                ow_write_byte_raw(&timer, 0xCC); // SKIP ROM
                ow_write_byte_raw(&timer, 0x44); // CONVERT T
                true
            }
        } else { false };

        // Attendre la fin de conversion : 100 ms suffisent pour 9-bit (max 93.75 ms).
        // Ce clone ne signale pas la fin via read-slots → pas de polling, attente fixe.
        wait_ms_usb(&timer, 100, &mut usb_dev, &mut serial);

        // Lecture scratchpad — 4 tentatives avec 15 ms entre chaque.
        // Le clone DS18B20 peut produire des CRC invalides en rafale quand le bus
        // est légèrement bruité (alimentation partagée avec l'I2C du BME280).
        let ds_result: Option<f32> = if conv_started {
            let mut result = None;
            'retry: for _attempt in 0..4u8 {
                if !ow_reset_raw(&timer) { break 'retry; }
                ow_write_byte_raw(&timer, 0xCC); // SKIP ROM
                ow_write_byte_raw(&timer, 0xBE); // READ SCRATCHPAD
                let mut sp = [0u8; 9];
                for b in sp.iter_mut() { *b = ow_read_byte_raw(&timer); }
                // CRC-8 Dallas/Maxim (polynôme 0x8C) — valide si résultat = 0
                let crc = {
                    let mut c: u8 = 0;
                    for &b in sp.iter() {
                        let mut byte = b;
                        for _ in 0..8 {
                            let mix = (c ^ byte) & 1;
                            c >>= 1;
                            if mix != 0 { c ^= 0x8C; }
                            byte >>= 1;
                        }
                    }
                    c
                };
                if crc == 0 {
                    let raw_t = (sp[0] as u16) | ((sp[1] as u16) << 8);
                    result = Some(raw_t as i16 as f32 / 16.0);
                    break 'retry;
                }
                // Attente avant le retry — USB polling actif pour ne pas perdre la connexion
                wait_ms_usb(&timer, 15, &mut usb_dev, &mut serial);
            }
            result
        } else {
            None
        };

        let ds_ms = (timer.get_counter().ticks() - t_ds_start) / 1_000;

        // BME280 : lu à chaque cycle (~15 ms, négligeable)
        let t_bme_start = timer.get_counter().ticks();
        let bme_result = if bme_ok {
            bme_opt.as_mut().and_then(|b| b.measure(&mut CortexDelay).ok())
        } else {
            None
        };
        let bme_ms = if bme_ok { (timer.get_counter().ticks() - t_bme_start) / 1_000 } else { 0 };

        // Formatage
        let mut msg: String<192> = String::new();

        match ds_result {
            Some(t) => {
                let neg  = t < 0.0;
                let abs  = if neg { -t } else { t };
                let sign = if neg { "-" } else { "" };
                let _ = write!(msg, "DS18B20: {}{}.{:02} C",
                    sign, abs as i32, ((abs % 1.0) * 100.0) as u32);
            }
            None => { let _ = write!(msg, "DS18B20: --"); }
        }

        let _ = write!(msg, "  |  ");

        match bme_result {
            Some((t, p, h)) => {
                let tneg  = t < 0.0;
                let tabs  = if tneg { -t } else { t };
                let tsign = if tneg { "-" } else { "" };
                let _ = write!(msg, "BME280: {}{}.{:02} C  {}.{:01} hPa  {}.{} %",
                    tsign, tabs as i32, ((tabs % 1.0) * 100.0) as u32,
                    p as u32, ((p % 1.0) * 10.0) as u32,
                    h as i32, ((h % 1.0) * 10.0) as u32);
            }
            None => { let _ = write!(msg, "BME280: --"); }
        }

        let _ = write!(msg, "  |  DS: {}ms  BME: {}ms\r\n", ds_ms, bme_ms);
        usb_write(&mut serial, msg.as_bytes());

        // Pause 200ms
        wait_ms_usb(&timer, 200, &mut usb_dev, &mut serial);
    }
}
