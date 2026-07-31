//! Interface utilisateur style Prusa pour l'affichage ILI9341 320×240.
//!
//! # Organisation
//!
//! - [`theme`]       : palette de couleurs et styles graphiques
//! - [`navigator`]   : pile de navigation générique (ne connaît aucun écran)
//! - [`screens`]     : écrans concrets (menu principal, stats...)
//! - [`router`]      : compose navigator + screens, point d'entrée public
//! - [`interactions`]: traits d'entrée (Rotary/Click) implémentés par écran

pub mod navigator;
pub mod screens;
pub mod router;
pub mod theme;
mod utils;
pub mod interactions;
