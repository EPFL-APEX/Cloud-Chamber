//! Sanity check de bring-up : fait clignoter la LED embarquée du Pico
//! (GP25, pas utilisée par le câblage de la chambre — cf.
//! `config::wiring`) à 1 Hz. Sert à vérifier que la chaîne complète
//! (toolchain, `flip-link`, règles udev, `probe-rs`) fonctionne avant de
//! passer à un bin qui touche du vrai matériel de la chambre.
//!
//! RP2040 uniquement, même limite que `identify_temp_sensors`. Derrière la
//! feature `bin-blinky` (désactivée par défaut) pour ne pas être construit
//! par les jobs CI `cargo check` sur les autres cibles :
//!
//! ```text
//! cargo run --target thumbv6m-none-eabi --features bin-blinky --bin blinky
//! ```
#![no_std]
#![no_main]

use defmt_rtt as _;
use panic_probe as _;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use rp2040_hal::{self as hal, Sio, Watchdog, clocks::init_clocks_and_plls, gpio::Pins, pac};

/// Fréquence du cristal externe du Pico — cf. `hal::clocks::init_clocks_and_plls`.
const XOSC_CRYSTAL_FREQ: u32 = 12_000_000;

#[hal::entry]
fn main() -> ! {
    let mut pac = pac::Peripherals::take().unwrap();
    let mut watchdog = Watchdog::new(pac.WATCHDOG);

    let clocks = init_clocks_and_plls(
        XOSC_CRYSTAL_FREQ,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();

    let mut timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

    let sio = Sio::new(pac.SIO);
    let pins = Pins::new(pac.IO_BANK0, pac.PADS_BANK0, sio.gpio_bank0, &mut pac.RESETS);
    let mut led = pins.gpio25.into_push_pull_output();

    defmt::info!("blinky demarre (GP25, 1 Hz)");

    loop {
        led.set_high().unwrap();
        timer.delay_ms(500);
        led.set_low().unwrap();
        timer.delay_ms(500);
    }
}
