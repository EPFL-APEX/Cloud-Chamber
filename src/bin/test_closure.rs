//! Test capteur de fermeture de chambre (contact sec via GPIO).
//!
//! Flasher sur le Pico (RP2040) ou Pico 2 (RP2350), ouvrir une console defmt (probe-rs run / defmt-print).
//! Toutes les constantes modifiables sont regroupées ici en haut.

#![no_std]
#![no_main]

// ─── Configuration ────────────────────────────────────────────────────────────

/// `true` → contact normalement ouvert (NO) avec pull-up : chambre fermée = LOW.
/// `false` → contact normalement fermé (NF) avec pull-down : chambre fermée = HIGH.
const ACTIVE_LOW: bool = true;

/// Délai entre deux lectures (en millisecondes).
const LOOP_PERIOD_MS: u32 = 100;

// ─── Dépendances ──────────────────────────────────────────────────────────────

use defmt::info;
use defmt_rtt as _;
use panic_probe as _;

#[cfg(rp2040)] use rp2040_hal as hal;
#[cfg(rp2350)] use rp235x_hal as hal;

#[cfg(rp2040)]
#[unsafe(link_section = ".boot2")]
#[used]
pub static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

#[cfg(rp2350)]
#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: rp235x_hal::block::ImageDef = rp235x_hal::block::ImageDef::secure_exe();

use cloud_chamber::drivers::closure::GpioClosureSensor;
use cloud_chamber::cloud_chamber_hal::sensors::ClosureSensor;
use hal::pac;

// ─── Point d'entrée ──────────────────────────────────────────────────────────
//
// Câblage (contact NO + pull-up) :
//   GPIO15 ──┬── 10kΩ ── 3.3V
//            └── contact ── GND
// Quand le contact se ferme (chambre fermée), GPIO15 passe à LOW.

#[hal::entry]
fn main() -> ! {
    let mut pac = pac::Peripherals::take().unwrap();
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);

    let _clocks = hal::clocks::init_clocks_and_plls(
        12_000_000,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .unwrap();

    let sio = hal::Sio::new(pac.SIO);
    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // Pull-up interne activé — adapter gpio15 si un autre pin est utilisé.
    let pin = pins.gpio15.into_pull_up_input();
    let mut sensor = GpioClosureSensor::new(pin, ACTIVE_LOW);

    info!("Test fermeture — ACTIVE_LOW={}", ACTIVE_LOW);

    let mut last_state: Option<bool> = None;

    loop {
        let closed = sensor.is_closed().unwrap();

        // Log seulement sur changement d'état pour réduire le bruit
        if last_state != Some(closed) {
            if closed {
                info!("Chambre FERMÉE");
            } else {
                info!("Chambre OUVERTE");
            }
            last_state = Some(closed);
        }

        cortex_m::asm::delay(LOOP_PERIOD_MS * 125_000);
    }
}
