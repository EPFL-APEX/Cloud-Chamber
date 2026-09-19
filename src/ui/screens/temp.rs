//! Écran de veille, la température de chambre sur la dernière demi-heure.
//!
//! Affichage seul, comme [`super::stats`]. Il garde son propre historique
//! parce que `SharedState` ne porte que la dernière mesure, et il le
//! remplit même quand il n'est pas affiché, sinon la veille s'ouvre vide.
//!
//! # Une seule courbe, celle qui pilote
//!
//! La plaque porte trois sondes sur la machine, entrée, milieu et sortie.
//! Le firmware n'en désigne qu'une, `CHAMBER_TEMP_IDX`, et c'est la seule
//! que `logic::cooling` regarde pour décider ses transitions. Les deux
//! autres n'ont pas d'index attribué, `config::wiring::TEMP_LABELS`
//! décrivant encore l'ancien jeu de sondes de circuit frigorifique.
//!
//! Tracer les trois demanderait donc d'inventer deux index, et de figer
//! dans un second fichier une table qu'on sait fausse. À reprendre une
//! fois que `identify_temp_sensors` aura tranché l'ordre réel du bus.

use core::fmt::Write as _;
use heapless::String;

use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{MonoTextStyle, ascii::{FONT_6X10, FONT_6X13}},
    pixelcolor::Rgb565,
    primitives::{Line, Primitive, PrimitiveStyle, Rectangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};

use crate::cloud_chamber_hal::config::CHAMBER_TEMP_IDX;
use crate::cloud_chamber_hal::{
    measurement::Measurement, ring_buffer::RingBuffer, timer::Instant, units::Celsius,
};
use crate::shared::data::SharedState;
use crate::ui::theme;

use super::stats::phase_label;

const TEMP_GRAPH_BUFFER_LENGTH: usize = 100;

const SCREEN_WIDTH: u32 = 320;
const TOP_BAND_HEIGHT: u32 = 29;
const PLOT: Rectangle = Rectangle::new(Point::new(10, 40), Size::new(300, 160));

/// Intervalle minimal entre deux points retenus. En durée plutôt qu'en
/// nombre d'appels, la cadence de sondage suivant le nombre de sondes.
///
/// 18 s sur 100 points font une demi-heure, assez pour contenir un
/// pré-refroidissement (45 min de timeout). À revoir si le buffer change.
const SAMPLE_INTERVAL_MS: u64 = 18_000;

/// Fenêtre couverte, affichée en pied de graphe. Déduite plutôt qu'écrite
/// en dur, sinon elle mentirait au premier changement de cadence.
const WINDOW_MIN: u64 = SAMPLE_INTERVAL_MS * TEMP_GRAPH_BUFFER_LENGTH as u64 / 60_000;

/// Amplitude verticale minimale. Sans elle, un palier étalerait le pas du
/// DS18B20 (0.0625 °C en 12 bits) sur toute la hauteur du cadre.
const MIN_SPAN_C: f32 = 2.0;

/// Écran de veille.
pub struct TempGraphScreen {
    temps_buffer: RingBuffer<Measurement<Celsius>, TEMP_GRAPH_BUFFER_LENGTH>,
}

impl TempGraphScreen {
    pub fn new() -> Self {
        // `RingBuffer::new()` demande `T: Default`, que `Measurement` n'a
        // pas. `filled` laisse le buffer logiquement vide, la graine n'est
        // jamais rendue par `get`.
        Self {
            temps_buffer: RingBuffer::filled(Measurement::new(
                Instant::from_micros(0),
                Celsius(0.0),
            )),
        }
    }

    /// Retient une mesure si la précédente est assez ancienne.
    ///
    /// Les NaN sont écartés ici, un point hors échelle écraserait le reste
    /// de la courbe.
    pub fn sample(&mut self, measurement: Measurement<Celsius>) {
        if measurement.value.0.is_nan() {
            return;
        }
        let elapsed = match self.temps_buffer.get(0) {
            Ok(last) => measurement
                .time
                .as_millis()
                .saturating_sub(last.time.as_millis()),
            Err(_) => SAMPLE_INTERVAL_MS,
        };
        if elapsed >= SAMPLE_INTERVAL_MS {
            self.temps_buffer.push(measurement);
        }
    }

