//! Sanity check de bring-up : journalise chaque événement de l'encodeur
//! rotatif (rotation horaire/anti-horaire, appui bouton) — tourne la
//! molette et presse le bouton en main pour vérifier le câblage et le sens
//! de rotation avant de le brancher dans l'UI.
//!
//! # Il scrute l'encodeur exactement comme le firmware
//!
//! Le montage passe par
//! [`ui::console::start_encoder`](cloud_chamber_firmware::ui::console::start_encoder),
//! la même fonction qu'appellent `main.rs` et `ui_test` : mêmes broches,
//! même pull-up, même scrutation depuis `TIMER_IRQ_0`, même file
//! d'événements. Ce bin n'a pas d'écran câblé — c'est pour ça que `console`
//! rend ses deux moitiés séparément.
//!
//! Avant, il scrutait depuis sa propre boucle avec un `delay_ms(1)`. C'est
//! précisément le schéma que le firmware a dû abandonner : une scrutation
//! depuis la boucle perd les crans survenus pendant que la boucle fait
//! autre chose. Le bin ne validait donc pas le chemin réel.
//!
//! # Pull-up interne
//!
//! Standard pour un encodeur mécanique dont le commun est au GND (contact =
//! tire à la masse, repos = haut). Si le tien est câblé autrement (commun
//! au 3,3 V), les événements sortiront inversés ou bruités — directement
//! visible dans les logs ci-dessous.
//!
//! # Cadence de scrutation
//!
//! `RotaryEncoder::poll` ne regarde qu'un front montant sur A et lit B à
//! cet instant précis — pas de machine à états complète sur les quatre
//! transitions de quadrature. Une scrutation trop lente manque des
//! transitions ou les lit à moitié faites (A et B pas encore synchrones),
//! ce qui peut renvoyer le mauvais sens sans que rien ne soit cassé côté
//! câblage. La période est
//! [`console::ENCODER_POLL_MS`](cloud_chamber_firmware::ui::console::ENCODER_POLL_MS),
//! face à un cycle de quadrature complet de 10-20 ms à vitesse normale.
//!
//! # Débordement de la file
//!
//! Un `evenement(s) encodeur perdus` en warn veut dire que la boucle n'a
//! pas dépilé assez vite. Sur ce bin, qui ne fait rien d'autre que
//! journaliser, ça ne devrait jamais arriver : si ça se produit, c'est le
//! transport defmt qui bloque, pas l'encodeur.
//!
//! RP2040 uniquement, derrière la feature `bin-encoder-test` (désactivée
//! par défaut) pour ne pas être construit par les jobs CI `cargo check` sur
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

use rp2040_hal as hal;

use cloud_chamber_firmware::board;
use cloud_chamber_firmware::drivers::encoder::EncoderEvent;
use cloud_chamber_firmware::ui::console;

#[hal::entry]
fn main() -> ! {
    let mut board = board::init();

    console::start_encoder(&mut board.timer);
    defmt::info!("encoder_test demarre — tourne la molette");

    loop {
        while let Some(event) = console::next_event() {
            match event {
                EncoderEvent::RotateClockwise => defmt::info!("rotation horaire"),
                EncoderEvent::RotateCounterClockwise => defmt::info!("rotation anti-horaire"),
                EncoderEvent::ButtonPressed => defmt::info!("bouton presse"),
                // L'ISR ne l'empile jamais ; le bras existe pour que
                // l'ajout d'une variante à `EncoderEvent` casse ici.
                EncoderEvent::None => {}
            }
        }

        let dropped = console::take_dropped_events();
        if dropped > 0 {
            defmt::warn!("{} evenement(s) encodeur perdus : file pleine", dropped);
        }
    }
}
