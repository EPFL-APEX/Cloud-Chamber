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
    primitives::{Line, Primitive, PrimitiveStyle, Rectangle},
    text::{Baseline, Text, TextStyleBuilder},
    Drawable,
};

use crate::{
    cloud_chamber_hal::config::{
        CHAMBER_PRESSURE_IDX, PLATE_MID_IDX, NUMBER_OF_TEMP_SENSOR,
    },
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
        SystemTask::Stopping(CutIsoprop) => "ARRET: POMPE IPA",
        SystemTask::Stopping(CutCompressor) => "ARRET: COMP.",
        SystemTask::Stopping(WaitPressureEquilibrium) => "ARRET: EQUIL.",
        SystemTask::Tripped(_) => "ARRET SECURITE",
    }
}

/// Cause de déclenchement en toutes lettres — partagé avec
/// `screens::running`, qui l'affiche dans son bandeau sur l'état
/// `Tripped` (même vocabulaire des deux côtés, une seule table).
pub(super) fn alert_message(cause: SafetyCause) -> &'static str {
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
///
/// Quatre sorties, pas seulement les deux pilotant la sécurité : la pompe
/// d'isopropanol et le chauffage de vitre font partie de ce que
/// l'opérateur vient vérifier. Les valeurs reflètent les `ActuatorPlan`
/// de chaque phase, relus à la main — `Stabilising` maintient tout ce
/// que `FinalCheckBeforeStabilising` avait mis en marche, pompe, vitre
/// et éclairage compris (le régime d'observation).
pub(super) struct ExpectedOutputs {
    pub compressor: bool,
    pub high_voltage: bool,
    pub iso_pump: bool,
    pub glass_heater: bool,
}

pub(super) fn expected_outputs(task: SystemTask) -> ExpectedOutputs {
    use CoolingPhase::*;
    use StoppingPhase::*;
    let (compressor, high_voltage, iso_pump, glass_heater) = match task {
        SystemTask::Cooling(SensorCheck) => (false, false, false, false),
        SystemTask::Cooling(PreCoolingThePlate) => (true, false, false, false),
        SystemTask::Cooling(StartingIpaCirculation) => (true, false, true, true),
        SystemTask::Cooling(SaturatingAirWithIpa) => (true, false, true, true),
        SystemTask::Cooling(HighVoltage) => (true, true, true, true),
        SystemTask::Cooling(FinalCheckBeforeStabilising) => (true, true, true, true),
        SystemTask::Stabilising => (true, true, true, true),
        SystemTask::Stopping(CutHighVoltage) => (true, false, true, true),
        SystemTask::Stopping(CutIsoprop) => (true, false, false, true),
        SystemTask::Stopping(CutCompressor) => (false, false, false, false),
        SystemTask::Stopping(WaitPressureEquilibrium) => (false, false, false, false),
        SystemTask::Idle | SystemTask::Tripped(_) => (false, false, false, false),
    };
    ExpectedOutputs { compressor, high_voltage, iso_pump, glass_heater }
}

/// Écran de statistiques en direct : la machine entière sur un coup
/// d'œil — phase, sonde pilote, cible, pression, fermeture, sorties
/// attendues, et les huit sondes.
///
/// # Mise en page
///
/// Trois zones coupées par des filets d'un pixel — le seul ornement :
///
/// ```text
/// PHASE                        SORTIES
/// HAUTE TENSION                ▪ COMP    ▪ HV
/// ─────────────────────────────────────────────  y=56
/// MILIEU PLAQUE   |  CIBLE -40.0 C    CHAMBRE
/// -35.2 C        |  1013 hPa         FERMEE
/// ─────────────────────────────────────────────  y=128
/// CHAUFFAGE IPA       +40.1 │ SORTIE RADIAT.     +38.9
/// ENTREE RADIAT.      -12.4 │ COMPRESSEUR        +42.3
/// SORTIE PLAQUE       +21.7 │ ENTREE COMP.       +20.9
/// ENTREE PLAQUE       -18.7 │
/// ```
///
/// La grille et les alignements font le travail : libellés en petit
/// gris, valeurs en blanc, une seule couleur vive (le carré d'état
/// rouge quand la sortie est attendue active, le mot de phase en rouge
/// sur déclenchement sécurité).
pub struct StatsScreen<'a> {
    pub state: &'a SharedState,
}

