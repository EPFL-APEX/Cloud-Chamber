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

use cloud_chamber_firmware::sensors::ds18b20::Ds18b20Bus;
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

struct CortexDelay;

impl DelayNs for CortexDelay {
    fn delay_ns(&mut self, ns: u32) {
        // RP2040 @ 125 MHz : 1 cycle ≈ 8 ns, asm::delay(N) ≈ N×3 cycles ≈ N×24 ns
        cortex_m::asm::delay(ns / 24 + 1);
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

struct OpenDrainPin {
    mask: u32,
    _owner: hal::gpio::Pin<
        hal::gpio::bank0::Gpio15,
        hal::gpio::FunctionSio<hal::gpio::SioInput>,
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
        let mask = 1u32 << OW_PIN;
        unsafe {
            // ── Fix pad register ────────────────────────────────────────────
            // PADS_BANK0_GPIO15 = 0x4001_C000 + 0x04 + 15×4 = 0x4001_C040
            // bit7 = OD (output disable) — DOIT être 0 pour que gpio_oe_set() drive la broche
            // bit6 = IE (input enable)   — DOIT être 1 pour lire l'état de la ligne
            // into_floating_input() peut laisser OD=1 dans certaines versions de rp2040-hal,
            // ce qui bloquerait totalement notre open-drain même avec SIO_OE=1.
            let pad = 0x4001_C040 as *mut u32;
            let val = core::ptr::read_volatile(pad);
            core::ptr::write_volatile(pad, (val & !0x80) | 0x40); // OD=0, IE=1

            let sio = &*pac::SIO::ptr();
            sio.gpio_out_clr().write(|w| w.bits(mask)); // OUT=0 (driver bas quand OE=1)
            sio.gpio_oe_clr().write(|w| w.bits(mask));  // OE=0  (haute-Z initialement)
        }
        Self { mask, _owner: pin }
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

    // ── I²C GP4/GP5 à 100 kHz ────────────────────────────────────────────────
    use rp2040_hal::fugit::RateExtU32;
    let i2c = hal::I2C::new_controller(
        pac.I2C0,
        pins.gpio4.into_function::<hal::gpio::FunctionI2C>(),
        pins.gpio5.into_function::<hal::gpio::FunctionI2C>(),
        100u32.kHz(),
        &mut pac.RESETS,
        clocks.system_clock.freq(),
    );

    // ── Drivers capteurs ──────────────────────────────────────────────────────
    let mut ds_bus = Ds18b20Bus::new(OpenDrainPin::new(pins.gpio15.into_floating_input()));
    let mut bme    = Bme280Driver::new(i2c);

    // ── Enumération USB + délai ouverture moniteur (5 s) ─────────────────────
    // On poll USB pendant 5 secondes pour laisser le temps d'ouvrir le port.
    wait_ms_usb(&timer, 5_000, &mut usb_dev, &mut serial);

    // ── Diagnostic GPIO15 open-drain ─────────────────────────────────────────
    // Teste si la broche peut réellement driver bas (vérifie OD pad + SIO_OE).
    // Si force_bas = false → le pad OD bloque la sortie (ou court-circuit)
    // Si relache_haut = false → pull-up absent ou trop faible
    {
        // Forcer bas via OE=1 (OUT déjà à 0 depuis OpenDrainPin::new)
        unsafe { (*pac::SIO::ptr()).gpio_oe_set().write(|w| w.bits(1u32 << 15)) };
        wait_ms_usb(&timer, 200, &mut usb_dev, &mut serial);
        let driven_low = unsafe {
            ((*pac::SIO::ptr()).gpio_in().read().bits() >> 15) & 1 == 0
        };
        // Relâcher → pull-up doit remonter la ligne
        unsafe { (*pac::SIO::ptr()).gpio_oe_clr().write(|w| w.bits(1u32 << 15)) };
        wait_ms_usb(&timer, 200, &mut usb_dev, &mut serial);
        let released_high = unsafe {
            ((*pac::SIO::ptr()).gpio_in().read().bits() >> 15) & 1 == 1
        };

        let mut msg: String<128> = String::new();
        let _ = write!(msg,
            "GPIO15 diag: force_bas={} (ok=true)  relache_haut={} (ok=true)\r\n",
            driven_low, released_high);
        usb_write(&mut serial, msg.as_bytes());
        if !driven_low {
            usb_write(&mut serial, b"  ERREUR: GP15 ne drive pas bas - OD pad bloque ou court-circuit\r\n");
        }
        if !released_high {
            usb_write(&mut serial, b"  ERREUR: GP15 reste bas apres relachement - pull-up 4.7k absent?\r\n");
        }
        usb_write(&mut serial, b"\r\n");
    }

    // ── Découverte DS18B20 ────────────────────────────────────────────────────
    let count = ds_bus.discover(&mut CortexDelay);

    // ── En-tête ───────────────────────────────────────────────────────────────
    usb_write(&mut serial, b"==============================\r\n");
    usb_write(&mut serial, b"  DS18B20 + BME280  (RP2040)\r\n");
    usb_write(&mut serial, b"  DS18B20: GP15 | BME280: GP4/GP5\r\n");
    usb_write(&mut serial, b"==============================\r\n\r\n");

    {
        let mut msg: String<64> = String::new();
        let _ = write!(msg, "DS18B20 detectes: {}\r\n\r\n", count);
        usb_write(&mut serial, msg.as_bytes());
    }

    // Répète le compte 3 fois (espacées d'1 s) pour ne pas le rater.
    for _ in 0..3 {
        wait_ms_usb(&timer, 1_000, &mut usb_dev, &mut serial);
        let mut msg: String<64> = String::new();
        let _ = write!(msg, "  >>> DS18B20 detectes: {} <<<\r\n", count);
        usb_write(&mut serial, msg.as_bytes());
    }
    usb_write(&mut serial, b"\r\n");

    // ── Init BME280 (retry jusqu'au succès) ───────────────────────────────────
    loop {
        match bme.init() {
            Ok(()) => { usb_write(&mut serial, b"BME280 OK\r\n\r\n"); break; }
            Err(_) => {
                usb_write(&mut serial, b"BME280 non detecte...\r\n");
                wait_ms_usb(&timer, 2_000, &mut usb_dev, &mut serial);
            }
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    // Boucle de mesure
    // ════════════════════════════════════════════════════════════════════════
    let mut n: u32 = 0;
    loop {
        n += 1;
        usb_dev.poll(&mut [&mut serial]);

        // Rappel périodique du nombre de capteurs détectés (toutes les 20 mesures)
        if n % 20 == 1 {
            let mut msg: String<64> = String::new();
            let _ = write!(msg, "--- DS18B20 detectes: {} ---\r\n", count);
            usb_write(&mut serial, msg.as_bytes());
        }

        // DS18B20 : envoie Convert T et revient immédiatement
        let ds_started = count > 0
            && ds_bus.start_conversion(0, &mut CortexDelay).is_ok();

        // Attendre 800ms en polant USB (pas de blocage dur)
        wait_ms_usb(&timer, 800, &mut usb_dev, &mut serial);

        let ds_result = if ds_started {
            ds_bus.read_celsius(0, &mut CortexDelay).ok()
        } else {
            None
        };

        // BME280 : mesure bloquante ~15ms
        let bme_result = bme.measure(&mut CortexDelay).ok();

        // Formatage
        let mut msg: String<128> = String::new();
        let _ = write!(msg, "[{}] ", n);

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
                let _ = write!(msg, "BME280: {}{}.{:02} C  Pres: {}.{:01} hPa  Humi: {}.{} %",
                    tsign, tabs as i32, ((tabs % 1.0) * 100.0) as u32,
                    p as u32, ((p % 1.0) * 10.0) as u32,
                    h as i32, ((h % 1.0) * 10.0) as u32);
            }
            None => { let _ = write!(msg, "BME280: --"); }
        }

        let _ = write!(msg, "\r\n");
        usb_write(&mut serial, msg.as_bytes());

        // Pause 200ms
        wait_ms_usb(&timer, 200, &mut usb_dev, &mut serial);
    }
}
