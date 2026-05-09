//! Test capteur DS18B20 (1-Wire).
//!
//! Flasher sur le Pico (RP2040) ou Pico 2 (RP2350), ouvrir une console defmt (probe-rs run / defmt-print).
//! Toutes les constantes modifiables sont regroupées ici en haut.

#![no_std]
#![no_main]

// ─── Configuration ────────────────────────────────────────────────────────────

/// Broche GPIO connectée à la ligne DATA du bus 1-Wire.
/// Résistance pull-up 4.7kΩ obligatoire entre DATA et 3.3V.
const DATA_PIN: u8 = 22;

/// Délai entre deux lectures (en millisecondes), en plus du temps de conversion.
const LOOP_PERIOD_MS: u32 = 1_000;

/// Nombre maximum de tentatives de lecture du scratchpad en cas de CRC invalide.
/// Avec un pull-up 4.7 kΩ sur breadboard, ~80 % des lectures échouent au CRC.
/// Fix durable : remplacer par 2.2 kΩ pour des fronts montants plus rapides.
const MAX_SCRATCHPAD_RETRIES: u8 = 8;

/// Résolution de la conversion température.
///
/// | Valeur                    | Précision  | Temps de conversion |
/// |---------------------------|------------|---------------------|
/// | MeasureResolution::TC8    | 0.5 °C     | ~94 ms  (9 bits)    |
/// | MeasureResolution::TC4    | 0.25 °C    | ~188 ms (10 bits)   |
/// | MeasureResolution::TC2    | 0.125 °C   | ~375 ms (11 bits)   |
/// | MeasureResolution::TC     | 0.0625 °C  | ~750 ms (12 bits, défaut) |
const RESOLUTION: onewire::ds18b20::MeasureResolution = onewire::ds18b20::MeasureResolution::TC8;

// ─── Dépendances ──────────────────────────────────────────────────────────────

use defmt::{error, info};
use defmt_rtt as _;
use panic_probe as _;

#[cfg(rp2040)] use rp2040_hal as hal;
#[cfg(rp2350)] use rp235x_hal as hal;

/// Bootloader 2e étape requis par le RP2040 pour configurer la flash externe.
#[cfg(rp2040)]
#[unsafe(link_section = ".boot2")]
#[used]
pub static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

/// Signature d'image requise par la ROM du RP2350.
#[cfg(rp2350)]
#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: rp235x_hal::block::ImageDef = rp235x_hal::block::ImageDef::secure_exe();

use core::convert::Infallible;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{ErrorType, InputPin, OutputPin};
use hal::pac;
use onewire::{DeviceSearch, OneWire, DS18B20};

// ─── Open-drain logiciel pour 1-Wire ─────────────────────────────────────────
//
// Le protocole 1-Wire est open-drain : le maître tire la ligne à 0 ou la
// "libère" (haute-impédance) ; une résistance de pull-up externe ramène
// la ligne à VCC quand personne ne tire.
//
// `InOutPin` push-pull ne convient pas : quand onewire appelle `set_high()`
// pour libérer le bus, le RP2040 drive activement 3.3 V. Le DS18B20
// (open-drain, ~4 mA sink) ne peut pas tirer la ligne assez bas contre le
// RP2040 (~12 mA source) → is_low() retourne toujours false → pulse de
// présence non détecté → Ok(None).
//
// Solution : basculer le registre GPIO_OE (output-enable) du SIO :
//   set_low()     → OE=1, OUT=0 → tire à GND
//   set_high()    → OE=0        → haute-impédance, pull-up monte la ligne
//   is_high/low() → lit GPIO_IN (buffer d'entrée toujours actif, OE ignoré)
struct OpenDrainPin {
    pin_mask: u32,
    // Maintient la propriété exclusive du GPIO22 pour que le compilateur
    // détecte tout conflit d'accès concurrent.
    _owner: hal::gpio::Pin<
        hal::gpio::bank0::Gpio22,
        hal::gpio::FunctionSio<hal::gpio::SioInput>,
        hal::gpio::PullNone,
    >,
}

impl OpenDrainPin {
    fn new(
        pin: hal::gpio::Pin<
            hal::gpio::bank0::Gpio22,
            hal::gpio::FunctionSio<hal::gpio::SioInput>,
            hal::gpio::PullNone,
        >,
    ) -> Self {
        let mask = 1u32 << DATA_PIN;
        unsafe {
            let sio = &*pac::SIO::ptr();
            // Pré-charger 0 : quand on activera OE, la ligne sera tirée à GND
            // sans transitoire HIGH.
            sio.gpio_out_clr().write(|w| w.bits(mask));
            // Démarrer en haute-impédance (OE=0).
            sio.gpio_oe_clr().write(|w| w.bits(mask));
        }
        Self { pin_mask: mask, _owner: pin }
    }
}

impl ErrorType for OpenDrainPin {
    type Error = Infallible;
}

impl OutputPin for OpenDrainPin {
    fn set_high(&mut self) -> Result<(), Infallible> {
        // Libérer → OE=0 → haute-impédance → pull-up monte la ligne.
        unsafe { (*pac::SIO::ptr()).gpio_oe_clr().write(|w| w.bits(self.pin_mask)) };
        Ok(())
    }

