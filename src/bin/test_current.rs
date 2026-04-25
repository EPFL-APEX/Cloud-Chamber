//! Test capteur de courant via ADC.
//!
//! Flasher sur le Pico (RP2040) ou Pico 2 (RP2350), ouvrir une console defmt (probe-rs run / defmt-print).
//! Toutes les constantes modifiables sont regroupées ici en haut.

#![no_std]
#![no_main]

// ─── Configuration ────────────────────────────────────────────────────────────

/// Facteur de conversion ADC brut → Ampères.
/// Formule : (brut / 4095) × Vref / (Rshunt × Gain_ampli)
/// Exemple : Rshunt=0.1Ω, gain=1, Vref=3.3V → max ≈ 33 A
const CURRENT_SCALE: f32 = 3.3 / 4095.0 / 0.1;

/// Courant nominal attendu (A) — affiché à titre de référence.
const NOMINAL_CURRENT: f32 = 5.0;

/// Seuil de surcourant pour avertissement dans le log (A).
const OVERCURRENT_THRESHOLD: f32 = 10.0;

/// Délai entre deux lectures (en millisecondes).
const LOOP_PERIOD_MS: u32 = 200;

// ─── Dépendances ──────────────────────────────────────────────────────────────

use defmt::{info, warn};
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

use hal::pac;

// ─── Point d'entrée ──────────────────────────────────────────────────────────
//
// Câblage : GPIO27 (ADC1) ← sortie amplificateur de courant (ex: INA219, ACS712)
//           La tension en GPIO27 doit rester entre 0 V et 3.3 V.

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

    let mut adc = hal::Adc::new(pac.ADC, &mut pac.RESETS);
    // GPIO27 = ADC1. Adapter selon le canal utilisé.
    let mut adc_pin = hal::adc::AdcPin::new(pins.gpio27.into_floating_input()).unwrap();

    info!(
        "Test courant — CURRENT_SCALE={=f32:.6}, nominal={=f32:.1} A, seuil={=f32:.1} A",
        CURRENT_SCALE,
        NOMINAL_CURRENT,
        OVERCURRENT_THRESHOLD
    );

    loop {
        let raw: u16 = adc.read(&mut adc_pin).unwrap();
        let amperes = raw as f32 * CURRENT_SCALE;

        if amperes > OVERCURRENT_THRESHOLD {
            warn!("SURCOURANT ! {=f32:.3} A > {=f32:.1} A", amperes, OVERCURRENT_THRESHOLD);
        } else {
            info!("ADC={} | Courant={=f32:.3} A", raw, amperes);
        }

        cortex_m::asm::delay(LOOP_PERIOD_MS * 125_000);
    }
}
