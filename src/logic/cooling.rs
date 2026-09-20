//! Séquence de démarrage (refroidissement).
//!
//! Chaque phase ne réagit qu'aux mesures disponibles dans l'historique et
//! construit son propre `ActuatorPlan` en même temps que sa transition — une
//! seule décision par phase, pas de table séparée ailleurs qui pourrait
//! diverger. Aucune notion de durée ici : les délais (attente minimale,
//! abandon si trop long) sont du ressort de l'appelant (`control_loop.rs`),
//! qui seul connaît la durée passée dans la phase courante.

use crate::cloud_chamber_hal::config::CHAMBER_TEMP_IDX;
use crate::cloud_chamber_hal::units::Celsius;
use crate::config::operating::{STABLE_TOLERANCE_C, STABLE_WINDOW};
use crate::cloud_chamber_hal::actuators::ActuatorPlan;
use crate::logic::probing::{MeasurementHistory, ProbingPlan};
use crate::shared::data::SystemTask;
use crate::shared::settings;

/// Tolérance accordée à `FinalCheckBeforeStabilising` au-dessus de la cible
/// de saturation.
///
/// La phase précédente a déjà franchi la cible ; celle-ci ne fait que
/// vérifier qu'on n'en est pas ressorti pendant l'établissement de la haute
/// tension. Un seuil strictement égal la ferait osciller sur le bruit de
/// mesure du DS18B20 (±0,5 °C à 12 bits).
const FINAL_CHECK_MARGIN: Celsius = Celsius::new(2.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoolingPhase {
    SensorCheck,
    PreCoolingThePlate,
    StartingIpaCirculation,
    SaturatingAirWithIpa,
    HighVoltage,
    FinalCheckBeforeStabilising,
}

impl CoolingPhase {
    /// Sonde tout à chaque cycle pour l'instant — l'optimisation "sauter la
    /// conversion température coûteuse (~800ms) sur certaines phases",
    /// l'intention originale de `ProbingPlan`, reste un raffinement
    /// ultérieur, pas requise pour un premier cycle correct.
    pub fn create_probing_plan(&self, _prob_hist: &MeasurementHistory) -> ProbingPlan {
        ProbingPlan::all()
    }

    pub fn react_to(self, history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
        use CoolingPhase::*;
        match self {
            SensorCheck                 => sensor_check(history),
            PreCoolingThePlate          => pre_cooling_the_plate(history),
            StartingIpaCirculation      => starting_ipa_circulation(history),
            SaturatingAirWithIpa        => saturating_air_with_ipa(history),
            HighVoltage                 => high_voltage(history),
            FinalCheckBeforeStabilising => final_check_before_stabilising(history),
        }
    }
}

fn sensor_check(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let plan = ActuatorPlan::all_off();

    // Ajouter le check des autres sensors ?
    // #todo
    match history.has_valid_reading(CHAMBER_TEMP_IDX) {
        true => (SystemTask::Cooling(CoolingPhase::PreCoolingThePlate), plan),
        false => (SystemTask::Cooling(CoolingPhase::SensorCheck), plan),
    }
}

fn pre_cooling_the_plate(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let precool_target = settings::get().precool_target;
    let plan = ActuatorPlan::all_off().with_cooling(precool_target);
    match history.is_at_or_below(CHAMBER_TEMP_IDX, precool_target) {
        true => (SystemTask::Cooling(CoolingPhase::StartingIpaCirculation), plan),
        false => (SystemTask::Cooling(CoolingPhase::PreCoolingThePlate), plan),
    }
}

fn starting_ipa_circulation(_history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    // Purement temporisé (pas de capteur dédié) — avancement décidé par
    // l'appelant (cf. `timed_transition` dans control_loop.rs).
    let settings = settings::get();
    let plan = ActuatorPlan::all_off()
        .with_cooling(settings.precool_target)
        .with_iso_heater(settings.ipa_heater_target)
        .with_iso_pump()
        .with_glass_heater();
    (SystemTask::Cooling(CoolingPhase::StartingIpaCirculation), plan)
}

fn saturating_air_with_ipa(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let settings = settings::get();
    let plan = ActuatorPlan::all_off()
        .with_cooling(settings.saturation_target)
        .with_iso_heater(settings.ipa_heater_target)
        .with_iso_pump()
        .with_glass_heater();
    // #todo faire une vrai estimation de la saturation....
    match history.is_at_or_below(CHAMBER_TEMP_IDX, settings.saturation_target) {
        true => (SystemTask::Cooling(CoolingPhase::HighVoltage), plan),
        false => (SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa), plan),
    }
}

fn high_voltage(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let settings = settings::get();
    let plan = ActuatorPlan::all_off()
        .with_cooling(settings.saturation_target)
        .with_iso_heater(settings.ipa_heater_target)
        .with_high_voltage()
        .with_iso_pump()
        .with_lights(true)
        .with_glass_heater();

    // Est-ce qu'on veut vraiment check la stabilité ? Ou est-ce qu'on veut juste allumer le HV
    match history.is_temp_stable(CHAMBER_TEMP_IDX, STABLE_WINDOW, STABLE_TOLERANCE_C) {
        true  => (SystemTask::Cooling(CoolingPhase::FinalCheckBeforeStabilising), plan),
        false => (SystemTask::Cooling(CoolingPhase::HighVoltage), plan),
    }
}

fn final_check_before_stabilising(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let settings = settings::get();
    let plan = ActuatorPlan::all_off()
        .with_cooling(settings.saturation_target)
        .with_iso_heater(settings.ipa_heater_target)
        .with_high_voltage()
        .with_iso_pump()
        .with_lights(true)
        .with_glass_heater();

    // Qu'est-ce qu'on veut check ici ??
    match history.is_at_or_below(CHAMBER_TEMP_IDX, settings.saturation_target + FINAL_CHECK_MARGIN) {
        true => (SystemTask::Stabilising, plan),
        false => (SystemTask::Cooling(CoolingPhase::FinalCheckBeforeStabilising), plan),
    }
}
