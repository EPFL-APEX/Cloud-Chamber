//! Écran de statistiques en direct.
//!
//! Port de l'affichage principal de `screen_driver.rs` (branche équipe,
//! `merge-Kynan-Thomas`) vers la structure modulaire de cette branche
//! (navigator/screens/theme). Seul l'affichage est repris — les boutons
//! tactiles de l'original ne le sont pas : cette branche navigue par
//! encodeur rotatif (`ui::interactions::{Rotary, Click}`, déjà utilisé par
//! `screens::menu`), pas par tactile.
//!
//! # Non porté
//!
//! `SensorSnapshot` ne modélise pas le BME280 (pas de case ambiante,
//! humidité, pression atmosphérique) — la ligne BME280 et l'indicateur de
//! sursaturation IPA de l'original (qui a besoin de la température
//! ambiante) ne sont donc pas représentables tels quels. Omis plutôt que
//! d'inventer une donnée. À revoir si `cloud_chamber_hal`/`SensorSnapshot`
//! gagne un jour une catégorie de mesure ambiante dédiée.

use core::fmt::Write as _;
use heapless::String;

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{ascii::{FONT_6X10, FONT_6X13, FONT_9X18_BOLD, FONT_10X20}, MonoTextStyle},
    pixelcolor::Rgb565,
    primitives::{Primitive, PrimitiveStyleBuilder, Rectangle},
    text::Text,
    Drawable,
};

use crate::{
    cloud_chamber_hal::config::{CHAMBER_PRESSURE_IDX, CHAMBER_TEMP_IDX},
    config::operating::{SATURATION_TARGET_C, TARGET_CHAMBER_TEMP},
    config::wiring::TEMP_LABELS,
    logic::{cooling::CoolingPhase, security::SafetyCause, stopping::StoppingPhase},
    shared::data::{SharedState, SystemTask},
    ui::theme,
};

/// Libellé court de l'état courant. Partagé avec `screens::running`
/// (même vocabulaire opérateur des deux côtés) — une seule table, pour
/// qu'un renommage de phase ne laisse pas un écran en arrière.
pub(super) fn phase_label(task: SystemTask) -> &'static str {
    use CoolingPhase::*;
    use StoppingPhase::*;
    match task {
        SystemTask::Idle => "IDLE",
        SystemTask::Cooling(SensorCheck) => "CHECK CAPTEURS",
        SystemTask::Cooling(PreCoolingThePlate) => "PRE-REFROIDIS.",
        SystemTask::Cooling(StartingIpaCirculation) => "CIRCUL. IPA",
        SystemTask::Cooling(SaturatingAirWithIpa) => "SATURATION IPA",
        SystemTask::Cooling(HighVoltage) => "HAUTE TENSION",
        SystemTask::Cooling(FinalCheckBeforeStabilising) => "VERIF. FINALE",
        SystemTask::Stabilising => "STABILISE",
        SystemTask::Stopping(CutHighVoltage) => "ARRET: HV OFF",
        SystemTask::Stopping(CutCompressor) => "ARRET: COMP.",
        SystemTask::Stopping(WaitPressureEquilibrium) => "ARRET: EQUIL.",
        SystemTask::Tripped(_) => "ARRET SECURITE",
    }
}

fn alert_message(cause: SafetyCause) -> &'static str {
    match cause {
        SafetyCause::CompressorOverheat => "SURCHAUFFE COMPRESSEUR",
        SafetyCause::CompressorSensorLost => "SONDE COMPRESSEUR PERDUE",
    }
}

/// Sorties actionneurs attendues pour une phase donnée — approximation pour
/// l'affichage uniquement (les valeurs réelles vivent dans les fonctions de
/// `logic::cooling`/`logic::stopping`, volontairement pas exposées comme
/// table séparée pour éviter qu'elle diverge de la vraie logique de
/// contrôle ; ici on ne fait qu'informer l'opérateur, pas piloter).
fn expected_outputs(task: SystemTask) -> (bool, bool) {
    use CoolingPhase::*;
    use StoppingPhase::*;
    match task {
        SystemTask::Cooling(SensorCheck) => (false, false),
        SystemTask::Cooling(PreCoolingThePlate) => (true, false),
        SystemTask::Cooling(StartingIpaCirculation) => (true, false),
        SystemTask::Cooling(SaturatingAirWithIpa) => (true, false),
        SystemTask::Cooling(HighVoltage) => (true, true),
        SystemTask::Cooling(FinalCheckBeforeStabilising) => (true, true),
        SystemTask::Stabilising => (true, true),
        SystemTask::Stopping(CutHighVoltage) => (true, false),
        SystemTask::Stopping(CutCompressor) => (false, false),
        SystemTask::Stopping(WaitPressureEquilibrium) => (false, false),
        SystemTask::Idle | SystemTask::Tripped(_) => (false, false),
    }
}

/// Écran de statistiques en direct : mesures, phase courante, sorties
/// actionneurs attendues pour cette phase.
pub struct StatsScreen<'a> {
    pub state: &'a SharedState,
}

impl<'a> StatsScreen<'a> {
    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        let snap = &self.state.snapshot;
        let task = self.state.task;

        let bg = PrimitiveStyleBuilder::new().fill_color(theme::BACKGROUND_COLOR).build();
        Rectangle::new(Point::zero(), Size::new(320, 240)).into_styled(bg).draw(display)?;

