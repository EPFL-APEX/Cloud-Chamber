//! Sanity check de bring-up : instancie `drivers::encoder::RotaryEncoder`
//! sur le matériel réel et journalise chaque événement détecté (rotation
//! horaire/anti-horaire, appui bouton) — tourne l'encodeur et presse le
//! bouton en main pour vérifier le câblage et le sens de rotation avant de
//! le brancher dans l'UI.
//!
//! `RotaryEncoder` n'a aujourd'hui aucun appelant dans le reste du repo :
//! ce bin est le premier test sur du vrai matériel.
//!
//! Broches tirées directement de `config::wiring`
//! (`PIN_ENCODER_A`/`PIN_ENCODER_B`/`PIN_ENCODER_SW`), configurées par
//! `board::configure_input_pin` — cf. `board`.
//!
//! Pull-up interne sur les trois broches : standard pour un encodeur
//! mécanique dont le commun est au GND (contact = tire à la masse, repos =
//! haut). Si le tien est câblé autrement (commun au 3.3V), les événements
//! sortiront inversés ou bruités — observable directement dans les logs
//! ci-dessous, à ajuster si besoin une fois testé.
//!
//! # Cadence de poll
//!
//! `RotaryEncoder::poll` ne regarde qu'un front montant sur A et lit B à
//! cet instant précis — pas de machine à états complète sur les 4
//! transitions de quadrature. Un poll trop lent par rapport à la vitesse
//! de rotation manque des transitions ou les lit à moitié faites (A et B
//! pas encore synchrones), ce qui peut renvoyer le mauvais sens sans que
//! rien ne soit cassé côté câblage. 1 ms laisse largement plus de marge
//! que les 10 ms d'origine face à un cycle de quadrature complet, qui peut
//! survenir en 10-20 ms à vitesse de rotation normale.
//!
//! RP2040 uniquement pour l'instant, même limite que les autres bins de
//! bring-up. Derrière la feature `bin-encoder-test` (désactivée par
//! défaut) pour ne pas être construit par les jobs CI `cargo check` sur
//! les autres cibles :
//!
//! ```text
//! cargo run --target thumbv6m-none-eabi --features bin-encoder-test \
//!     --bin encoder_test
//! ```
#![no_std]
#![no_main]

use defmt_rtt as _;
use panic_probe as _;

use embedded_hal::delay::DelayNs;
use rp2040_hal as hal;

use cloud_chamber_firmware::board;
use cloud_chamber_firmware::config::wiring::{PIN_ENCODER_A, PIN_ENCODER_B, PIN_ENCODER_SW};
use cloud_chamber_firmware::drivers::encoder::{EncoderEvent, RotaryEncoder};

#[hal::entry]
fn main() -> ! {
    let mut board = board::init();

    let pin_a = board::configure_input_pin(PIN_ENCODER_A);
    let pin_b = board::configure_input_pin(PIN_ENCODER_B);
    let pin_sw = board::configure_input_pin(PIN_ENCODER_SW);
    let mut encoder = RotaryEncoder::new(pin_a, pin_b, pin_sw);

    defmt::info!(
        "encoder_test demarre — A=GP{} B=GP{} SW=GP{}, poll toutes les 1ms",
        PIN_ENCODER_A,
        PIN_ENCODER_B,
        PIN_ENCODER_SW
    );

    loop {
        match encoder.poll() {
            EncoderEvent::RotateClockwise => defmt::info!("rotation horaire"),
            EncoderEvent::RotateCounterClockwise => defmt::info!("rotation anti-horaire"),
            EncoderEvent::ButtonPressed => defmt::info!("bouton presse"),
            EncoderEvent::None => {}
        }
        board.timer.delay_ms(1);
    }
}
