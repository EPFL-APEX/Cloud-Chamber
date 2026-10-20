//! Écran « cycle en cours » : la séquence en checklist pendant
//! `Cooling` **et pendant `Stopping`**, un panneau de régime pendant
//! `Stabilising` — le tout sur la grille de `screens::stats`.
//!
//! Affichage + **bouton d'action principal**, dérivé de [`SharedState`] —
//! comme [`super::stats`], il n'a aucun état propre et est reconstruit à
//! chaque rendu par [`crate::ui::router`]. C'est
//! `logic::control_loop` qui fait avancer la machine ; cet écran montre où
//! elle en est, et porte le bouton START/STOP :
//!
//! - **rotation** : déplace la sélection entre le bouton d'action et le
//!   bouton `BACK` — la sélection vit dans le routeur, cet écran est
//!   reconstruit à chaque rendu et la reçoit en champ ;
//! - **clic** : active le bouton sélectionné — l'action (`START` machine
//!   à l'arrêt, `STOP` cycle en cours, `ACQUITTER` déclenchement) ou le
//!   retour au menu (`BACK`).
//!
//! # Le contenu suit l'état
//!
//! - `Cooling(_)`: la checklist des six étapes de démarrage, l'étape
//!   courante au carré rouge, les faites au carré plein, les autres en
//!   contour (cf. [`PHASES`]).
//! - `Stopping(_)`: la checklist des quatre étapes d'arrêt, même
//!   mécanique d'avancement (cf. [`STOPPING_STEPS`]) — l'opérateur voit
//!   la séquence se dérouler aussi en fin de cycle, pas seulement au
//!   démarrage.
//! - `Stabilising` : plus de checklist, il n'y a plus de séquence à
//!   suivre — un panneau de régime : sonde pilote en grand, écart à la
//!   cible, bande de régulation, et le verdict `STABLE` / `HORS BANDE`
//!   (cf. [`SteadyState`]).
//! - `Idle` : la checklist de démarrage en attente — l'aperçu de ce que
//!   START lancera.
//! - `Tripped(_)` : la cause en toutes lettres sous le mot de phase
//!   rouge, la sonde pilote en grand, et les sorties attendues (coupées)
//!   en carrés — l'ACQUITTER est le seul geste utile.

use core::fmt::Write as _;
use heapless::String;

use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{MonoTextStyle, ascii::{FONT_6X10, FONT_6X13, FONT_9X18_BOLD, FONT_10X20}},
    pixelcolor::Rgb565,
    primitives::{Line, Primitive, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle},
    text::{Baseline, Text, TextStyleBuilder},
};

use crate::cloud_chamber_hal::config::PLATE_MID_IDX;
use crate::config::operating::REGULATION_BAND_C;
use crate::logic::cooling::CoolingPhase;
use crate::logic::stopping::StoppingPhase;
use crate::shared::data::{SharedState, SystemTask};
use crate::shared::settings;
use crate::ui::theme;

use super::stats::{alert_message, phase_label};
use super::widgets::{Status, StatusLine, StatusLines};

const SCREEN_WIDTH: i32 = 320;
/// Marge gauche, celle de toute la grille.
const MARGIN: i32 = 12;
/// Ordonnées des trois filets qui découpent l'écran. Le bandeau
/// d'état est serré (48 px) : mesuré au densitomètre, l'ancien
/// espacement laissait un trou de 30 px entre le mot de phase et la
/// checklist, et tout le poids de l'écran tombait vers le bas.
const RULE_CONTENT: i32 = 48;
const RULE_BUTTONS: i32 = 170;
/// Hauteur d'un rang de la checklist de démarrage : six rangs remplissent
/// la zone de contenu d'un trait.
const COOLING_ROW_HEIGHT: i32 = 18;
/// Idem pour les quatre rangs de la séquence d'arrêt : plus aérés, même
/// zone — la checklist d'arrêt ne se termine pas à mi-hauteur.
const STOPPING_ROW_HEIGHT: i32 = 26;
/// Filet vertical de la zone héroïne (panneaux `Stabilising`/`Tripped`).
const VERTICAL_RULE_X: i32 = 160;

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

