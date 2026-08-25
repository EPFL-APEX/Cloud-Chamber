//! Écran « cycle en cours » : la séquence de refroidissement sous forme de
//! checklist, avec l'étape courante mise en avant.
//!
//! Affichage seul, dérivé de [`SharedState`] — comme [`super::stats`], il
//! n'a aucun état propre et est reconstruit à chaque rendu par
//! [`crate::ui::router`]. C'est `logic::control_loop` qui fait avancer la
//! machine ; cet écran ne fait que montrer où elle en est.
//!
//! # Ce que la checklist peut et ne peut pas dire
//!
//! [`SystemTask`] ne porte une position dans la séquence que pendant
//! `Cooling(phase)`. Les autres états n'en portent aucune :
//!
//! - `Stabilising` : la séquence est allée au bout — toutes les étapes sont
//!   marquées faites.
//! - `Idle` : rien n'a (encore) tourné — toutes en attente.
//! - `Stopping(_)` / `Tripped(_)` : impossible de savoir jusqu'où le cycle
//!   était allé, l'information n'existe plus dans `SystemTask`. La liste
//!   reste donc en attente et c'est **l'en-tête** qui porte l'état réel
//!   (« ARRET… », cause de sécurité). Inventer un avancement plausible
//!   serait pire que de ne rien afficher.

use core::fmt::Write as _;
use heapless::String;

use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{MonoTextStyle, ascii::{FONT_6X10, FONT_6X13, FONT_9X18_BOLD}},
    pixelcolor::Rgb565,
    primitives::{Primitive, PrimitiveStyle, Rectangle},
    text::{Baseline, Text, TextStyleBuilder},
};

use crate::cloud_chamber_hal::config::CHAMBER_TEMP_IDX;
use crate::logic::cooling::CoolingPhase;
use crate::shared::data::{SharedState, SystemTask};
use crate::ui::theme;

use super::stats::phase_label;
use super::widgets::{Status, StatusLine, StatusLines};

const SCREEN_WIDTH: u32 = 320;
const TOP_BAND_HEIGHT: u32 = 29;
const LIST_TOP: i32 = 34;

/// Les étapes de `Cooling`, dans l'ordre où la machine les traverse.
///
/// Cet ordre est ce qui donne un sens à « avant » et « après » dans la
/// checklist. Il doit rester celui de [`CoolingPhase`] et des transitions de
/// `logic::cooling` — d'où le test `phase_order_matches_the_cooling_sequence`
/// plus bas, qui échoue si une phase est ajoutée sans être placée ici.
const PHASES: [(CoolingPhase, &str); 6] = [
    (CoolingPhase::SensorCheck, "Verification capteurs"),
    (CoolingPhase::PreCoolingThePlate, "Pre-refroidissement"),
    (CoolingPhase::StartingIpaCirculation, "Circulation IPA"),
    (CoolingPhase::SaturatingAirWithIpa, "Saturation IPA"),
    (CoolingPhase::HighVoltage, "Haute tension"),
    (CoolingPhase::FinalCheckBeforeStabilising, "Verification finale"),
];

/// Rang d'une phase dans [`PHASES`].
fn phase_index(phase: CoolingPhase) -> usize {
    PHASES
        .iter()
        .position(|(p, _)| *p == phase)
        // Inatteignable tant que `PHASES` couvre tout `CoolingPhase` — le
        // test `phase_order_matches_the_cooling_sequence` le garantit. `0`
        // plutôt qu'un panic : un écran ne doit jamais faire tomber la
        // machine, surtout pendant un cycle.
        .unwrap_or(0)
}

/// Statut à afficher pour l'étape de rang `row`, vu l'état courant.
/// Cf. doc de module pour les états qui ne portent pas de position.
fn status_for(row: usize, task: SystemTask) -> Status {
    match task {
        SystemTask::Cooling(phase) => {
            let current = phase_index(phase);
            if row < current {
                Status::Done
            } else if row == current {
                Status::Active
            } else {
                Status::Pending
            }
        }
        SystemTask::Stabilising => Status::Done,
        SystemTask::Idle | SystemTask::Stopping(_) | SystemTask::Tripped(_) => Status::Pending,
    }
}

