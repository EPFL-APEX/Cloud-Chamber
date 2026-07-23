//! Écran de statut principal : affiche toutes les mesures en temps réel.
//!
//! # Layout 320×240
//!
//! ```text
//! ┌────────────────────────────────┐ y=0
//! │ Barre de statut (20 px)        │
//! ├────────────────────────────────┤ y=20
//! │ Températures (5 jauges)        │
//! ├────────────────────────────────┤ y=110
//! │ Tensions (3 valeurs)           │
//! ├────────────────────────────────┤ y=155
//! │ Courant (1 valeur)             │
//! ├────────────────────────────────┤ y=175
//! │ Fermeture                      │
//! └────────────────────────────────┘ y=240
//! ```

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::Rgb565,
    primitives::{Primitive, PrimitiveStyleBuilder, Rectangle},
    text::Text,
    Drawable,
};

use crate::{
    shared::data::{SensorSnapshot, SystemState},
    ui::{theme, widgets::{format_temp, StatusBar, TemperatureGauge}},
};

/// Écran de statut complet.
pub struct StatusScreen<'a> {
    pub snapshot: &'a SensorSnapshot,
    pub system_state: SystemState,
}

impl<'a> StatusScreen<'a> {
    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        // Fond
        let bg_style = PrimitiveStyleBuilder::new().fill_color(theme::BG).build();
        Rectangle::new(Point::zero(), Size::new(320, 240))
            .into_styled(bg_style)
            .draw(display)?;

        // Barre de statut
        let bar_color = match self.system_state {
            SystemState::Normal    => theme::SUCCESS,
            SystemState::Warning   => theme::WARNING,
            SystemState::Alarm     => theme::DANGER,
            SystemState::Emergency => theme::DANGER,
        };
        let title = match self.system_state {
            SystemState::Normal    => "Cloud Chamber  OK",
            SystemState::Warning   => "Cloud Chamber  AVERTISSEMENT",
            SystemState::Alarm     => "Cloud Chamber  ALARME",
            SystemState::Emergency => "Cloud Chamber  URGENCE",
        };
        StatusBar { title, state_color: bar_color }.draw(display)?;

        let text_style = MonoTextStyle::new(&FONT_6X10, theme::FG);

        // Températures
        Text::new("Temperatures:", Point::new(4, 34), text_style).draw(display)?;
        for (i, &t) in self.snapshot.temps.iter().enumerate() {
            let y = 40 + i as i32 * 14;
            let mut buf = [0u8; 8];
            let label = format_temp(t, &mut buf);
            Text::new(label, Point::new(4, y + 10), text_style).draw(display)?;
            TemperatureGauge {
                origin: Point::new(60, y as i32),
                width: 240,
                value: t,
                max: 80.0,
            }.draw(display)?;
        }

        // Tensions
        Text::new("Tensions:", Point::new(4, 118), text_style).draw(display)?;
        for (i, &v) in self.snapshot.volts.iter().enumerate() {
            let mut buf = [0u8; 8];
            let s = format_temp(v, &mut buf);
            Text::new(s, Point::new(4 + i as i32 * 100, 130), text_style).draw(display)?;
        }

        // Courant
        Text::new("Courant:", Point::new(4, 148), text_style).draw(display)?;
        {
            let mut buf = [0u8; 8];
            let s = format_temp(self.snapshot.amps[0], &mut buf);
            Text::new(s, Point::new(60, 148), text_style).draw(display)?;
        }

        // Fermeture
        let closed_label = if self.snapshot.is_closed { "Fermee: OUI" } else { "Fermee: NON" };
        Text::new(closed_label, Point::new(4, 165), text_style).draw(display)?;

        Ok(())
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_graphics_simulator::SimulatorDisplay;

    fn make_display() -> SimulatorDisplay<Rgb565> {
        SimulatorDisplay::new(Size::new(320, 240))
    }

    #[test]
    fn status_normal_draws_without_error() {
        let mut d = make_display();
        let snap = SensorSnapshot::default();
        StatusScreen { snapshot: &snap, system_state: SystemState::Normal }.draw(&mut d).unwrap();
    }

    #[test]
    fn status_alarm_draws_without_error() {
        let mut d = make_display();
        let mut snap = SensorSnapshot::default();
        snap.temps[0] = 65.0;
        StatusScreen { snapshot: &snap, system_state: SystemState::Alarm }.draw(&mut d).unwrap();
    }

    #[test]
    fn status_with_closed_chamber_draws() {
        let mut d = make_display();
        let mut snap = SensorSnapshot::default();
        snap.is_closed = true;
        StatusScreen { snapshot: &snap, system_state: SystemState::Normal }.draw(&mut d).unwrap();
    }
}
