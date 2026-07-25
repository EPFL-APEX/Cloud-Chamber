//! Interface utilisateur — écran TFT ILI9341 320×240 et interaction tactile.
//!
//! Nom retenu suite à la review PR #20 : le module ne pilote pas seulement
//! l'écran, il porte toute l'interaction avec l'utilisateur.
//!
//! # Organisation
//!
//! - [`screen_driver`] : couche basse — layout, rendu et zones tactiles du
//!   TFT KMRTM28028-SPI. C'est le code actuellement validé sur matériel
//!   (ex-`src/display.rs`).
//!
//! Par-dessus viendront les modules de la branche équipe, aujourd'hui
//! conservés mais pas encore compilés (ils dépendent des traits
//! `cloud_chamber_hal` — cf. lib.rs, plan de convergence) :
//!
//! - `theme`        : palette de couleurs et styles graphiques
//! - `navigator`    : pile de navigation entre écrans
//! - `screens`      : écrans complets (statut, menu principal)
//! - `interactions` : gestion des entrées utilisateur
//!
//! Le `mod.rs` d'origine de la branche équipe est conservé sous
//! `mod_equipe.rs.disabled` : il sera restauré quand ces modules
//! compileront, et `screen_driver` passera alors dessous comme couche de
//! rendu.

pub mod screen_driver;
pub mod touch;
pub mod widgets;

pub mod navigator;
pub mod screens;
pub mod theme;
mod utils;
pub mod interactions;
