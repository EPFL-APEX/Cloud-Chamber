//! Test capteur de tension via ADC.
//!
//! Flasher sur le Pico (RP2040) ou Pico 2 (RP2350), ouvrir une console defmt (probe-rs run / defmt-print).
//! Toutes les constantes modifiables sont regroupées ici en haut.

#![no_std]
#![no_main]

// ─── Configuration ────────────────────────────────────────────────────────────

/// Facteur de conversion ADC brut → Volts.
/// Formule : (brut / 4095) × Vref × (R1+R2)/R2
/// Exemple avec diviseur 1/11 (R1=100kΩ, R2=10kΩ) et Vref=3.3V → max ≈ 36 V
const VOLTAGE_SCALE: f32 = 3.3 / 4095.0 * 11.0;

/// Tension nominale attendue (V) — affiché uniquement à titre de référence.
const NOMINAL_VOLTAGE: f32 = 24.0;

/// Délai entre deux lectures (en millisecondes).
const LOOP_PERIOD_MS: u32 = 500;

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

use hal::pac;

// ─── Point d'entrée ──────────────────────────────────────────────────────────
//
// Câblage : GPIO26 (ADC0) ← pont diviseur ← tension à mesurer
//           GPIO26 doit rester entre 0 V et 3.3 V en permanence.

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
    // GPIO26 = ADC0. Pour ADC1 utiliser gpio27, ADC2 → gpio28, ADC3 → gpio29.
    let mut adc_pin = hal::adc::AdcPin::new(pins.gpio26.into_floating_input()).unwrap();

    info!(
        "Test tension — VOLTAGE_SCALE={=f32:.6}, nominale={=f32:.1} V",
        VOLTAGE_SCALE,
        NOMINAL_VOLTAGE
    );

    loop {
        let raw: u16 = adc.read(&mut adc_pin).unwrap();
        let voltage = raw as f32 * VOLTAGE_SCALE;
        let error_v = voltage - NOMINAL_VOLTAGE;

        info!(
            "ADC={} | Tension={=f32:.3} V | Écart nominale={=f32:+.3} V",
            raw, voltage, error_v
        );

        cortex_m::asm::delay(LOOP_PERIOD_MS * 125_000);
    }
}
