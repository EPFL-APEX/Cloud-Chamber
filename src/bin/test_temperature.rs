//! Test capteur DS18B20 (1-Wire) — utilise le driver Ds18b20Bus du projet.
//!
//! Flasher sur le Pico (RP2040) ou Pico 2 (RP2350), ouvrir une console defmt
//! (probe-rs run / defmt-print).
//! Toutes les constantes modifiables sont regroupées ici en haut.

#![no_std]
#![no_main]

// ─── Configuration ────────────────────────────────────────────────────────────

/// Broche GPIO connectée à la ligne DATA du bus 1-Wire.
/// Résistance pull-up 4.7kΩ obligatoire entre DATA et 3.3V.
const DATA_PIN: u8 = 22;

/// Délai entre deux cycles de lecture (en millisecondes).
const LOOP_PERIOD_MS: u32 = 1_000;

/// Résolution de conversion.
///
/// | Variante       | Précision  | Temps de conversion |
/// |----------------|------------|---------------------|
/// | `Bits9`        | ±0.5 °C    | ~150 ms             |
/// | `Bits10`       | ±0.25 °C   | ~240 ms             |
/// | `Bits11`       | ±0.125 °C  | ~430 ms             |
/// | `Bits12`       | ±0.0625 °C | ~800 ms (défaut)    |
const RESOLUTION: Resolution = Resolution::Bits9;

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

use cloud_chamber::drivers::ds18b20::{Ds18b20Bus, Resolution};

// ─── Open-drain logiciel pour 1-Wire ─────────────────────────────────────────
//
// Le RP2040/RP2350 ne dispose pas de GPIO open-drain natif.
//   set_low()  → OE=1, OUT=0 → tire la ligne à GND
//   set_high() → OE=0        → haute-impédance, pull-up remonte la ligne
//   is_*()     → lit GPIO_IN (buffer d'entrée toujours actif, indépendant de OE)
//
// On convertit d'abord la broche en push_pull_output pour que le HAL configure
// correctement FUNCSEL=SIO dans IO_BANK0, puis on repasse en haute-Z via SIO_OE_CLR.
struct OpenDrainPin {
    pin_mask: u32,
    _owner: hal::gpio::Pin<
        hal::gpio::bank0::Gpio22,
        hal::gpio::FunctionSio<hal::gpio::SioOutput>,
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
        let out_pin = pin.into_push_pull_output();
        let mask = 1u32 << DATA_PIN;
        unsafe {
            let sio = &*pac::SIO::ptr();
            sio.gpio_out_clr().write(|w| w.bits(mask)); // OUT=0 pré-chargé
            sio.gpio_oe_clr().write(|w| w.bits(mask));  // démarrer en haute-Z
        }
        Self { pin_mask: mask, _owner: out_pin }
    }
}

impl ErrorType for OpenDrainPin { type Error = Infallible; }

impl OutputPin for OpenDrainPin {
    fn set_high(&mut self) -> Result<(), Infallible> {
        unsafe { (*pac::SIO::ptr()).gpio_oe_clr().write(|w| w.bits(self.pin_mask)) };
        Ok(())
    }
    fn set_low(&mut self) -> Result<(), Infallible> {
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
//   GPIO22 ──┬── 4.7 kΩ ── 3.3V   (pull-up externe obligatoire)
//            └── DATA du DS18B20
//   GND    ── GND du DS18B20
//   3.3V   ── VDD du DS18B20

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

    let mut bus = Ds18b20Bus::new(OpenDrainPin::new(pins.gpio22.into_floating_input()));

    info!("Test DS18B20 — recherche sur GPIO{}...", DATA_PIN);

    let count = bus.discover(&mut timer);
    if count > 0 {
        info!("{} capteur(s) DS18B20 trouvé(s)", count);
        for idx in 0..count {
            match bus.set_resolution(idx, &mut timer, RESOLUTION) {
                Ok(())  => info!("Capteur {} — résolution configurée", idx),
                Err(_)  => error!("Capteur {} — échec configuration résolution", idx),
            }
        }
    } else {
        error!("Aucun DS18B20 sur le bus — vérifier câblage et pull-up 4.7kΩ sur GPIO{}", DATA_PIN);
    }

    loop {
        for idx in 0..count {
            match bus.start_conversion(idx, &mut timer) {
                Ok(()) => {
                    timer.delay_ms(RESOLUTION.conversion_time_ms());
                    match bus.read_celsius(idx, &mut timer) {
                        Ok(temp) => info!("Capteur {}: {=f32} °C", idx, temp),
                        Err(_)   => error!("Capteur {}: erreur lecture (CRC ou bus)", idx),
                    }
                }
                Err(_) => error!("Capteur {}: erreur démarrage conversion", idx),
            }
        }
        timer.delay_ms(LOOP_PERIOD_MS);
    }
}
