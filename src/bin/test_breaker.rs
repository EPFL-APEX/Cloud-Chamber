//! Test disjoncteur / relai via GPIO.
//!
//! Flasher sur le Pico (RP2040) ou Pico 2 (RP2350), ouvrir une console defmt (probe-rs run / defmt-print).
//! Toutes les constantes modifiables sont regroupées ici en haut.

#![no_std]
#![no_main]

// ─── Configuration ────────────────────────────────────────────────────────────

/// `true` → broche HIGH = relai déclenché (bobine active-haute).
/// `false` → broche LOW  = relai déclenché (bobine active-basse, plus courant).
const ACTIVE_HIGH: bool = false;

/// Durée pendant laquelle le relai reste déclenché (en millisecondes).
const TRIP_DURATION_MS: u32 = 2_000;

/// Durée pendant laquelle le relai reste réinitialisé (en millisecondes).
const RESET_DURATION_MS: u32 = 3_000;

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

use cloud_chamber::drivers::breaker::GpioBreaker;
use cloud_chamber::cloud_chamber_hal::actuators::BreakerActuator;
use embedded_hal::digital::OutputPin;
use hal::pac;

// ─── Point d'entrée ──────────────────────────────────────────────────────────
//
// Câblage :
//   GPIO20 → IN du module relai
//   Alimentation du module relai : 3.3V ou 5V selon le modèle.
//   ATTENTION : ne jamais dépasser la tension/courant max du relai.
//
// Pour changer la broche, remplacer gpio20 ci-dessous.

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
    let mut pins = hal::gpio::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // Broche sortie push-pull — le relai sera à l'état repos au démarrage.
    let pin = pins.gpio20.into_push_pull_output();
    let mut breaker = GpioBreaker::new(pin, ACTIVE_HIGH);

    // S'assurer que le relai part en état repos
    breaker.reset().ok();

    info!(
        "Test disjoncteur — ACTIVE_HIGH={}, trip={}ms, reset={}ms",
        ACTIVE_HIGH, TRIP_DURATION_MS, RESET_DURATION_MS
    );

    loop {
        breaker.trip().ok();
        info!("Relai DÉCLENCHÉ (tripped={})", breaker.is_tripped().unwrap());
        cortex_m::asm::delay(TRIP_DURATION_MS * 125_000);

        breaker.reset().ok();
        info!("Relai RÉINITIALISÉ (tripped={})", breaker.is_tripped().unwrap());
        cortex_m::asm::delay(RESET_DURATION_MS * 125_000);
    }
}
