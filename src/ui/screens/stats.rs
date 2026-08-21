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
//! humidité, pression atmosphérique) : la ligne BME280 de l'original n'est
//! pas représentable telle quelle. Omise plutôt que d'inventer une donnée.
//! À revoir si `cloud_chamber_hal`/`SensorSnapshot` gagne un jour une
//! catégorie de mesure ambiante dédiée.
//!
//! L'indicateur de sursaturation, lui, était dans le même cas et ne l'est
//! plus : il prend ds3 (`ISO_TEMP_IDX`, sonde du thermostat feutre) comme
//! point chaud au lieu de l'ambiante du BME280. Voir `logic::saturation`,
//! qui porte aussi la réserve sur l'identité réelle de ds3.

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
    cloud_chamber_hal::config::{CHAMBER_PRESSURE_IDX, CHAMBER_TEMP_IDX, ISO_TEMP_IDX},
    config::operating::{SATURATION_TARGET_C, TARGET_CHAMBER_TEMP},
    config::wiring::TEMP_LABELS,
    logic::{cooling::CoolingPhase, saturation, security::SafetyCause, stopping::StoppingPhase},
    shared::data::{SharedState, SystemTask},
    ui::theme,
};

use super::widgets::ProgressBar;

fn phase_label(task: SystemTask) -> &'static str {
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
            match snap.press[CHAMBER_PRESSURE_IDX].map(|m| m.value.0) {
                Some(p) => { write!(s, "Pression: {:5.2}bar", p).ok(); }
                None => { write!(s, "Pression: ---").ok(); }
            }
            Text::new(s.as_str(), Point::new(4, 150), MonoTextStyle::new(&FONT_6X10, theme::TEXT_COLOR))
                .draw(display)?;
        }

        // ─── Sursaturation ────────────────────────────────────────────────────
        // Le seul indicateur qui réponde directement à « peut-on voir des
        // traces maintenant » — d'où sa place en bas, en pleine largeur,
        // lisible de loin. Cf. `logic::saturation` pour ce que vaut ce
        // rapport, et pour la réserve sur l'identité réelle de ds3.
        {
            // Une seule sonde manquante suffit à tout annuler : une barre
            // calculée sur une seule extrémité du gradient n'a aucun sens.
            let ratio = match (snap.temps[ISO_TEMP_IDX], snap.temps[CHAMBER_TEMP_IDX]) {
                (Some(warm), Some(cold)) => saturation::ratio(warm.value, cold.value),
                _ => None,
            };
            let progress = ratio.and_then(saturation::scale);

            let mut s: String<28> = String::new();
            match ratio {
                Some(r) => write!(s, "Sursaturation: x{:.0}", r),
                None => write!(s, "Sursaturation: ---"),
            }
            .ok();
            Text::new(s.as_str(), Point::new(4, 172), MonoTextStyle::new(&FONT_6X13, theme::TEXT_COLOR))
                .draw(display)?;

            // Vert seulement à 100 %, c'est-à-dire au point de
            // fonctionnement — pas en route vers lui.
            let color = match progress {
                Some(p) if p >= 1.0 => theme::SUCCESS_COLOR,
                _ => theme::WARNING_COLOR,
            };
            ProgressBar {
                top_left: Point::new(4, 180),
                size: Size::new(312, 16),
                ratio: progress,
                color,
            }
            .draw(display)?;
        }

        // BME280 (ambiance) : non représentable, cf. doc du module en tête
        // de fichier.

        Ok(())
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_graphics_simulator::SimulatorDisplay;

    use crate::cloud_chamber_hal::measurement::Measurement;
    use crate::cloud_chamber_hal::timer::Instant;
    use crate::cloud_chamber_hal::units::Celsius;
    use crate::shared::data::SensorSnapshot;
    use crate::shared::settings::with_isolated_settings;

    fn make_display() -> SimulatorDisplay<Rgb565> {
        SimulatorDisplay::new(Size::new(320, 240))
    }

    /// Chambre en cours de refroidissement : feutre à la consigne, plaque
    /// à mi-chemin. Les deux sondes de `logic::saturation` sont renseignées.
    fn cooling_snapshot() -> SensorSnapshot {
        let mut snap = SensorSnapshot::default();
        let now = Instant::from_micros(0);
        snap.temps[ISO_TEMP_IDX] = Some(Measurement::new(now, Celsius(40.0)));
        snap.temps[CHAMBER_TEMP_IDX] = Some(Measurement::new(now, Celsius(-30.0)));
        snap
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

    /// Le cas qui compte pour la barre : les deux sondes présentes. Les
    /// autres tests de ce module tournent tous sur un snapshot vide, donc
    /// sans jamais entrer dans le calcul de sursaturation.
    #[test]
    fn saturation_bar_screenshot() -> Result<(), core::convert::Infallible> {
        use embedded_graphics_simulator::OutputSettingsBuilder;

        // `saturation::scale` lit le static de réglages pour sa référence
        // — verrou obligatoire, cf. `with_isolated_settings`.
        with_isolated_settings(|| {
            let mut display = make_display();
            let state = SharedState {
                snapshot: cooling_snapshot(),
                task: SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa),
                new_data: true,
            };
            StatsScreen { state: &state }.draw(&mut display)?;

            let path = std::env::args_os()
                .nth(1)
                .unwrap_or_else(|| "screenshots/Stats.png".into());
            display
                .to_rgb_output_image(&OutputSettingsBuilder::new().build())
                .save_png(&path)
                .expect("failed to save screenshot");

            Ok(())
        })
    }
}