/// Libellés d'affichage des sondes, dans l'ordre du bus
/// (`config::wiring::TEMP_LABELS`). Sans accents : les fontes `ascii`
/// n'ont pas de É. La sonde pilote (index 4) y a sa place pour garder
/// l'alignement des index, mais elle est rendue en zone héroïne, pas
/// dans la table.
/// Libellés d'affichage des sondes, dans l'ordre du bus
/// (`config::wiring::TEMP_LABELS`). Sans accents : les fontes `ascii`
/// n'ont pas de É. La sonde pilote (milieu de plaque) y a sa place pour
/// garder l'alignement des index, mais elle est rendue en zone héroïne,
/// pas dans la table.
const DISPLAY_LABELS: [&str; NUMBER_OF_TEMP_SENSOR] = [
    "CHAUFFAGE IPA",
    "SORTIE RADIAT.",
    "ENTREE RADIAT.",
    "MILIEU PLAQUE",
    "COMPRESSEUR",
    "SORTIE PLAQUE",
    "ENTREE COMP.",
    "ENTREE PLAQUE",
];

/// Ordonnées des deux filets qui découpent l'écran.
const RULE_Y: [i32; 2] = [56, 128];
/// Abscisse du filet vertical de la zone héroïne.
const VERTICAL_RULE_X: i32 = 160;
/// Marge gauche, celle de toutes les zones.
const MARGIN: i32 = 12;

impl<'a> StatsScreen<'a> {
    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        let snap = &self.state.snapshot;
        let task = self.state.task;
        // Les cibles affichées sont celles de l'opérateur, pas les
        // constantes de `operating` : l'écran de réglages les change à
        // chaud, et afficher la constante ferait croire que l'édition
        // n'a pas pris (vu en pratique : « Cible: -40.0C » quoi qu'il
        // arrive).
        let settings = crate::shared::settings::get();

        display.clear(theme::BACKGROUND_COLOR)?;

        // ─── Bandeau : phase + sorties attendues ─────────────────────────────
        let top_style = TextStyleBuilder::new().baseline(Baseline::Top).build();
        Text::with_text_style(
            "PHASE",
            Point::new(MARGIN, 8),
            MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
            top_style,
        )
        .draw(display)?;

        let phase_color = match task {
            SystemTask::Tripped(_) => theme::DANGER_COLOR,
            SystemTask::Idle => theme::DIM_COLOR,
            _ => theme::TEXT_COLOR,
        };
        Text::with_text_style(
            phase_label(task),
            Point::new(MARGIN, 20),
            MonoTextStyle::new(&FONT_9X18_BOLD, phase_color),
            top_style,
        )
        .draw(display)?;

        // Sur déclenchement, la cause sous la phase — c'est elle que
        // l'opérateur cherche en traversant la pièce.
        if let SystemTask::Tripped(cause) = task {
            Text::with_text_style(
                alert_message(cause),
                Point::new(MARGIN, 42),
                MonoTextStyle::new(&FONT_6X10, theme::DANGER_COLOR),
                top_style,
            )
            .draw(display)?;
        }

        // Sorties attendues, à droite du bandeau : les quatre sorties
        // de la machine en grille 2×2, un carré azur par sortie active,
        // gris pour éteinte — lu comme un pictogramme, pas une phrase.
        // Le carré est calé sur l'axe optique de son libellé : mark à
        // label_y + 2, soit le même centre (le décalage de la première
        // version se voyait).
        let outputs = expected_outputs(task);
        Text::with_text_style(
            "SORTIES",
            Point::new(180, 6),
            MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
            top_style,
        )
        .draw(display)?;
        // Colonne des libellés : la marque puis 4 px de gouttière, le
        // libellé 6×13. Deux rangs de deux.
        for (col, (label, on)) in [
            ("COMP", outputs.compressor),
            ("HV", outputs.high_voltage),
            ("IPA", outputs.iso_pump),
            ("VITRE", outputs.glass_heater),
        ]
        .iter()
        .enumerate()
        {
            let row = col / 2;
            let col_x = col % 2;
            let x = 180 + col_x as i32 * 68;
            let y = 18 + row as i32 * 16;
            state_mark(display, Point::new(x, y + 2), *on)?;
            Text::with_text_style(
                label,
                Point::new(x + 12, y),
                MonoTextStyle::new(&FONT_6X13, theme::TEXT_COLOR),
                top_style,
            )
            .draw(display)?;
        }

        // ─── Héroïne : sonde pilote + métadonnées ────────────────────────────
        let mut val: String<12> = String::new();
        match snap.temps[PLATE_MID_IDX] {
            Some(m) if !m.value.is_nan() => {
                let _ = write!(val, "{:+.1}", m.value.0);
                let width = val.len() as i32 * 10;
                Text::with_text_style(
                    val.as_str(),
                    Point::new(MARGIN, 76),
                    MonoTextStyle::new(&FONT_10X20, theme::TEXT_COLOR),
                    top_style,
                )
                .draw(display)?;
                Text::with_text_style(
                    // Pas de « ° » : les fontes `ascii` n'ont pas ce
                    // glyphe, le caractère serait ignoré au rendu.
                    "C",
                    Point::new(MARGIN + width + 2, 81),
                    MonoTextStyle::new(&FONT_6X13, theme::DIM_COLOR),
                    top_style,
                )
                .draw(display)?;
            }
            // Rien plutôt que « --- » en gros : une sonde muette ne doit
            // pas ressembler à une mesure.
            _ => {
                Text::with_text_style(
                    "---",
                    Point::new(MARGIN, 80),
                    MonoTextStyle::new(&FONT_6X13, theme::DIM_COLOR),
                    top_style,
                )
                .draw(display)?;
            }
        }