/// Les étapes de `Stopping`, dans l'ordre où la machine les traverse —
/// même exigence d'ordre que [`PHASES`], garanti par le test
/// `stopping_steps_match_the_sequence`.
const STOPPING_STEPS: [(StoppingPhase, &str); 4] = [
    (StoppingPhase::CutHighVoltage, "Couper haute tension"),
    (StoppingPhase::CutIsoprop, "Couper circulation IPA"),
    (StoppingPhase::CutCompressor, "Couper compresseur"),
    (StoppingPhase::WaitPressureEquilibrium, "Equilibrage pression"),
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

/// Rang d'une phase dans [`STOPPING_STEPS`] — mêmes garanties que
/// [`phase_index`].
fn stopping_index(phase: StoppingPhase) -> usize {
    STOPPING_STEPS
        .iter()
        .position(|(p, _)| *p == phase)
        .unwrap_or(0)
}

/// Statut à afficher pour l'étape de rang `row` d'une séquence, vu la
/// phase courante : les rangs passés sont faits, le rang courant est
/// actif, les suivants en attente.
fn status_for(row: usize, current: usize) -> Status {
    if row < current {
        Status::Done
    } else if row == current {
        Status::Active
    } else {
        Status::Pending
    }
}

/// Le régime autour de la cible, pendant `Stabilising` : dans la bande
/// de régulation du compresseur ou dehors. C'est l'information que
/// l'opérateur vient chercher en régime établi — pas une checklist,
/// un verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteadyState {
    /// À ± [`REGULATION_BAND_C`] de la cible : le régulateur à
    /// hystérésis tient la plaque.
    InBand,
    /// Dehors : la plaque dérive, le compresseur (ou la fuite) a du
    /// travail — pas encore « stabilisé » au sens où l'écran
    /// l'entend.
    OutOfBand,
}

impl SteadyState {
    /// Classifie l'écart mesuré à la cible.
    pub fn of(temp_c: f32, target_c: f32) -> Self {
        if (temp_c - target_c).abs() <= REGULATION_BAND_C.0 {
            SteadyState::InBand
        } else {
            SteadyState::OutOfBand
        }
    }

    /// Le mot affiché, et sa couleur — `STABLE` se lit vert de loin,
    /// `HORS BANDE` en avertissement.
    pub fn label(self) -> (&'static str, Rgb565) {
        match self {
            SteadyState::InBand => ("STABLE", theme::SUCCESS_COLOR),
            SteadyState::OutOfBand => ("HORS BANDE", theme::WARNING_COLOR),
        }
    }
}

/// Le bouton de l'écran de suivi actuellement sélectionné par la
/// rotation — la sélection vit dans `ui::router::Screens`, pas ici :
/// cet écran est reconstruit à chaque rendu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunningButton {
    /// Le bouton d'action principal (START / STOP / ACQUITTER).
    Action,
    /// Le retour au menu.
    Back,
}

impl RunningButton {
    /// L'autre bouton — la rotation va à l'autre, quel que soit le sens :
    /// deux boutons, pas d'ordre, un toggle. Sans ça, tourner dans un
    /// sens sélectionnait BACK et aucun geste ne revenait sur l'action.
    pub fn other(self) -> Self {
        match self {
            RunningButton::Action => RunningButton::Back,
            RunningButton::Back => RunningButton::Action,
        }
    }
}

/// Écran « cycle en cours ». `selected` vient de la sélection du routeur.
pub struct RunningScreen<'a> {
    pub state: &'a SharedState,
    pub selected: RunningButton,
}

impl<'a> RunningScreen<'a> {
    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        let task = self.state.task;
        let top_style = TextStyleBuilder::new().baseline(Baseline::Top).build();

        display.clear(theme::BACKGROUND_COLOR)?;

        // ─── Bandeau : phase + sonde pilote ──────────────────────────────────
        // Même bandeau que l'écran de statistiques : petit libellé gris,
        // mot de phase en gros, mesure qui pilote à droite.
        Text::with_text_style(
            "PHASE",
            Point::new(MARGIN, 6),
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
            Point::new(MARGIN, 16),
            MonoTextStyle::new(&FONT_9X18_BOLD, phase_color),
            top_style,
        )
        .draw(display)?;

        // La cause de déclenchement en toutes lettres — c'est elle que
        // l'opérateur cherche en arrivant sur un écran rouge.
        if let SystemTask::Tripped(cause) = task {
            Text::with_text_style(
                alert_message(cause),
                Point::new(MARGIN, 36),
                MonoTextStyle::new(&FONT_6X10, theme::DANGER_COLOR),
                top_style,
            )
            .draw(display)?;
        }

