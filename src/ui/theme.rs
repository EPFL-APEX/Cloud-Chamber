//! Palette de couleurs et styles graphiques inspirés du style Prusa.
//!
//! # Pourquoi importer `RgbColor` ?
//!
//! `Rgb565::BLACK` et `Rgb565::WHITE` sont définis par le trait `RgbColor`
//! de `embedded-graphics`. Ce trait doit être en scope pour utiliser ces
//! constantes de couleur, même si le type `Rgb565` est déjà importé.

use embedded_graphics::pixelcolor::{Rgb565, RgbColor};

// ─── Couleurs principales ─────────────────────────────────────────────────────

pub const BACKGROUND_COLOR:Rgb565 = Rgb565::new(0, 5, 4);
pub const BACKGROUND_COLOR_DARKER:Rgb565 = Rgb565::new(1, 4, 3);
pub const ACCENT_COLOR:Rgb565 = Rgb565::new(1, 9, 8);
pub const HIGHLIGHT_COLOR: Rgb565 = Rgb565::new(1, 29, 23);
