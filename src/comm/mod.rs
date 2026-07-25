//! Liaison série USB avec l'hôte : transport, commandes et télémétrie.
//!
//! Déplacé depuis `src/bin/main.rs` suite à la review PR #20 — le point
//! d'entrée ne doit porter que la logique globale.
//!
//! Ce fichier ne contient que des déclarations et des ré-exports :
//!
//! - [`usb`]      : transport (écriture bornée, maintien de l'énumération)
//! - [`protocol`] : commandes reçues et ligne `STATE` publiée

pub mod protocol;
pub mod usb;

pub use protocol::{handle_command, parse_f32, publish_state};
pub use usb::{keepalive, usb_write, Serial};