        // Sonde pilote, alignée à droite du bandeau.
        let mut pilot: String<16> = String::new();
        match self.state.snapshot.temps[PLATE_MID_IDX] {
            Some(m) if !m.value.is_nan() => {
                let _ = write!(pilot, "{:+.1} C", m.value.0);
                (pilot.as_str(), theme::TEXT_COLOR)
            }
            _ => ("---", theme::DIM_COLOR),
        };
        Text::with_text_style(
            "CHAMBRE",
            Point::new(308 - 7 * 6, 6),
            MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
            top_style,
        )
        .draw(display)?;
        Text::with_text_style(
            pilot.as_str(),
            Point::new(308 - pilot.len() as i32 * 6, 18),
            MonoTextStyle::new(&FONT_6X13, theme::TEXT_COLOR),
            top_style,
        )
        .draw(display)?;

        hline(display, 0, SCREEN_WIDTH, RULE_CONTENT)?;

        // ─── Contenu : suit l'état ────────────────────────────────────────────
        match task {
            SystemTask::Cooling(phase) => {
                let current = phase_index(phase);
                let mut lines = [StatusLine { label: "", status: Status::Pending }; PHASES.len()];
                for (row, (_, label)) in PHASES.iter().enumerate() {
                    lines[row] = StatusLine { label, status: status_for(row, current) };
                }
                StatusLines::<{ PHASES.len() }, false> { lines, top: RULE_CONTENT + 10, line_height: COOLING_ROW_HEIGHT }
                    .draw(display)?;
            }
            SystemTask::Idle => {
                // Aperçu de ce que START lancera, tout en attente.
                let mut lines = [StatusLine { label: "", status: Status::Pending }; PHASES.len()];
                for (row, (_, label)) in PHASES.iter().enumerate() {
                    lines[row] = StatusLine { label, status: Status::Pending };
                }
                StatusLines::<{ PHASES.len() }, false> { lines, top: RULE_CONTENT + 10, line_height: COOLING_ROW_HEIGHT }
                    .draw(display)?;
            }
            SystemTask::Stopping(phase) => {
                let current = stopping_index(phase);
                let mut lines =
                    [StatusLine { label: "", status: Status::Pending }; STOPPING_STEPS.len()];
                for (row, (_, label)) in STOPPING_STEPS.iter().enumerate() {
                    lines[row] = StatusLine { label, status: status_for(row, current) };
                }
                StatusLines::<{ STOPPING_STEPS.len() }, false> { lines, top: RULE_CONTENT + 10, line_height: STOPPING_ROW_HEIGHT }
                    .draw(display)?;
            }
            SystemTask::Stabilising => self.draw_steady_panel(display)?,
            SystemTask::Tripped(_) => self.draw_tripped_panel(display)?,
        }

        hline(display, 0, SCREEN_WIDTH, RULE_BUTTONS)?;

        // ─── Boutons : action principal + BACK ───────────────────────
        //
        // START à l'arrêt, STOP en course, ACQUITTER en déclenchement —
        // le même clic fait toujours « la chose évidente » vu l'état
        // affiché. En `Stopping`, la machine exécute déjà la séquence :
        // le bouton propose REPRENDRE (cf. la garde dans `ui::router`).
        let (label, action_color) = match task {
            SystemTask::Idle => ("START", theme::SUCCESS_COLOR),
            SystemTask::Cooling(_) | SystemTask::Stabilising => {
                ("STOP", theme::DANGER_COLOR)
            }
            SystemTask::Tripped(_) => ("ACQUITTER", theme::WARNING_COLOR),
            // Pendant la séquence d'arrêt, tout est déjà coupé
            // proprement : reprendre repasse par `SensorCheck`. Un
            // « ... » muet ici redémarrait la machine sans le dire.
            SystemTask::Stopping(_) => ("REPRENDRE", theme::ACCENT_COLOR),
        };