/// Couleur de l'en-tête selon la gravité de l'état courant.
fn header_color(task: SystemTask) -> Rgb565 {
    match task {
        SystemTask::Tripped(_) => theme::DANGER_COLOR,
        SystemTask::Idle => theme::DIM_COLOR,
        SystemTask::Stabilising => theme::SUCCESS_COLOR,
        SystemTask::Cooling(_) | SystemTask::Stopping(_) => theme::WARNING_COLOR,
    }
}

/// Écran « cycle en cours ».
pub struct RunningScreen<'a> {
    pub state: &'a SharedState,
}

impl<'a> RunningScreen<'a> {
    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        let task = self.state.task;

        display.clear(theme::BACKGROUND_COLOR)?;

        // ─── Bande de titre : phase courante, en toutes lettres ──────────────
        Rectangle::new(Point::new(0, 0), Size::new(SCREEN_WIDTH, TOP_BAND_HEIGHT))
            .into_styled(PrimitiveStyle::with_fill(theme::BACKGROUND_COLOR_DARKER))
            .draw(display)?;

        let top_style = TextStyleBuilder::new().baseline(Baseline::Top).build();
        Text::with_text_style(
            phase_label(task),
            Point::new(10, 8),
            MonoTextStyle::new(&FONT_6X13, header_color(task)),
            top_style,
        )
        .draw(display)?;

        // ─── Checklist des étapes ────────────────────────────────────────────
        let mut lines = [StatusLine { label: "", status: Status::Pending }; PHASES.len()];
        for (row, (_, label)) in PHASES.iter().enumerate() {
            lines[row] = StatusLine { label, status: status_for(row, task) };
        }
        StatusLines::<{ PHASES.len() }, true> { lines, top: LIST_TOP }.draw(display)?;

        // ─── Pied : température chambre, la mesure qui pilote la séquence ────
        {
            let mut s: String<24> = String::new();
            let (text, color) = match self.state.snapshot.temps[CHAMBER_TEMP_IDX] {
                Some(m) if !m.value.0.is_nan() => {
                    let _ = write!(s, "Chambre: {:+6.1} C", m.value.0);
                    (s.as_str(), theme::TEXT_COLOR)
                }
                _ => ("Chambre: ---", theme::DIM_COLOR),
            };
            Text::with_text_style(
                text,
                Point::new(10, 186),
                MonoTextStyle::new(&FONT_9X18_BOLD, color),
                top_style,
            )
            .draw(display)?;
        }

        Text::with_text_style(
            "Clic: retour au menu",
            Point::new(10, 216),
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
    use crate::cloud_chamber_hal::measurement::Measurement;
    use crate::cloud_chamber_hal::timer::Instant;
    use crate::cloud_chamber_hal::units::Celsius;
    use crate::logic::security::SafetyCause;
    use crate::logic::stopping::StoppingPhase;
    use embedded_graphics_simulator::SimulatorDisplay;

    fn make_display() -> SimulatorDisplay<Rgb565> {
        SimulatorDisplay::new(Size::new(320, 240))
    }

    fn state_with(task: SystemTask) -> SharedState {
        SharedState { snapshot: Default::default(), task, new_data: false }
    }

    /// Garde-fou : si une variante est ajoutée à `CoolingPhase` sans être
    /// placée dans `PHASES`, ce test échoue — sinon l'étape manquante
    /// serait silencieusement affichée au mauvais rang (cf. `phase_index`).
    #[test]
    fn phase_order_matches_the_cooling_sequence() {
        use CoolingPhase::*;
        let expected = [
            SensorCheck,
            PreCoolingThePlate,
            StartingIpaCirculation,
            SaturatingAirWithIpa,
            HighVoltage,
            FinalCheckBeforeStabilising,
        ];
        let listed: [CoolingPhase; PHASES.len()] = PHASES.map(|(p, _)| p);
        assert_eq!(listed, expected);

        // Et chaque phase se retrouve bien à son propre rang.
        for (i, phase) in expected.iter().enumerate() {
            assert_eq!(phase_index(*phase), i);
        }
    }

    #[test]
    fn the_current_phase_is_the_only_active_one() {
        let task = SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa);
        let statuses: [Status; PHASES.len()] =
            core::array::from_fn(|row| status_for(row, task));

        assert_eq!(
            statuses,
            [
                Status::Done,    // Verification capteurs
                Status::Done,    // Pre-refroidissement
                Status::Done,    // Circulation IPA
                Status::Active,  // Saturation IPA  <- en cours
                Status::Pending, // Haute tension
                Status::Pending, // Verification finale
            ]
        );
    }

