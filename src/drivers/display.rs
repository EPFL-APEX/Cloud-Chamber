//! Driver d'affichage : wrapper autour de n'importe quel `DrawTarget` Rgb565.
//!
//! # Pourquoi un wrapper ?
//!
//! `CloudChamberDisplay<D>` encapsule un driver bas niveau (ex: ILI9341 via SPI)
//! et ré-expose les traits `DrawTarget` et `OriginDimensions`. Cela permet
//! d'ajouter des méthodes de dessin spécifiques au projet (fond, logo…)
//! sans modifier le driver générique.
//!
//! # Supertrait `OriginDimensions`
//!
//! `DrawTarget` a `OriginDimensions` comme supertrait. Rust exige néanmoins
//! que la bound `+ OriginDimensions` soit **explicitement répétée** dans les
//! blocs `impl` qui en ont besoin, même si elle est logiquement impliquée.

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::Rgb565,
    Pixel,
};

/// Wrapper sur un display `DrawTarget<Color = Rgb565>`.
pub struct CloudChamberDisplay<D>
where
    D: DrawTarget<Color = Rgb565> + OriginDimensions,
{
    pub driver: D,
}

impl<D> CloudChamberDisplay<D>
where
    D: DrawTarget<Color = Rgb565> + OriginDimensions,
{
    pub fn new(driver: D) -> Self {
        Self { driver }
    }
}

impl<D> DrawTarget for CloudChamberDisplay<D>
where
    D: DrawTarget<Color = Rgb565> + OriginDimensions,
{
    type Color = Rgb565;
    type Error = D::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.driver.draw_iter(pixels)
    }
}

impl<D> OriginDimensions for CloudChamberDisplay<D>
where
    D: DrawTarget<Color = Rgb565> + OriginDimensions,
{
    fn size(&self) -> Size {
        self.driver.size()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_graphics::geometry::Point;
    use embedded_graphics_simulator::SimulatorDisplay;

    fn make_display() -> CloudChamberDisplay<SimulatorDisplay<Rgb565>> {
        CloudChamberDisplay::new(SimulatorDisplay::new(Size::new(320, 240)))
    }

    #[test]
    fn display_reports_correct_size() {
        let display = make_display();
        assert_eq!(display.size(), Size::new(320, 240));
    }

    #[test]
    fn draw_pixel_does_not_error() {
        let mut display = make_display();
        let pixel = Pixel(Point::new(10, 10), Rgb565::new(31, 0, 0));
        display.draw_iter([pixel]).unwrap();
    }
}