        // ─── Boutons : compacts, alignés à droite du pied ──────────────────
        //
        // Un bouton, c'est un filet d'un pixel et du texte — la couleur
        // porte le sens (rouge = STOP, vert = START), pas le volume. La
        // sélection s'inverse : rectangle plein, texte couleur du fond.
        // Posés en bas à droite : l'action proche de la main, l'aide de
        // geste en face, à gauche, sur la même ligne de base.
        let action_selected = self.selected == RunningButton::Action;
        // 9 px par caractère en 9×18 gras, 8 px de respiration de part
        // et d'autre, 26 px de haut — juste ce qu'il faut pour le doigt
        // et l'œil, pas un bloc.
        let action_w = label.len() as i32 * 9 + 16;
        let action_x = 308 - action_w;
        outlined_button(
            display,
            Rectangle::new(Point::new(action_x, 178), Size::new(action_w as u32, 26)),
            action_color,
            action_selected,
        )?;
        Text::with_text_style(
            label,
            Point::new(action_x + 8, 182),
            MonoTextStyle::new(
                &FONT_9X18_BOLD,
                if action_selected { theme::BACKGROUND_COLOR } else { action_color },
            ),
            top_style,
        )
        .draw(display)?;

        // BACK : plus petit que l'action — c'est une sortie, pas une
        // commande. Filet et texte discrets, à gauche de l'action.
        let back_selected = self.selected == RunningButton::Back;
        let back_w = 4 * 6 + 16;
        let back_x = action_x - 10 - back_w;
        outlined_button(
            display,
            Rectangle::new(Point::new(back_x, 178), Size::new(back_w as u32, 26)),
            theme::DIM_COLOR,
            back_selected,
        )?;
        Text::with_text_style(
            "BACK",
            Point::new(back_x + 8, 184),
            MonoTextStyle::new(
                &FONT_6X13,
                if back_selected { theme::BACKGROUND_COLOR } else { theme::DIM_COLOR },
            ),
            top_style,
        )
        .draw(display)?;

        // ─── Aide de geste ──────────────────────────────────────────
        // En face des boutons, sur deux lignes courtes : jamais sous la
        // main d'un BACK, même avec l'action la plus large.
        Text::with_text_style(
            "Tourner: choisir",
            Point::new(MARGIN, 178),
            MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
            top_style,
        )
        .draw(display)?;
        Text::with_text_style(
            "Clic: valider",
            Point::new(MARGIN, 194),
            MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
            top_style,
        )
        .draw(display)?;

