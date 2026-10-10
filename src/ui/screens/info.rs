//! Écran Info : version du firmware et identité de la machine.
//!
//! La vue « plaque signalétique » — ce qu'un opérateur branché sur une
//! machine inconnue doit voir en premier : quelle version tourne, pour
//! la rapprocher d'une release ou d'un signalement de bug. Affichage
//! seul, aucune mesure : les données vivent sur l'écran Données, la
//! synthèse sur Stats.

use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{MonoTextStyle, ascii::{FONT_6X13, FONT_9X18_BOLD}},
    pixelcolor::Rgb565,
    primitives::{Primitive, PrimitiveStyleBuilder, Rectangle},
    text::Text,
};

use crate::ui::theme;

/// Version du firmware, à maintenir en phase avec `Cargo.toml`.
pub const FIRMWARE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Écran Info : version du firmware, cible de construction.
pub struct InfoScreen;

impl InfoScreen {
    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        let bg = PrimitiveStyleBuilder::new().fill_color(theme::BACKGROUND_COLOR).build();
        Rectangle::new(Point::zero(), Size::new(320, 240)).into_styled(bg).draw(display)?;

        Text::new(
            "INFOS",
            Point::new(4, 14),
            MonoTextStyle::new(&FONT_9X18_BOLD, theme::HIGHLIGHT_COLOR),
        )
        .draw(display)?;

        let mut line: heapless::String<40> = heapless::String::new();
        let _ = core::fmt::Write::write_fmt(&mut line, format_args!("Firmware : v{}", FIRMWARE_VERSION));
        Text::new(
            line.as_str(),
            Point::new(20, 60),
            MonoTextStyle::new(&FONT_6X13, theme::TEXT_COLOR),
        )
        .draw(display)?;

        Text::new(
            "Cloud Chamber - EPFL APEX",
            Point::new(20, 84),
            MonoTextStyle::new(&FONT_6X13, theme::TEXT_COLOR),
        )
        .draw(display)?;

        Text::new(
            "RP2040 (Pico), Rust no_std",
            Point::new(20, 108),
            MonoTextStyle::new(&FONT_6X13, theme::DIM_COLOR),
        )
        .draw(display)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_graphics::pixelcolor::Rgb565;
    use embedded_graphics_simulator::SimulatorDisplay;

    #[test]
    fn draws_without_error() {
        let mut d = SimulatorDisplay::<Rgb565>::new(Size::new(320, 240));
        InfoScreen.draw(&mut d).unwrap();
    }
}
