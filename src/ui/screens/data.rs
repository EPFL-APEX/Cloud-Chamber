//! Écran Données : toutes les mesures brutes, index par index.
//!
//! Pendant « affichage seul » de [`crate::ui::screens::stats`], qui montre
//! la *synthèse* (une valeur par rôle), celui-ci montre *tout* : les huit
//! sondes 1-Wire avec leur label de câblage
//! ([`TEMP_LABELS`](crate::config::wiring::TEMP_LABELS)), la pression, et
//! l'état de fermeture — la vue de diagnostic, pour vérifier un capteur
//! précis au banc plutôt qu'une grandeur de contrôle.
//!
//! Même facture que les autres écrans : bandeau d'un mot, filet, table
//! libellé gris / valeur blanche alignée à droite, pied de deux cases.
//!
//! Une sonde muette affiche `---`, pas une valeur inventée — même règle
//! que la bande du bas du menu principal.

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

use crate::{
    cloud_chamber_hal::config::{CHAMBER_PRESSURE_IDX, NUMBER_OF_TEMP_SENSOR},
    config::wiring::TEMP_LABELS,
    shared::data::SharedState,
    ui::theme,
};

/// Marge gauche, celle de toute la grille.
const MARGIN: i32 = 12;
/// Filet sous le bandeau de titre.
const RULE_TITLE: i32 = 36;
/// Filet au-dessus du pied.
const RULE_FOOTER: i32 = 164;

/// Écran Données : les huit températures + pression + fermeture.
pub struct DataScreen<'a> {
    pub state: &'a SharedState,
}

/// Libellé de câblage en capitales — la vue de diagnostic montre
/// l'identité exacte de la sonde, un emplacement sans rôle câblé reste
/// « LIBRE » : c'est une information en soi.
fn push_upper(out: &mut String<24>, label: &str) {
    for c in label.chars() {
        let _ = out.push(c.to_ascii_uppercase());
    }
}

impl<'a> DataScreen<'a> {
    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        let snap = &self.state.snapshot;
        let top_style = TextStyleBuilder::new().baseline(Baseline::Top).build();

        display.clear(theme::BACKGROUND_COLOR)?;

        // ─── Bandeau : le mot de l'écran, un filet ────────────────────────
        Text::with_text_style(
            "DONNEES",
            Point::new(MARGIN, 8),
            MonoTextStyle::new(&FONT_9X18_BOLD, theme::HIGHLIGHT_COLOR),
            top_style,
        )
        .draw(display)?;
        Line::new(Point::new(0, RULE_TITLE), Point::new(320, RULE_TITLE))
            .into_styled(PrimitiveStyle::with_stroke(theme::DIM_COLOR, 1))
            .draw(display)?;

        // ─── Les huit sondes : deux colonnes, valeurs alignées à droite ───
        for i in 0..NUMBER_OF_TEMP_SENSOR {
            let col = (i % 2) as i32;
            let row = (i / 2) as i32;
            let x = MARGIN + col * 156;
            let y = 48 + row * 28;

            let mut label: String<24> = String::new();
            if TEMP_LABELS[i].is_empty() {
                let _ = label.push_str("LIBRE");
            } else {
                push_upper(&mut label, TEMP_LABELS[i]);
            }
            Text::with_text_style(
                label.as_str(),
                Point::new(x, y),
                MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
                top_style,
            )
            .draw(display)?;

            let mut v: String<12> = String::new();
            let (text, color) = match snap.temps[i] {
                Some(m) if !m.value.is_nan() => {
                    let _ = write!(v, "{:+.1}", m.value.0);
                    (v.as_str(), theme::TEXT_COLOR)
                }
                _ => ("---", theme::DIM_COLOR),
            };
            Text::with_text_style(
                text,
                Point::new(x + 148 - text.len() as i32 * 6, y + 11),
                MonoTextStyle::new(&FONT_6X13, color),
                top_style,
            )
            .draw(display)?;
        }

        // ─── Pied : pression et fermeture, deux cases ─────────────────────
        Line::new(Point::new(0, RULE_FOOTER), Point::new(320, RULE_FOOTER))
            .into_styled(PrimitiveStyle::with_stroke(theme::DIM_COLOR, 1))
            .draw(display)?;

        Text::with_text_style(
            "PRESSION",
            Point::new(MARGIN, 174),
            MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
            top_style,
        )
        .draw(display)?;
        // `HectoPascal` porte bien des hPa — même convention d'unité que
        // l'écran de statistiques.
        let mut p: String<16> = String::new();
        let (p_text, p_color) = match snap.press[CHAMBER_PRESSURE_IDX] {
            Some(m) if !m.value.is_nan() => {
                let _ = write!(p, "{:.0} hPa", m.value.0);
                (p.as_str(), theme::TEXT_COLOR)
            }
            _ => ("---", theme::DIM_COLOR),
        };
        Text::with_text_style(
            p_text,
            Point::new(MARGIN, 186),
            MonoTextStyle::new(&FONT_6X13, p_color),
            top_style,
        )
        .draw(display)?;

        Text::with_text_style(
            "CHAMBRE",
            Point::new(172, 174),
            MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
            top_style,
        )
        .draw(display)?;
        Text::with_text_style(
            if snap.is_closed { "FERMEE" } else { "OUVERTE" },
            Point::new(172, 186),
            MonoTextStyle::new(&FONT_6X13, theme::TEXT_COLOR),
            top_style,
        )
        .draw(display)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_chamber_hal::measurement::Measurement;
    use crate::cloud_chamber_hal::timer::Instant;
    use crate::cloud_chamber_hal::units::Celsius;
    use crate::shared::data::{SensorSnapshot, SystemTask};
    use embedded_graphics_simulator::SimulatorDisplay;

    fn make_display() -> SimulatorDisplay<Rgb565> {
        SimulatorDisplay::new(Size::new(320, 240))
    }

    #[test]
    fn draws_empty_without_error() {
        let mut d = make_display();
        let state = SharedState {
            snapshot: SensorSnapshot::default(),
            task: SystemTask::Idle,
            new_data: false,
            core1_panicked: false,
        };
        DataScreen { state: &state }.draw(&mut d).unwrap();
    }

    #[test]
    fn draws_full_without_error() {
        let mut d = make_display();
        let mut snapshot = SensorSnapshot::default();
        for i in 0..NUMBER_OF_TEMP_SENSOR {
            snapshot.temps[i] = Some(Measurement::new(Instant::from_micros(1), Celsius(20.0 + i as f32)));
        }
        let state = SharedState {
            snapshot,
            task: SystemTask::Stabilising,
            new_data: true,
            core1_panicked: false,
        };
        DataScreen { state: &state }.draw(&mut d).unwrap();
    }
}
