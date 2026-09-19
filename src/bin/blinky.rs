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
use rp2040_hal as hal;

use cloud_chamber_firmware::board;

#[hal::entry]
fn main() -> ! {
    let mut board = board::init();
    let mut led = board.pins.gpio25.into_push_pull_output();

    defmt::info!("blinky demarre (GP25, 1 Hz) — un log par bascule ci-dessous");

    let mut on = false;
    let mut tick: u32 = 0;
    loop {
        on = !on;
        if on { led.set_high().unwrap(); } else { led.set_low().unwrap(); }
        defmt::info!("tick {} : led = {}", tick, on);
        tick = tick.wrapping_add(1);
        board.timer.delay_ms(500);
    }
}