        Ok(())
    }

    /// Panneau de régime (`Stabilising`) : plus de séquence à suivre —
    /// la sonde pilote en grand, l'écart à la cible, la bande de
    /// régulation, et le verdict.
    fn draw_steady_panel<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        let top_style = TextStyleBuilder::new().baseline(Baseline::Top).build();

        // En régime établi, le compresseur régule autour de la cible de
        // saturation (cf. `logic::control_loop`, bras `Stabilising`).
        let target = settings::get().saturation_target;

        Text::with_text_style(
            "MILIEU PLAQUE",
            Point::new(MARGIN, 56),
            MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
            top_style,
        )
        .draw(display)?;

        match self.state.snapshot.temps[PLATE_MID_IDX] {
            Some(m) if !m.value.is_nan() => {
                let mut v: String<12> = String::new();
                let _ = write!(v, "{:+.1}", m.value.0);
                Text::with_text_style(
                    v.as_str(),
                    Point::new(MARGIN, 68),
                    MonoTextStyle::new(&FONT_10X20, theme::TEXT_COLOR),
                    top_style,
                )
                .draw(display)?;
                Text::with_text_style(
                    "C",
                    Point::new(MARGIN + v.len() as i32 * 10 + 2, 73),
                    MonoTextStyle::new(&FONT_6X13, theme::DIM_COLOR),
                    top_style,
                )
                .draw(display)?;

                // ─── Écart à la cible : l'information du régime ───────
                let ecart = m.value.0 - target.0;
                v.clear();
                let _ = write!(v, "{:+.1} C", ecart);
                let (verdict, verdict_color) = SteadyState::of(m.value.0, target.0).label();
                Text::with_text_style(
                    "ECART A LA CIBLE",
                    Point::new(172, 56),
                    MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
                    top_style,
                )
                .draw(display)?;
                Text::with_text_style(
                    v.as_str(),
                    Point::new(172, 68),
                    MonoTextStyle::new(&FONT_9X18_BOLD, theme::TEXT_COLOR),
                    top_style,
                )
                .draw(display)?;

                // La bande du régulateur : ce que « stable » veut dire
                // ici, chiffres posés.
                Text::with_text_style(
                    "BANDE",
                    Point::new(172, 94),
                    MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
                    top_style,
                )
                .draw(display)?;
                let mut b: String<16> = String::new();
                let _ = write!(b, "+/-{:.1} C", REGULATION_BAND_C.0);
                Text::with_text_style(
                    b.as_str(),
                    Point::new(172, 106),
                    MonoTextStyle::new(&FONT_6X13, theme::TEXT_COLOR),
                    top_style,
                )
                .draw(display)?;

                Text::with_text_style(
                    "ETAT",
                    Point::new(172, 130),
                    MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
                    top_style,
                )
                .draw(display)?;
                Text::with_text_style(
                    verdict,
                    Point::new(172, 142),
                    MonoTextStyle::new(&FONT_9X18_BOLD, verdict_color),
                    top_style,
                )
                .draw(display)?;
            }
            // Sonde muette : pas de panneau de régime à inventer.
            _ => {
                Text::with_text_style(
                    "---",
                    Point::new(MARGIN, 72),
                    MonoTextStyle::new(&FONT_6X13, theme::DIM_COLOR),
                    top_style,
                )
                .draw(display)?;
            }
        }

        Line::new(
            Point::new(VERTICAL_RULE_X, 56),
            Point::new(VERTICAL_RULE_X, RULE_BUTTONS),
        )
        .into_styled(PrimitiveStyle::with_stroke(theme::DIM_COLOR, 1))
        .draw(display)?;

        Ok(())
    }

    /// Panneau de déclenchement (`Tripped`) : la sonde pilote en grand,
    /// et les sorties attendues — toutes coupées — en carrés.
    fn draw_tripped_panel<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        let top_style = TextStyleBuilder::new().baseline(Baseline::Top).build();

        Text::with_text_style(
            "MILIEU PLAQUE",
            Point::new(MARGIN, 56),
            MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
            top_style,
        )
        .draw(display)?;
        let mut v: String<12> = String::new();
        let hero = match self.state.snapshot.temps[PLATE_MID_IDX] {
            Some(m) if !m.value.is_nan() => {
                let _ = write!(v, "{:+.1}", m.value.0);
                (v.as_str(), theme::TEXT_COLOR)
            }
            _ => ("---", theme::DIM_COLOR),
        };
        Text::with_text_style(
            hero.0,
            Point::new(MARGIN, 68),
            MonoTextStyle::new(&FONT_10X20, hero.1),
            top_style,
        )
        .draw(display)?;

        Line::new(
            Point::new(VERTICAL_RULE_X, 56),
            Point::new(VERTICAL_RULE_X, RULE_BUTTONS),
        )
        .into_styled(PrimitiveStyle::with_stroke(theme::DIM_COLOR, 1))
        .draw(display)?;

        // Tripped = toutes sorties coupées (`logic::security`) : les
        // carrés attendus sont éteints — les montrer dit le silence
        // plutôt que le laisser deviner.
        Text::with_text_style(
            "SORTIES",
            Point::new(172, 56),
            MonoTextStyle::new(&FONT_6X10, theme::DIM_COLOR),
            top_style,
        )
        .draw(display)?;
        Text::with_text_style(
            "TOUTES COUPEES",
            Point::new(172, 68),
            MonoTextStyle::new(&FONT_6X13, theme::TEXT_COLOR),
            top_style,
        )
        .draw(display)?;

        Ok(())
    }
}