    #[test]
    fn first_phase_marks_nothing_as_done() {
        let task = SystemTask::Cooling(CoolingPhase::SensorCheck);
        assert_eq!(status_for(0, task), Status::Active);
        for row in 1..PHASES.len() {
            assert_eq!(status_for(row, task), Status::Pending);
        }
    }

    #[test]
    fn stabilising_marks_every_phase_done() {
        for row in 0..PHASES.len() {
            assert_eq!(status_for(row, SystemTask::Stabilising), Status::Done);
        }
    }

    /// Cf. doc de module : ces états ne portent aucune position dans la
    /// séquence, la liste reste donc neutre — l'en-tête porte l'information.
    #[test]
    fn states_without_a_position_leave_the_list_pending() {
        for task in [
            SystemTask::Idle,
            SystemTask::Stopping(StoppingPhase::CutHighVoltage),
            SystemTask::Tripped(SafetyCause::CompressorOverheat),
        ] {
            for row in 0..PHASES.len() {
                assert_eq!(status_for(row, task), Status::Pending, "{task:?}");
            }
        }
    }

    #[test]
    fn draws_every_cooling_phase_without_error() {
        for (phase, _) in PHASES {
            let mut d = make_display();
            let state = state_with(SystemTask::Cooling(phase));
            RunningScreen { state: &state }.draw(&mut d).unwrap();
        }
    }

    #[test]
    fn draws_idle_stabilising_and_tripped_without_error() {
        for task in [
            SystemTask::Idle,
            SystemTask::Stabilising,
            SystemTask::Stopping(StoppingPhase::CutCompressor),
            SystemTask::Tripped(SafetyCause::CompressorSensorLost),
        ] {
            let mut d = make_display();
            let state = state_with(task);
            RunningScreen { state: &state }.draw(&mut d).unwrap();
        }
    }

    #[test]
    fn draws_with_a_chamber_reading_without_error() {
        let mut d = make_display();
        let mut state = state_with(SystemTask::Cooling(CoolingPhase::HighVoltage));
        state.snapshot.temps[CHAMBER_TEMP_IDX] =
            Some(Measurement::new(Instant::from_micros(1), Celsius(-32.4)));
        RunningScreen { state: &state }.draw(&mut d).unwrap();
    }

    #[test]
    fn running_screenshot() -> Result<(), core::convert::Infallible> {
        use embedded_graphics_simulator::OutputSettingsBuilder;

        let mut display = make_display();
        let mut state = state_with(SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa));
        state.snapshot.temps[CHAMBER_TEMP_IDX] =
            Some(Measurement::new(Instant::from_micros(1), Celsius(-28.6)));
        RunningScreen { state: &state }.draw(&mut display)?;

        let output_settings = OutputSettingsBuilder::new().build();
        let path = std::env::args_os()
            .nth(1)
            .unwrap_or_else(|| "screenshots/Running.png".into());
        display
            .to_rgb_output_image(&output_settings)
            .save_png(&path)
            .expect("failed to save screenshot");

        Ok(())
    }
}