    pub fn draw<D>(&self, display: &mut D, state: &SharedState) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        display.clear(theme::BACKGROUND_COLOR)?;

        let top_style = TextStyleBuilder::new().baseline(Baseline::Top).build();
        let right_style = TextStyleBuilder::new()
            .baseline(Baseline::Top)
            .alignment(Alignment::Right)
            .build();

        // ─── Bande de titre, phase courante et valeur du moment ──────────────
        Rectangle::new(Point::zero(), Size::new(SCREEN_WIDTH, TOP_BAND_HEIGHT))
            .into_styled(PrimitiveStyle::with_fill(theme::BACKGROUND_COLOR_DARKER))
            .draw(display)?;

        Text::with_text_style(
            phase_label(state.task),
            Point::new(10, 8),
            MonoTextStyle::new(&FONT_6X13, theme::HIGHLIGHT_COLOR),
            top_style,
        )
        .draw(display)?;

        match state.snapshot.temps[CHAMBER_TEMP_IDX] {
            Some(m) if !m.value.0.is_nan() => {
                // Nommee, sinon rien ne dit de quelle sonde vient la
                // courbe. Meme vocabulaire que `running.rs`.
                let mut s: String<20> = String::new();
                let _ = write!(s, "Chambre {:+.1} C", m.value.0);
                Text::with_text_style(
                    s.as_str(),
                    Point::new(SCREEN_WIDTH as i32 - 10, 8),
                    MonoTextStyle::new(&FONT_6X13, theme::TEXT_COLOR),
                    right_style,
                )
                .draw(display)?;
            }
            // Rien plutôt que `---`, le pied porte déjà les bornes.
            _ => {}
        }

        // Un seul parcours pour le nombre de points et les bornes, `get`
        // refusant au premier index jamais écrit.
        let (mut count, mut min, mut max) = (0usize, f32::MAX, f32::MIN);
        for i in 0..TEMP_GRAPH_BUFFER_LENGTH {
            let Ok(m) = self.temps_buffer.get(i) else { break };
            count += 1;
            min = min.min(m.value.0);
            max = max.max(m.value.0);
        }

        // Rien à relier, et `count - 1` diviserait par zéro dans `x_of`.
        if count < 2 {
            Text::with_text_style(
                "Acquisition en cours",
                Point::new(10, 110),
                MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
                top_style,
            )
            .draw(display)?;
            return Ok(());
        }

        let span = (max - min).max(MIN_SPAN_C);
        // Haut du cadre, pas le maximum mesuré. Quand le plancher de `span`
        // s'applique, un palier se collerait sinon en haut, `max - value`
        // valant zéro partout.
        let top = (min + max + span) / 2.0;

        // `get(0)` est la plus récente, le tracé va donc de droite à gauche.
        let x_of = |i: usize| {
            PLOT.top_left.x + PLOT.size.width as i32
                - (i as i32 * PLOT.size.width as i32) / (count as i32 - 1)
        };
        let y_of = |value: f32| {
            PLOT.top_left.y + ((top - value) / span * PLOT.size.height as f32) as i32
        };

        // ─── Courbe ─────────────────────────────────────────────────────────
        let line_style = PrimitiveStyle::with_stroke(theme::HIGHLIGHT_COLOR, 2);
        let mut previous = None;
        for i in 0..count {
            let Ok(m) = self.temps_buffer.get(i) else { break };
            let point = Point::new(x_of(i), y_of(m.value.0));
            if let Some(from) = previous {
                Line::new(from, point).into_styled(line_style).draw(display)?;
            }
            previous = Some(point);
        }

        // ─── Pied, bornes de l'échelle ──────────────────────────────────────
        let mut s: String<40> = String::new();
        let _ = write!(s, "{:+.1} a {:+.1} C sur {} min", min, max, WINDOW_MIN);
        Text::with_text_style(
            s.as_str(),
            Point::new(10, 208),
            MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
            top_style,
        )
        .draw(display)?;

        Ok(())
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::data::{SensorSnapshot, SystemTask};
    use embedded_graphics_simulator::SimulatorDisplay;

    fn make_display() -> SimulatorDisplay<Rgb565> {
        SimulatorDisplay::new(Size::new(320, 240))
    }

