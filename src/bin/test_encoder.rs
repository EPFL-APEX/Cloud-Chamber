//! Test encodeur rotatif (3 broches : A, B, bouton).
//!
//! Flasher sur le Pico (RP2040) ou Pico 2 (RP2350), ouvrir une console defmt (probe-rs run / defmt-print).
//! Toutes les constantes modifiables sont regroupées ici en haut.

#![no_std]
#![no_main]

// ─── Configuration ────────────────────────────────────────────────────────────

/// Fréquence de polling de l'encodeur (en millisecondes).
/// Valeur typique : 5–20 ms. En dessous de 5 ms, les rebonds peuvent créer
/// des faux événements selon le modèle d'encodeur utilisé.
const POLL_PERIOD_MS: u32 = 10;

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

use cloud_chamber::drivers::encoder::{EncoderEvent, RotaryEncoder};
use hal::pac;

// ─── Point d'entrée ──────────────────────────────────────────────────────────
//
// Câblage :
//   GPIO10 ← signal A  (pull-up interne)
//   GPIO11 ← signal B  (pull-up interne)
//   GPIO12 ← bouton SW (pull-up interne, contact NO vers GND)
//
// Pour changer les broches, remplacer gpio10/gpio11/gpio12 ci-dessous.

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

    let pin_a = pins.gpio10.into_pull_up_input();
    let pin_b = pins.gpio11.into_pull_up_input();
    let pin_sw = pins.gpio12.into_pull_up_input();

    let mut encoder = RotaryEncoder::new(pin_a, pin_b, pin_sw);
    let mut position: i32 = 0;

    info!("Test encodeur — polling={}ms", POLL_PERIOD_MS);

    loop {
        match encoder.poll() {
            EncoderEvent::RotateClockwise => {
                position += 1;
                info!("→ Horaire      | position={}", position);
            }
            EncoderEvent::RotateCounterClockwise => {
                position -= 1;
                info!("← Anti-horaire | position={}", position);
            }
            EncoderEvent::ButtonPressed => {
                info!("⏎ Bouton pressé | position={}", position);
            }
            EncoderEvent::None => {}
        }

        cortex_m::asm::delay(POLL_PERIOD_MS * 125_000);
    }
}
