//! Composants graphiques réutilisables.
//!
//! # Pourquoi importer `Primitive` ?
//!
//! `Rectangle::into_styled()` est défini par le trait `Primitive` de
//! `embedded-graphics`. Sans cet import, l'appel `.into_styled()` échoue
//! avec "method not found", même si `Rectangle` est correctement importé.
//!
//! # Formatage sans allocation
//!
//! `no_std` interdit `format!()` (qui alloue). La fonction `format_temp()`
//! écrit directement dans un buffer `[u8; 8]` — suffisant pour "-99.9\0".

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::Rgb565,
    primitives::{Primitive, PrimitiveStyleBuilder, Rectangle},
    text::Text,
    Drawable,
};

use crate::ui::theme;

// ─── Barre de statut ──────────────────────────────────────────────────────────

/// Barre d'état en haut de l'écran (hauteur 20 px).
pub struct StatusBar<'a> {
    pub title: &'a str,
    pub state_color: Rgb565,
}

impl<'a> StatusBar<'a> {
    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        let style = PrimitiveStyleBuilder::new()
            .fill_color(self.state_color)
            .build();
        Rectangle::new(Point::zero(), Size::new(320, 20))
            .into_styled(style)
            .draw(display)?;

        let text_style = MonoTextStyle::new(&FONT_6X10, theme::FG);
        Text::new(self.title, Point::new(4, 14), text_style).draw(display)?;
        Ok(())
    }
}

// ─── Jauge de température ─────────────────────────────────────────────────────

/// Barre de progression représentant une température (0–100 °C).
pub struct TemperatureGauge {
    pub origin: Point,
    pub width: u32,
    pub value: f32,
    pub max: f32,
}

impl TemperatureGauge {
    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        let height = 10u32;
        let bg_style = PrimitiveStyleBuilder::new().fill_color(theme::BORDER).build();
        Rectangle::new(self.origin, Size::new(self.width, height))
            .into_styled(bg_style)
            .draw(display)?;

        let ratio = (self.value / self.max).clamp(0.0, 1.0);
        let filled = (ratio * self.width as f32) as u32;
        let bar_color = if ratio > 0.8 { theme::DANGER } else if ratio > 0.6 { theme::WARNING } else { theme::SUCCESS };
        let bar_style = PrimitiveStyleBuilder::new().fill_color(bar_color).build();
        Rectangle::new(self.origin, Size::new(filled.max(1), height))
            .into_styled(bar_style)
            .draw(display)?;

        Ok(())
    }
}

// ─── Élément de menu ──────────────────────────────────────────────────────────

/// Un élément de menu avec label et indicateur de sélection.
pub struct MenuItem<'a> {
    pub label: &'a str,
    pub origin: Point,
    pub selected: bool,
}

impl<'a> MenuItem<'a> {
    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        let bg = if self.selected { theme::SELECTED } else { theme::BG };
        let style = PrimitiveStyleBuilder::new().fill_color(bg).build();
        Rectangle::new(self.origin, Size::new(320, 16))
            .into_styled(style)
            .draw(display)?;

        let text_color = if self.selected { theme::ACCENT } else { theme::FG };
        let text_style = MonoTextStyle::new(&FONT_6X10, text_color);
        Text::new(self.label, Point::new(self.origin.x + 8, self.origin.y + 12), text_style)
            .draw(display)?;
        Ok(())
    }
}

// ─── Barre de progression générique ──────────────────────────────────────────

pub struct ProgressBar {
    pub origin: Point,
    pub width: u32,
    pub height: u32,
    pub ratio: f32, // 0.0 – 1.0
    pub color: Rgb565,
}

impl ProgressBar {
    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        let bg_style = PrimitiveStyleBuilder::new().fill_color(theme::BORDER).build();
        Rectangle::new(self.origin, Size::new(self.width, self.height))
            .into_styled(bg_style)
            .draw(display)?;
        let filled = ((self.ratio.clamp(0.0, 1.0)) * self.width as f32) as u32;
        let bar_style = PrimitiveStyleBuilder::new().fill_color(self.color).build();
        Rectangle::new(self.origin, Size::new(filled.max(1), self.height))
            .into_styled(bar_style)
            .draw(display)?;
        Ok(())
    }
}

// ─── Formatage no-alloc ───────────────────────────────────────────────────────

/// Formate un flottant en chaîne dans `buf`. Retourne le nombre d'octets écrits.
///
/// Produit le format "±INT.DEC" (ex: "25.3", "-4.1").
/// `buf` doit avoir au moins 8 octets.
pub fn format_temp(value: f32, buf: &mut [u8; 8]) -> &str {
    let neg = value < 0.0;
    let abs = if neg { -value } else { value };
    let mut int_part = abs as u32;
    // +0.5 for rounding; clamp overflow (e.g. 9.95 → dec=10 → carry)
    let mut dec_part = ((abs - int_part as f32) * 10.0 + 0.5) as u32;
    if dec_part >= 10 { dec_part = 0; int_part += 1; }

    let mut pos = 0usize;
    if neg { buf[pos] = b'-'; pos += 1; }

    // entier (max 3 chiffres)
    if int_part >= 100 { buf[pos] = b'0' + ((int_part / 100) % 10) as u8; pos += 1; }
    if int_part >= 10  { buf[pos] = b'0' + ((int_part / 10)  % 10) as u8; pos += 1; }
    buf[pos] = b'0' + (int_part % 10) as u8; pos += 1;

    buf[pos] = b'.'; pos += 1;
    buf[pos] = b'0' + (dec_part % 10) as u8; pos += 1;

    core::str::from_utf8(&buf[..pos]).unwrap_or("?")
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_graphics::geometry::Size;
    use embedded_graphics_simulator::SimulatorDisplay;

    fn display() -> SimulatorDisplay<Rgb565> {
        SimulatorDisplay::new(Size::new(320, 240))
    }

    #[test]
    fn status_bar_draws_without_error() {
        let mut d = display();
        StatusBar { title: "TEST", state_color: theme::ACCENT }.draw(&mut d).unwrap();
    }

    #[test]
    fn temperature_gauge_draws_without_error() {
        let mut d = display();
        TemperatureGauge { origin: Point::new(0, 30), width: 200, value: 50.0, max: 100.0 }
            .draw(&mut d).unwrap();
    }

    #[test]
    fn menu_item_selected_draws_without_error() {
        let mut d = display();
        MenuItem { label: "Option", origin: Point::new(0, 50), selected: true }.draw(&mut d).unwrap();
    }

    #[test]
    fn format_temp_positive() {
        let mut buf = [0u8; 8];
        let s = format_temp(25.3, &mut buf);
        assert_eq!(s, "25.3");
    }

    #[test]
    fn format_temp_negative() {
        let mut buf = [0u8; 8];
        let s = format_temp(-4.1, &mut buf);
        assert_eq!(s, "-4.1");
    }

    #[test]
    fn format_temp_zero() {
        let mut buf = [0u8; 8];
        let s = format_temp(0.0, &mut buf);
        assert_eq!(s, "0.0");
    }
}
