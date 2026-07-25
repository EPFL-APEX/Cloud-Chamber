//! Primitives de dessin réutilisables — palette et helpers graphiques.
//!
//! Extrait de `screen_driver.rs` suite a la review PR #20 : « il faut split
//! ce monolithe dans le module UI pour l'affichage, il y a une partie qui
//! peut etre reutilisee et qui sera donc un module a part, et une partie qui
//! sera un des screens de l'UI ».
//!
//! Ce fichier est la partie réutilisable : rien ici ne connaît la chambre à
//! brouillard, tout est générique sur `DrawTarget<Color = Rgb565>`. Les
//! futurs écrans (stats, paramètres, actions) s'appuient dessus.
//!
//! À FUSIONNER avec `ui/theme.rs` de la branche équipe quand il sera
//! réactivé : la palette ci-dessous fait doublon avec la sienne. Je ne l'ai
//! pas fait ici pour éviter de créer deux thèmes concurrents.

use core::fmt::Write as _;
use heapless::String;

use embedded_graphics::{
    mono_font::{MonoFont, MonoTextStyleBuilder},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyleBuilder, Rectangle},
    text::{Baseline, Text},
};

use crate::data::SystemState;

// ── Palette ─────────────────────────────────────────────────────────────────
pub const BG:       Rgb565 = Rgb565::BLACK;
pub const WH:       Rgb565 = Rgb565::WHITE;
pub const CY:       Rgb565 = Rgb565::CYAN;
pub const GR:       Rgb565 = Rgb565::GREEN;
pub const RD:       Rgb565 = Rgb565::RED;
pub const YL:       Rgb565 = Rgb565::YELLOW;
pub const DIM:      Rgb565 = Rgb565::new(8, 16, 8);
pub const BTN_STOP: Rgb565 = Rgb565::new(18, 3, 3);  // fond bouton ARRÊT (rouge sombre)
pub const BTN_GO:   Rgb565 = Rgb565::new(2, 18, 2);  // fond bouton MARCHE (vert sombre)
pub const BTN_CYC:  Rgb565 = Rgb565::new(14, 28, 0); // fond bouton CYCLE (jaune sombre)
pub const BTN_RST:  Rgb565 = Rgb565::new(3,  8, 18);

// ── Helpers de dessin ───────────────────────────────────────────────────────

/// Rectangle plein.
pub fn fill<D: DrawTarget<Color = Rgb565>>(
    d: &mut D, x: u32, y: u32, w: u32, h: u32, col: Rgb565,
) {
    Rectangle::new(Point::new(x as i32, y as i32), Size::new(w, h))
        .into_styled(PrimitiveStyleBuilder::new().fill_color(col).build())
        .draw(d).ok();
}

/// Texte sur le fond par défaut.
pub fn txt<D: DrawTarget<Color = Rgb565>>(
    d: &mut D, s: &str, x: i32, y: i32, font: &MonoFont<'_>, fg: Rgb565,
) {
    txt_on(d, s, x, y, font, fg, BG);
}

/// Texte avec fond explicite — pour dessiner sur les boutons colorés
/// sans laisser de rectangle noir derrière les caractères.
pub fn txt_on<D: DrawTarget<Color = Rgb565>>(
    d: &mut D, s: &str, x: i32, y: i32, font: &MonoFont<'_>, fg: Rgb565, bg: Rgb565,
) {
    let style = MonoTextStyleBuilder::new()
        .font(font).text_color(fg).background_color(bg).build();
    Text::with_baseline(s, Point::new(x, y), style, Baseline::Top).draw(d).ok();
}

/// Formate une température de capteur DS18B20 avec sa couleur d'état.
/// Retourne `---` en gris si le capteur est absent ou la lecture invalide.
pub fn fmt_temp<const N: usize>(
    state: &SystemState, idx: usize, rom_count: usize,
) -> (String<N>, Rgb565) {
    let mut val: String<N> = String::new();
    if idx < rom_count && state.temperatures[idx].valid {
        let t = state.temperatures[idx].value;
        write!(val, "{:+6.1}C ", t).ok();
        (val, if t < -20.0 { CY } else if t > 80.0 { RD } else { WH })
    } else {
        write!(val, "  ---   ").ok();
        (val, DIM)
    }
}