        let mut s: String<24> = String::new();
        // Cible opérateur.
        Text::with_text_style(
            "CIBLE",
            Point::new(172, 64),
            MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
            top_style,
        )
        .draw(display)?;
        let _ = write!(s, "{:+.1} C", settings.chamber_target.0);
        Text::with_text_style(
            s.as_str(),
            Point::new(172, 76),
            MonoTextStyle::new(&FONT_6X13, theme::TEXT_COLOR),
            top_style,
        )
        .draw(display)?;

        // Pression — `HectoPascal` porte bien des hPa (cf. doc du
        // module pour l'unité, corrigée d'un affichage 1000× trop grand).
        Text::with_text_style(
            "PRESSION",
            Point::new(172, 92),
            MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
            top_style,
        )
        .draw(display)?;
        let mut p: String<24> = String::new();
        let (p_text, p_color) = match snap.press[CHAMBER_PRESSURE_IDX].map(|m| m.value.0) {
            Some(v) => {
                let _ = write!(p, "{:.0} hPa", v);
                (p.as_str(), theme::TEXT_COLOR)
            }
            None => ("---", theme::DIM_COLOR),
        };
        Text::with_text_style(
            p_text,
            Point::new(172, 104),
            MonoTextStyle::new(&FONT_6X13, p_color),
            top_style,
        )
        .draw(display)?;

        // Fermeture de la chambre.
        Text::with_text_style(
            "CHAMBRE",
            Point::new(252, 64),
            MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
            top_style,
        )
        .draw(display)?;
        Text::with_text_style(
            if snap.is_closed { "FERMEE" } else { "OUVERTE" },
            Point::new(252, 76),
            MonoTextStyle::new(&FONT_6X13, theme::TEXT_COLOR),
            top_style,
        )
        .draw(display)?;

        // ─── Filets de la grille ─────────────────────────────────────────────
        Line::new(Point::new(0, RULE_Y[0]), Point::new(320, RULE_Y[0]))
            .into_styled(PrimitiveStyle::with_stroke(theme::DIM_COLOR, 1))
            .draw(display)?;
        Line::new(Point::new(0, RULE_Y[1]), Point::new(320, RULE_Y[1]))
            .into_styled(PrimitiveStyle::with_stroke(theme::DIM_COLOR, 1))
            .draw(display)?;
        Line::new(
            Point::new(VERTICAL_RULE_X, 64),
            Point::new(VERTICAL_RULE_X, 112),
        )
        .into_styled(PrimitiveStyle::with_stroke(theme::DIM_COLOR, 1))
        .draw(display)?;

        // ─── Table des autres sondes : deux colonnes ─────────────────────────
        // La sonde pilote vit en zone héroïne ; les sept autres, une par
        // cellule, libellé gris sur valeur blanche, valeurs alignées à
        // droite de leur colonne.
        let mut cell = 0usize;
        for i in 0..NUMBER_OF_TEMP_SENSOR {
            if i == PLATE_MID_IDX {
                continue;
            }
            let col = (cell % 2) as i32;
            let row = (cell / 2) as i32;
            let x = MARGIN + col * 156;
            let y = 138 + row * 26;

            Text::with_text_style(
                DISPLAY_LABELS[i],
                Point::new(x, y),
                MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
                top_style,
            )
            .draw(display)?;

            let mut s: String<12> = String::new();
            let (text, color) = match snap.temps[i] {
                Some(m) if !m.value.is_nan() => {
                    let _ = write!(s, "{:+.1}", m.value.0);
                    (s.as_str(), theme::TEXT_COLOR)
                }
                _ => ("---", theme::DIM_COLOR),
            };
            Text::with_text_style(
                text,
                Point::new(x + 148 - text.len() as i32 * 6, y + 10),
                MonoTextStyle::new(&FONT_6X13, color),
                top_style,
            )
            .draw(display)?;

            cell += 1;
        }

        Ok(())
    }
}

/// Carré d'état 8×8 : azur si la sortie est attendue active, gris discret
/// sinon — le pictogramme du bandeau, dans la couleur des icônes.
fn state_mark<D>(display: &mut D, top_left: Point, on: bool) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565> + OriginDimensions,
{
    let color = if on { theme::HIGHLIGHT_COLOR } else { theme::DIM_COLOR };
    Rectangle::new(top_left, Size::new(8, 8))
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(display)
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
            core1_panicked: false,
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
            core1_panicked: false,
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
            core1_panicked: false,
        };
        StatsScreen { state: &state }.draw(&mut d).unwrap();
    }
}

