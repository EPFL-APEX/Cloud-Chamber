//! Interface utilisateur style Prusa pour l'affichage ILI9341 320×240.
//!
//! # Organisation
//!
//! - [`theme`]     : palette de couleurs et styles graphiques
//! - [`navigator`] : pile de navigation entre écrans
//! - [`screens`]   : écrans complets (statut, menu principal)

pub mod navigator;
pub mod screens;
pub mod theme;
mod utils;
pub mod interactions;