    fn set_low(&mut self) -> Result<(), Infallible> {
        // Tirer à GND → OE=1 (OUT est déjà 0).
        unsafe { (*pac::SIO::ptr()).gpio_oe_set().write(|w| w.bits(self.pin_mask)) };
        Ok(())
    }
}

impl InputPin for OpenDrainPin {
    fn is_high(&mut self) -> Result<bool, Infallible> {
        Ok(unsafe { (*pac::SIO::ptr()).gpio_in().read().bits() } & self.pin_mask != 0)
    }

    fn is_low(&mut self) -> Result<bool, Infallible> {
        Ok(unsafe { (*pac::SIO::ptr()).gpio_in().read().bits() } & self.pin_mask == 0)
    }
}

// ─── Point d'entrée ──────────────────────────────────────────────────────────
//
// Câblage :
//   GPIO22 ──┬── 4.7 kΩ ── 3.3 V   (pull-up externe obligatoire)
//            └── DATA du DS18B20
//   GND    ── GND du DS18B20
//   3.3 V  ── VDD du DS18B20  (ou VDD=GND en mode parasite → parasite_mode=true)

#[hal::entry]
fn main() -> ! {
    let mut pac = pac::Peripherals::take().unwrap();
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);

    let clocks = hal::clocks::init_clocks_and_plls(
        12_000_000,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .unwrap();

    // Le timer sert de source de délai pour le protocole 1-Wire.
    #[cfg(rp2040)]
    let mut timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);
    #[cfg(rp2350)]
    let mut timer = hal::Timer::new_timer0(pac.TIMER0, &mut pac.RESETS, &clocks);

    let sio = hal::Sio::new(pac.SIO);
    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // into_floating_input() → FUNCSEL=SIO, pas de pull interne.
    // Le pull-up externe 4.7 kΩ suffit ; un pull interne (~47 kΩ) en parallèle
    // serait inoffensif mais inutile.
    let one_wire_pin = OpenDrainPin::new(pins.gpio22.into_floating_input());
    // parasite_mode = false : le DS18B20 est alimenté par VDD (pas parasite).
    let mut ow = OneWire::new(one_wire_pin, false);

    info!("Test DS18B20 — recherche sur GPIO{}...", DATA_PIN);

    // ── Recherche du premier DS18B20 sur le bus ───────────────────────────────
    // On conserve une copie de Device pour pouvoir envoyer WriteScratchpad
    // après la création du DS18B20 (dont le champ `device` est privé).
    let (sensor, device_addr) = loop {
        let mut search = DeviceSearch::new_for_family(onewire::ds18b20::FAMILY_CODE);
        match ow.search_next(&mut search, &mut timer) {
            Ok(Some(device)) => {
                info!("DS18B20 trouvé : {}", device);
                let addr = device.clone();
                match DS18B20::new(device) {
                    Ok(s) => break (s, addr),
                    Err(_) => error!("Family code inattendu"),
                }
            }
            Ok(None) => error!("Aucun DS18B20 sur le bus — vérifier câblage et pull-up 4.7kΩ"),
            Err(_) => error!("Erreur bus 1-Wire"),
        }
        timer.delay_ms(2_000);
    };

    // Configurer la résolution du capteur via WriteScratchpad (commande 0x4E).
    // Format : [cmd, TH_alarm, TL_alarm, config_register].
    // Les alarmes sont désactivées (0x00). Le registre de config encode la résolution.
    match ow.reset_select_write_only(
        &mut timer,
        &device_addr,
        &[onewire::ds18b20::Command::WriteScratchpad as u8, 0x00, 0x00, RESOLUTION as u8],
    ) {
        Ok(()) => info!("Résolution configurée : {} ms", RESOLUTION.time_ms()),
        Err(_) => error!("Impossible de configurer la résolution"),
    }

    // ── Boucle de mesure ──────────────────────────────────────────────────────
    let wait_ms = RESOLUTION.time_ms() as u32 + 50;

    loop {
        // 1. Déclencher la conversion.
        match sensor.measure_temperature(&mut ow, &mut timer) {
            Ok(_) => {
                // 2. Attendre la fin de conversion + 50 ms de marge de sécurité.
                //    Le temps est dérivé de RESOLUTION, pas de ce que le capteur retourne.
                timer.delay_ms(wait_ms);

                // 3. Lire le scratchpad avec retry (CRC sur 72 bits est sensible au bruit).
                //    Chaque tentative relance reset + MATCH_ROM + READ_SCRATCHPAD.
                //    Fix durable : remplacer le pull-up 4.7 kΩ par 2.2 kΩ.
                let mut attempt: u8 = 0;
                loop {
                    attempt += 1;
                    match sensor.read_temperature(&mut ow, &mut timer) {
                        Ok(raw) => {
                            let celsius = raw as i16 as f32 / 16.0;
                            if attempt > 1 {
                                info!("Température : {=f32} °C (essai {})", celsius, attempt);
                            } else {
                                info!("Température : {=f32} °C", celsius);
                            }
                            break;
                        }
                        Err(_) => {
                            if attempt >= MAX_SCRATCHPAD_RETRIES {
                                error!("Échec lecture après {} essais — vérifier pull-up", MAX_SCRATCHPAD_RETRIES);
                                break;
                            }
                        }
                    }
                }
            }
            Err(_) => error!("Erreur démarrage conversion"),
        }

        timer.delay_ms(LOOP_PERIOD_MS);
    }
}