    fn state() -> SharedState {
        SharedState {
            snapshot: SensorSnapshot::default(),
            task: SystemTask::Idle,
            new_data: false,
        }
    }

    fn at(second: u64, value: f32) -> Measurement<Celsius> {
        Measurement::new(Instant::from_micros(second * 1_000_000), Celsius(value))
    }

    /// Le cas qui motive la décimation, une mesure par seconde ne doit pas
    /// remplir le buffer en cent secondes.
    #[test]
    fn samples_closer_than_the_interval_are_dropped() {
        let mut screen = TempGraphScreen::new();
        for second in 0..100 {
            screen.sample(at(second, -10.0));
        }
        let kept = (0..TEMP_GRAPH_BUFFER_LENGTH)
            .take_while(|&i| screen.temps_buffer.get(i).is_ok())
            .count();
        // 99 s de mesures, le premier point à t = 0 puis un toutes les 18 s.
        assert_eq!(kept, 6);
    }

    #[test]
    fn samples_far_enough_apart_are_kept() {
        let mut screen = TempGraphScreen::new();
        for step in 0..4 {
            screen.sample(at(step * SAMPLE_INTERVAL_MS / 1_000, -10.0));
        }
        assert!(screen.temps_buffer.get(3).is_ok());
    }

    /// Une sonde muette ne doit pas entrer dans le buffer, son NaN
    /// emporterait `min` et `max` avec lui.
    #[test]
    fn a_silent_probe_is_not_recorded() {
        let mut screen = TempGraphScreen::new();
        screen.sample(at(0, f32::NAN));
        assert!(screen.temps_buffer.get(0).is_err());
    }

    #[test]
    fn draws_before_any_sample() {
        let mut d = make_display();
        TempGraphScreen::new().draw(&mut d, &state()).unwrap();
    }

    #[test]
    fn draws_a_full_buffer() {
        let mut d = make_display();
        let mut screen = TempGraphScreen::new();
        for step in 0..TEMP_GRAPH_BUFFER_LENGTH as u64 {
            screen.sample(at(step * SAMPLE_INTERVAL_MS / 1_000, 20.0 - step as f32));
        }
        screen.draw(&mut d, &state()).unwrap();
    }

    /// Un palier ne doit pas remplir le cadre de bruit de quantification.
    #[test]
    fn a_flat_run_keeps_the_minimum_span() {
        let mut d = make_display();
        let mut screen = TempGraphScreen::new();
        for step in 0..10u64 {
            screen.sample(at(step * SAMPLE_INTERVAL_MS / 1_000, -40.0));
        }
        screen.draw(&mut d, &state()).unwrap();
    }

    #[test]
    fn temp_graph_screenshot() -> Result<(), core::convert::Infallible> {
        use crate::logic::cooling::CoolingPhase;
        use embedded_graphics_simulator::OutputSettingsBuilder;

        let mut display = make_display();
        let mut screen = TempGraphScreen::new();

        // Une descente de 20 a -40 C, la forme qu'on veut reconnaitre de
        // loin pendant un pre-refroidissement.
        let mut last = at(0, 20.0);
        for step in 0..TEMP_GRAPH_BUFFER_LENGTH as u64 {
            let value = 20.0 - 60.0 * step as f32 / TEMP_GRAPH_BUFFER_LENGTH as f32;
            last = at(step * SAMPLE_INTERVAL_MS / 1_000, value);
            screen.sample(last);
        }

        // Machine en cours de cycle et sonde renseignee : la capture montre
        // alors la bande de titre complete, valeur du moment comprise.
        let mut snapshot = SensorSnapshot::default();
        snapshot.temps[CHAMBER_TEMP_IDX] = Some(last);
        let state = SharedState {
            snapshot,
            task: SystemTask::Cooling(CoolingPhase::PreCoolingThePlate),
            new_data: false,
        };
        screen.draw(&mut display, &state)?;

        let path = std::env::args_os()
            .nth(1)
            .unwrap_or_else(|| "screenshots/TempGraph.png".into());
        display
            .to_rgb_output_image(&OutputSettingsBuilder::new().build())
            .save_png(&path)
            .expect("failed to save screenshot");

        Ok(())
    }
}
