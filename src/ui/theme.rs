//! Palette de couleurs et styles graphiques inspirés du style Prusa.
//!
//! # Pourquoi importer `RgbColor` ?
//!
//! `Rgb565::BLACK` et `Rgb565::WHITE` sont définis par le trait `RgbColor`
//! de `embedded-graphics`. Ce trait doit être en scope pour utiliser ces
//! constantes de couleur, même si le type `Rgb565` est déjà importé.

use embedded_graphics::pixelcolor::{Rgb565, RgbColor};

// ─── Couleurs principales ─────────────────────────────────────────────────────

pub const BG: Rgb565       = Rgb565::BLACK;
pub const FG: Rgb565       = Rgb565::WHITE;
pub const ACCENT: Rgb565   = Rgb565::new(31, 20, 0);  // orange Prusa
pub const SUCCESS: Rgb565  = Rgb565::new(0, 40, 0);   // vert
pub const WARNING: Rgb565  = Rgb565::new(31, 40, 0);  // jaune-orange
pub const DANGER: Rgb565   = Rgb565::new(31, 0, 0);   // rouge
pub const SELECTED: Rgb565 = Rgb565::new(5, 10, 20);  // fond sélection (bleu sombre)
pub const BORDER: Rgb565   = Rgb565::new(10, 20, 10); // gris

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bg_is_black() {
        assert_eq!(BG, Rgb565::BLACK);
    }

    #[test]
    fn fg_is_white() {
        assert_eq!(FG, Rgb565::WHITE);
    }
}