        // ─── En-tête : SAFE ou cause de déclenchement ────────────────────────
        let (header, header_color) = match task {
            SystemTask::Tripped(cause) => (alert_message(cause), theme::DANGER_COLOR),
            _ => ("SAFE", theme::SUCCESS_COLOR),
        };
        Text::new(header, Point::new(4, 14), MonoTextStyle::new(&FONT_9X18_BOLD, header_color))
            .draw(display)?;

        // ─── Phase courante ───────────────────────────────────────────────────
        let phase_color = match task {
            SystemTask::Tripped(_) => theme::DANGER_COLOR,
            SystemTask::Idle => theme::DIM_COLOR,
            _ => theme::WARNING_COLOR,
        };
        Text::new(phase_label(task), Point::new(4, 34), MonoTextStyle::new(&FONT_9X18_BOLD, phase_color))
            .draw(display)?;

        // ─── Base chambre (ds4) — grande valeur ──────────────────────────────
        {
            let mut val: String<12> = String::new();
            let (text, color) = match snap.temps[CHAMBER_TEMP_IDX] {
                Some(m) if !m.value.0.is_nan() => {
                    write!(val, "{:+6.1}C", m.value.0).ok();
                    let color = if m.value.0 <= SATURATION_TARGET_C { theme::ACCENT_COLOR }
                        else { theme::TEXT_COLOR };
                    (val.as_str(), color)
                }
                _ => ("  ---  ", theme::DIM_COLOR),
            };
            Text::new(text, Point::new(4, 66), MonoTextStyle::new(&FONT_10X20, color)).draw(display)?;
        }

        // ─── Cible ────────────────────────────────────────────────────────────
        // Pas encore de consigne opérateur dynamique sur cette branche —
        // affiche la constante de configuration en attendant.
        {
            let mut s: String<24> = String::new();
            write!(s, "Cible: {:+5.1}C", TARGET_CHAMBER_TEMP).ok();
            Text::new(s.as_str(), Point::new(4, 84), MonoTextStyle::new(&FONT_6X10, theme::TEXT_COLOR))
                .draw(display)?;
        }

        // ─── Compresseur + HT (attendus pour la phase courante) ──────────────
        {
            let (compressor, hv) = expected_outputs(task);
            let (ct, cc) = if compressor { ("COMP: ON ", theme::SUCCESS_COLOR) }
                else { ("COMP: OFF", theme::DIM_COLOR) };
            let (ht, hc) = if hv { ("HV: ON ", theme::WARNING_COLOR) }
                else { ("HV: OFF", theme::DIM_COLOR) };
            Text::new(ct, Point::new(4, 100), MonoTextStyle::new(&FONT_6X13, cc)).draw(display)?;
            Text::new(ht, Point::new(140, 100), MonoTextStyle::new(&FONT_6X13, hc)).draw(display)?;
        }

        // ─── Circuit ds0..ds3 ─────────────────────────────────────────────────
        for i in 0..4usize {
            let mut s: String<16> = String::new();
            let (text, color) = match snap.temps[i] {
                Some(m) if !m.value.0.is_nan() => {
                    write!(s, "{}: {:+5.1}C", &TEMP_LABELS[i][..3.min(TEMP_LABELS[i].len())], m.value.0).ok();
                    (s.as_str(), theme::TEXT_COLOR)
                }
                _ => ("---", theme::DIM_COLOR),
            };
            let x = 4 + (i as i32 % 2) * 158;
            let y = 118 + (i as i32 / 2) * 14;
            Text::new(text, Point::new(x, y), MonoTextStyle::new(&FONT_6X10, color)).draw(display)?;
        }

        // ─── Pression chambre ─────────────────────────────────────────────────
        {
            let mut s: String<32> = String::new();
            // `HectoPascal` porte bien des hPa (`Abp2Sensor` convertit les
            // bar du capteur, `Bme280Sensor` lit déjà des hPa) — l'ancien
            // libellé « bar » affichait donc la valeur 1000× trop grande.
            match snap.press[CHAMBER_PRESSURE_IDX].map(|m| m.value.0) {
                Some(p) => { write!(s, "Pression: {:7.1} hPa", p).ok(); }
                None => { write!(s, "Pression: ---").ok(); }
            }
            Text::new(s.as_str(), Point::new(4, 150), MonoTextStyle::new(&FONT_6X10, theme::TEXT_COLOR))
                .draw(display)?;
        }

        // BME280 (ambiance) et sursaturation IPA : non représentables,
        // cf. doc du module en tête de fichier.

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
    fn draws_idle_without_error() {
        let mut d = make_display();
        let state = SharedState {
            snapshot: Default::default(),
            task: SystemTask::Idle,
            new_data: false,
        };
        StatsScreen { state: &state }.draw(&mut d).unwrap();
    }

    #[test]
    fn draws_tripped_without_error() {
        let mut d = make_display();
        let state = SharedState {
            snapshot: Default::default(),
            task: SystemTask::Tripped(SafetyCause::CompressorOverheat),
            new_data: false,
        };
        StatsScreen { state: &state }.draw(&mut d).unwrap();
    }

    #[test]
    fn draws_cooling_phase_without_error() {
        let mut d = make_display();
        let state = SharedState {
            snapshot: Default::default(),
            task: SystemTask::Cooling(CoolingPhase::HighVoltage),
            new_data: false,
        };
        StatsScreen { state: &state }.draw(&mut d).unwrap();
    }
}
