//! Écran Info : la plaque signalétique de la machine.
//!
//! Ce qu'un opérateur branché sur une machine inconnue doit voir en
//! premier : quelle version tourne, sur quoi, depuis combien de temps —
//! pour rapprocher la machine d'une release ou d'un signalement de bug.
//! Affichage seul, aucune mesure : la synthèse vit sur Stats, la courbe
//! sur Courbe.
//!
//! L'uptime vient de la boucle du cœur 0, seule à lire le timer : elle
//! publie via `Screens::set_uptime` à chaque tour.

use core::fmt::Write as _;
use heapless::String;

use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{MonoTextStyle, ascii::{FONT_6X10, FONT_6X13, FONT_9X18_BOLD}},
    pixelcolor::Rgb565,
    primitives::{Line, Primitive, PrimitiveStyle},
    text::{Baseline, Text, TextStyleBuilder},
};

use crate::cloud_chamber_hal::timer::Duration;
use crate::ui::theme;

/// Version du firmware, à maintenir en phase avec `Cargo.toml`.
pub const FIRMWARE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Écran Info : version, plateforme, identité, uptime.
pub struct InfoScreen {
    pub uptime: Duration,
}

impl InfoScreen {
    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        let top_style = TextStyleBuilder::new().baseline(Baseline::Top).build();

        display.clear(theme::BACKGROUND_COLOR)?;

        // ─── Bandeau : le mot de l'écran, un filet ────────────────────────
        Text::with_text_style(
            "INFOS",
            Point::new(12, 8),
            MonoTextStyle::new(&FONT_9X18_BOLD, theme::HIGHLIGHT_COLOR),
            top_style,
        )
        .draw(display)?;
        Line::new(Point::new(0, 36), Point::new(320, 36))
            .into_styled(PrimitiveStyle::with_stroke(theme::DIM_COLOR, 1))
            .draw(display)?;

        // ─── Fiche : libellé gris, valeur blanche, une rangée par ligne ───
        let mut uptime: String<24> = String::new();
        let us = self.uptime.as_micros();
        let total_s = us / 1_000_000;
        let (h, m, s) = (total_s / 3600, (total_s / 60) % 60, total_s % 60);
        if h > 0 {
            let _ = write!(uptime, "{}h {:02}m", h, m);
        } else if m > 0 {
            let _ = write!(uptime, "{}m {:02}s", m, s);
        } else {
            let _ = write!(uptime, "{}s", s);
        }

        for (row, (label, value)) in [
            ("FIRMWARE", FIRMWARE_VERSION),
            ("MACHINE", "Cloud Chamber - EPFL APEX"),
            ("PLATEFORME", "RP2040 (Pico) - Rust no_std"),
            ("UPTIME", uptime.as_str()),
        ]
        .iter()
        .enumerate()
        {
            let y = 52 + row as i32 * 30;
            Text::with_text_style(
                label,
                Point::new(12, y),
                MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
                top_style,
            )
            .draw(display)?;
            Text::with_text_style(
                value,
                Point::new(12, y + 12),
                MonoTextStyle::new(&FONT_6X13, theme::TEXT_COLOR),
                top_style,
            )
            .draw(display)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_graphics_simulator::SimulatorDisplay;

    fn make_display() -> SimulatorDisplay<Rgb565> {
        SimulatorDisplay::new(Size::new(320, 240))
    }

    #[test]
    fn draws_without_error() {
        let mut d = make_display();
        InfoScreen { uptime: Duration::from_secs(5025) }.draw(&mut d).unwrap();
    }
}
