//! Sanity check de bring-up : fait clignoter la LED embarquée du Pico
//! (GP25, pas utilisée par le câblage de la chambre — cf.
//! `config::wiring`) à 1 Hz, en journalisant chaque battement. Sert à
//! vérifier que la chaîne complète (toolchain, `flip-link`, règles udev,
//! `probe-rs`, attache RTT) fonctionne avant de passer à un bin qui touche
//! du vrai matériel de la chambre.
//!
//! Un seul message au démarrage aurait pu se perdre si l'attache RTT de
//! `probe-rs` arrive après coup (course entre le reset/flash et le moment
//! où le viewer RTT commence à lire) — un message par bascule permet de
//! voir si le programme tourne (LED + logs) même si les tout premiers
//! logs ont été manqués.
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

#[unsafe(link_section = ".boot2")]
#[used]
static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

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

    defmt::info!("blinky demarre (GP25, 1 Hz) — un log par bascule ci-dessous");

    let mut on = false;
    let mut tick: u32 = 0;
    loop {
        on = !on;
        if on { led.set_high().unwrap(); } else { led.set_low().unwrap(); }
        defmt::info!("tick {} : led = {}", tick, on);
        tick = tick.wrapping_add(1);
        timer.delay_ms(500);
    }
}
