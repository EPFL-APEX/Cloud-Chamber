//! Liste "libellé + statut" réutilisable entre écrans (checklist, self-test,
//! séquence de démarrage...).
//!
//! `N_LINES` fixe le nombre de lignes à la compilation (pas d'allocation
//! heap). `SEPARATOR` active des séparateurs horizontaux entre les lignes
//! (et un dernier après la liste) — deux rendus différents selon l'écran.

use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point},
    mono_font::{ascii::FONT_6X13, MonoTextStyle},
    pixelcolor::Rgb565,
    primitives::{Circle, Line, Primitive, PrimitiveStyle},
    text::{Baseline, Text, TextStyle, TextStyleBuilder},
};

use crate::ui::theme;

/// Avancement d'une étape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pending,
    Done,
    Failed,
}

/// Une ligne de la liste : libellé + statut courant.
#[derive(Clone, Copy)]
pub struct StatusLine {
    pub label: &'static str,
    pub status: Status,
}

const WIDTH: i32 = 320;
const ICON_SIZE: i32 = 14;
const ICON_MARGIN_RIGHT: i32 = 10;

/// Liste de `N_LINES` lignes statut, avec séparateurs optionnels
/// (`SEPARATOR`). Taille fixée à la compilation — pas d'allocation heap.
pub struct StatusLines<const N_LINES: usize, const SEPARATOR: bool> {
    pub lines: [StatusLine; N_LINES],
}

impl<const N_LINES: usize, const SEPARATOR: bool> StatusLines<N_LINES, SEPARATOR> {
    const LINE_HEIGHT: i32 = if SEPARATOR { 22 } else { 20 };

    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        let char_style = MonoTextStyle::new(&FONT_6X13, theme::TEXT_COLOR);
        let text_style: TextStyle = TextStyleBuilder::new().baseline(Baseline::Top).build();
        let separator_style = PrimitiveStyle::with_stroke(theme::ACCENT_COLOR, 1);

        for (i, line) in self.lines.iter().enumerate() {
            let y = i as i32 * Self::LINE_HEIGHT;

            if SEPARATOR {
                Line::new(Point::new(0, y), Point::new(WIDTH, y))
                    .into_styled(separator_style)
                    .draw(display)?;
            }

            Text::with_text_style(line.label, Point::new(10, y + 4), char_style, text_style)
                .draw(display)?;

            let icon_top_left = Point::new(
                WIDTH - ICON_MARGIN_RIGHT - ICON_SIZE,
                y + (Self::LINE_HEIGHT - ICON_SIZE) / 2,
            );
            draw_status_icon(display, icon_top_left, line.status)?;
        }

        if SEPARATOR {
            let y = N_LINES as i32 * Self::LINE_HEIGHT;
            Line::new(Point::new(0, y), Point::new(WIDTH, y))
                .into_styled(separator_style)
                .draw(display)?;
        }

        Ok(())
    }
}

/// Dessine l'icône de statut. Fonction séparée (pas une branche de `match`
/// qui retournerait une valeur `Drawable` commune) car les trois statuts
/// dessinent des primitives de types concrets différents (`Circle` vs deux
/// `Line`) — chaque branche appelle `.draw()` elle-même.
fn draw_status_icon<D>(display: &mut D, top_left: Point, status: Status) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    match status {
        // "Spinner qui tourne" simplifié en icône statique : draw() n'a pas
        // accès à une frame/au temps pour animer quoi que ce soit.
        Status::Pending => Circle::new(top_left, ICON_SIZE as u32)
            .into_styled(PrimitiveStyle::with_stroke(theme::DIM_COLOR, 2))
            .draw(display),

        Status::Done => {
            let style = PrimitiveStyle::with_stroke(theme::SUCCESS_COLOR, 2);
            let p1 = top_left + Point::new(1, ICON_SIZE / 2);
            let p2 = top_left + Point::new(ICON_SIZE / 2 - 1, ICON_SIZE - 3);
            let p3 = top_left + Point::new(ICON_SIZE - 1, 1);
            Line::new(p1, p2).into_styled(style).draw(display)?;
            Line::new(p2, p3).into_styled(style).draw(display)
        }

        Status::Failed => {
            let style = PrimitiveStyle::with_stroke(theme::DANGER_COLOR, 2);
            let top_right = top_left + Point::new(ICON_SIZE - 1, 0);
            let bottom_left = top_left + Point::new(0, ICON_SIZE - 1);
            let bottom_right = top_left + Point::new(ICON_SIZE - 1, ICON_SIZE - 1);
            Line::new(top_left, bottom_right).into_styled(style).draw(display)?;
            Line::new(top_right, bottom_left).into_styled(style).draw(display)
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_graphics::geometry::Size;
    use embedded_graphics_simulator::SimulatorDisplay;

    fn make_display() -> SimulatorDisplay<Rgb565> {
        SimulatorDisplay::new(Size::new(320, 240))
    }

    #[test]
    fn draws_without_separator() {
        let mut d = make_display();
        let widget = StatusLines::<3, false> {
            lines: [
                StatusLine { label: "Etape 1", status: Status::Done },
                StatusLine { label: "Etape 2", status: Status::Pending },
                StatusLine { label: "Etape 3", status: Status::Failed },
            ],
        };
        widget.draw(&mut d).unwrap();
    }

    #[test]
    fn draws_with_separator() {
        let mut d = make_display();
        let widget = StatusLines::<2, true> {
            lines: [
                StatusLine { label: "A", status: Status::Pending },
                StatusLine { label: "B", status: Status::Pending },
            ],
        };
        widget.draw(&mut d).unwrap();
    }

    #[test]
    fn draws_empty_list() {
        let mut d = make_display();
        let widget = StatusLines::<0, false> { lines: [] };
        widget.draw(&mut d).unwrap();
    }

    #[test]
    fn status_lines_screenshot() -> Result<(), core::convert::Infallible> {
        use embedded_graphics_simulator::OutputSettingsBuilder;

        let mut display = make_display();
        let widget = StatusLines::<4, true> {
            lines: [
                StatusLine { label: "Verification capteurs", status: Status::Done },
                StatusLine { label: "Pre-refroidissement", status: Status::Done },
                StatusLine { label: "Saturation IPA", status: Status::Pending },
                StatusLine { label: "Haute tension", status: Status::Failed },
            ],
        };
        widget.draw(&mut display)?;

        let output_settings = OutputSettingsBuilder::new().build();
        let path = std::env::args_os()
            .nth(1)
            .unwrap_or_else(|| "screenshots/StatusLines.png".into());
        display
            .to_rgb_output_image(&output_settings)
            .save_png(&path)
            .expect("failed to save screenshot");

        Ok(())
    }
}
