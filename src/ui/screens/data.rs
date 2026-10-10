//! Écran Données : toutes les mesures brutes, index par index.
//!
//! Pendant « affichage seul » de [`crate::ui::screens::stats`], qui montre
//! la *synthèse* (une valeur par rôle), celui-ci montre *tout* : les huit
//! sondes 1-Wire avec leur label de câblage
//! ([`TEMP_LABELS`](crate::config::wiring::TEMP_LABELS)), la pression, et
//! l'état de fermeture — la vue de diagnostic, pour vérifier un capteur
//! précis au banc plutôt qu'une grandeur de contrôle.
//!
//! Une sonde muette affiche `---`, pas une valeur inventée — même règle
//! que la bande du bas du menu principal.

use core::fmt::Write as _;
use heapless::String;

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{ascii::{FONT_6X10, FONT_9X18_BOLD}, MonoTextStyle},
    pixelcolor::Rgb565,
    primitives::{Primitive, PrimitiveStyleBuilder, Rectangle},
    text::Text,
    Drawable,
};

use crate::{
    cloud_chamber_hal::config::{CHAMBER_PRESSURE_IDX, NUMBER_OF_TEMP_SENSOR},
    config::wiring::TEMP_LABELS,
    shared::data::SharedState,
    ui::theme,
};

/// Écran Données : les huit températures + pression + fermeture.
pub struct DataScreen<'a> {
    pub state: &'a SharedState,
}

/// Une ligne « label : valeur » — `---` si la sonde n'a rien dit.
fn temp_line(label: &str, value: Option<f32>, out: &mut String<32>) {
    match value {
        Some(v) if !v.is_nan() => {
            let _ = write!(out, "{label}: {v:+6.1}C");
        }
        _ => {
            let _ = write!(out, "{label}: ---");
        }
    }
}

impl<'a> DataScreen<'a> {
    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        let snap = &self.state.snapshot;

        let bg = PrimitiveStyleBuilder::new().fill_color(theme::BACKGROUND_COLOR).build();
        Rectangle::new(Point::zero(), Size::new(320, 240)).into_styled(bg).draw(display)?;

        Text::new(
            "DONNEES",
            Point::new(4, 14),
            MonoTextStyle::new(&FONT_9X18_BOLD, theme::HIGHLIGHT_COLOR),
        )
        .draw(display)?;

        // Huit sondes sur deux colonnes : 6 lignes de 20 px à partir de
        // y=34 tiennent dans 240 px avec la pression et la fermeture.
        const LINE_HEIGHT: i32 = 20;
        const COLUMN_WIDTH: i32 = 160;
        const TOP: i32 = 34;

        for i in 0..NUMBER_OF_TEMP_SENSOR {
            let mut line: String<32> = String::new();
            // Label vide pour un emplacement sans rôle câblé : montre
            // l'index plutôt que rien — c'est une vue de diagnostic, un
            // emplacement « ds5 » muet est une information en soi.
            let label = if TEMP_LABELS[i].is_empty() { "libre" } else { TEMP_LABELS[i] };
            temp_line(
                label,
                snap.temps[i].map(|m| m.value.0),
                &mut line,
            );
            let x = 4 + (i as i32 % 2) * COLUMN_WIDTH;
            let y = TOP + (i as i32 / 2) * LINE_HEIGHT;
            Text::new(line.as_str(), Point::new(x, y), MonoTextStyle::new(&FONT_6X10, theme::TEXT_COLOR))
                .draw(display)?;
        }

        // ─── Pression chambre ────────────────────────────────────────
        {
            let mut line: String<32> = String::new();
            match snap.press[CHAMBER_PRESSURE_IDX].map(|m| m.value.0) {
                Some(p) if !p.is_nan() => {
                    let _ = write!(line, "pression chambre: {p:7.1} hPa");
                }
                _ => {
                    let _ = write!(line, "pression chambre: ---");
                }
            }
            Text::new(
                line.as_str(),
                Point::new(4, TOP + 4 * LINE_HEIGHT),
                MonoTextStyle::new(&FONT_6X10, theme::TEXT_COLOR),
            )
            .draw(display)?;
        }

        // ─── Fermeture ────────────────────────────────────────────────
        {
            let mut line: String<32> = String::new();
            let _ = write!(line, "chambre fermee: {}", if snap.is_closed { "oui" } else { "non" });
            Text::new(
                line.as_str(),
                Point::new(4, TOP + 5 * LINE_HEIGHT),
                MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
            )
            .draw(display)?;
        }

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
    use embedded_graphics::pixelcolor::Rgb565;
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
