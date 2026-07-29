//! Liaison série USB avec l'hôte : transport et commandes.
//!
//! Module optionnel (feature `usb-comm`, désactivée par défaut) : pas
//! toujours branché sur la machine en usage réel, plutôt pour le debug ou
//! des cas d'usage spécifiques (scripts d'acquisition, terminal série).
//!
//! Portée actuelle : transport + commande `CYCLE` (démarrage/arrêt du
//! cycle automatique, réarmement du disjoncteur) uniquement. `TARGET`/`HV`/
//! `COMP` dépendent d'une consigne opérateur et d'un mode manuel qui
//! n'existent pas encore sur cette branche — retournent une erreur
//! explicite plutôt que d'être simulés. La ligne de télémétrie `STATE`
//! (port de `publish_state` côté équipe) n'est pas non plus portée : elle
//! dépend de champs qu'on n'a pas ici (BME280, cible opérateur, duty cycle
//! iso, uptime) — à construire une fois ces manques comblés.
//!
//! - [`usb`]      : transport (écriture bornée, maintien de l'énumération)
//! - [`protocol`] : commandes reçues de l'hôte

pub mod protocol;
pub mod usb;

pub use protocol::{handle_command, parse_f32};
pub use usb::{Serial, keepalive, usb_write};