/// Filet horizontal d'un pixel — même signature que `screens::stats`.
fn hline<D>(d: &mut D, x0: i32, x1: i32, y: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565> + OriginDimensions,
{
    Line::new(Point::new(x0, y), Point::new(x1, y))
        .into_styled(PrimitiveStyle::with_stroke(theme::DIM_COLOR, 1))
        .draw(d)
}

/// Bouton filaire : rectangle au filet d'un pixel dans la couleur du
/// geste, plein et inversé quand sélectionné. Pas de relief, pas
/// d'ombre — la sélection s'entend par l'inversion, les couleurs par
/// le texte.
fn outlined_button<D>(
    d: &mut D,
    bounds: Rectangle,
    color: Rgb565,
    selected: bool,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565> + OriginDimensions,
{
    let style = if selected {
        PrimitiveStyleBuilder::new()
            .fill_color(color)
            .stroke_color(color)
            .stroke_width(1)
            .build()
    } else {
        PrimitiveStyleBuilder::new()
            .stroke_color(color)
            .stroke_width(1)
            .build()
    };
    bounds.into_styled(style).draw(d)
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
        SharedState { snapshot: Default::default(), task, new_data: false, core1_panicked: false }
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
        let current = phase_index(CoolingPhase::SaturatingAirWithIpa);
        let statuses: [Status; PHASES.len()] =
            core::array::from_fn(|row| status_for(row, current));

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
        let current = phase_index(CoolingPhase::SensorCheck);
        assert_eq!(status_for(0, current), Status::Active);
        for row in 1..PHASES.len() {
            assert_eq!(status_for(row, current), Status::Pending);
        }
    }

    /// Garde-fou jumeau de `phase_order_matches_the_cooling_sequence` :
    /// une variante de `StoppingPhase` ajoutée sans être placée dans
    /// `STOPPING_STEPS` ferait échouer ce test.
    #[test]
    fn stopping_steps_match_the_sequence() {
        use StoppingPhase::*;
        let expected = [CutHighVoltage, CutIsoprop, CutCompressor, WaitPressureEquilibrium];
        let listed: [StoppingPhase; STOPPING_STEPS.len()] = STOPPING_STEPS.map(|(p, _)| p);
        assert_eq!(listed, expected);

        for (i, phase) in expected.iter().enumerate() {
            assert_eq!(stopping_index(*phase), i);
        }
    }

    /// La checklist d'arrêt se remplit comme celle du démarrage — c'est
    /// le comportement demandé par l'opérateur : voir la séquence
    /// d'arrêt se dérouler, pas seulement celle de démarrage.
    #[test]
    fn the_stopping_checklist_fills_as_the_sequence_progresses() {
        let current = stopping_index(StoppingPhase::CutCompressor);
        let statuses: [Status; STOPPING_STEPS.len()] =
            core::array::from_fn(|row| status_for(row, current));

        assert_eq!(
            statuses,
            [
                Status::Done,    // Couper haute tension
                Status::Done,    // Couper circulation IPA
                Status::Active,  // Couper compresseur <- en cours
                Status::Pending, // Equilibrage pression
            ]
        );
    }

    /// Le verdict du panneau de régime : dans la bande du régulateur ou
    /// dehors — la frontière est la demi-bande de l'hystérésis.
    #[test]
    fn steady_state_classifies_the_gap_to_the_target() {
        let target = -35.0_f32;
        // La bande vaut 1.0 °C (`REGULATION_BAND_C`) : ±1.0 de part et
        // d'autre de la cible tient, un dixième de plus sort.
        assert_eq!(SteadyState::of(target, target), SteadyState::InBand);
        assert_eq!(SteadyState::of(target + 1.0, target), SteadyState::InBand);
        assert_eq!(SteadyState::of(target - 1.0, target), SteadyState::InBand);
        assert_eq!(SteadyState::of(target + 1.1, target), SteadyState::OutOfBand);
        assert_eq!(SteadyState::of(target - 3.0, target), SteadyState::OutOfBand);
    }

    #[test]
    fn draws_every_cooling_phase_without_error() {
        for (phase, _) in PHASES {
            let mut d = make_display();
            let state = state_with(SystemTask::Cooling(phase));
            RunningScreen { state: &state, selected: RunningButton::Action }.draw(&mut d).unwrap();
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
            RunningScreen { state: &state, selected: RunningButton::Action }.draw(&mut d).unwrap();
        }
    }

    #[test]
    fn draws_with_a_chamber_reading_without_error() {
        let mut d = make_display();
        let mut state = state_with(SystemTask::Cooling(CoolingPhase::HighVoltage));
        state.snapshot.temps[PLATE_MID_IDX] =
            Some(Measurement::new(Instant::from_micros(1), Celsius(-32.4)));
        RunningScreen { state: &state, selected: RunningButton::Action }.draw(&mut d).unwrap();
    }

    #[test]
    fn running_screenshot() -> Result<(), core::convert::Infallible> {
        use embedded_graphics_simulator::OutputSettingsBuilder;

        let mut display = make_display();
        let mut state = state_with(SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa));
        state.snapshot.temps[PLATE_MID_IDX] =
            Some(Measurement::new(Instant::from_micros(1), Celsius(-28.6)));
        RunningScreen { state: &state, selected: RunningButton::Action }.draw(&mut display)?;

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
