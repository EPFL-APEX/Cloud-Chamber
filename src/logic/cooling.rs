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
use crate::config::{
    IPA_HEATER_TARGET_C, PRECOOL_TARGET_C, SATURATION_TARGET_C, STABLE_TOLERANCE_C, STABLE_WINDOW_MS,
};
use crate::cloud_chamber_hal::actuators::ActuatorPlan;
use crate::logic::probing::{MeasurementHistory, ProbingPlan};
use crate::shared::data::SystemTask;

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
    let plan = ActuatorPlan { cooling: None, iso_heater: None, high_voltage: false };
    match history.temps[CHAMBER_TEMP_IDX].get(0) {
        Ok(m) if !m.value.0.is_nan() => (SystemTask::Cooling(CoolingPhase::PreCoolingThePlate), plan),
        _ => (SystemTask::Cooling(CoolingPhase::SensorCheck), plan),
    }
}

fn pre_cooling_the_plate(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let plan = ActuatorPlan {
        cooling: Some(Celsius(PRECOOL_TARGET_C)), iso_heater: None, high_voltage: false,
    };
    match history.temps[CHAMBER_TEMP_IDX].get(0) {
        Ok(m) if !m.value.0.is_nan() && m.value.0 <= PRECOOL_TARGET_C =>
            (SystemTask::Cooling(CoolingPhase::StartingIpaCirculation), plan),
        _ => (SystemTask::Cooling(CoolingPhase::PreCoolingThePlate), plan),
    }
}

fn starting_ipa_circulation(_history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    // Purement temporisé (pas de capteur dédié) — avancement décidé par
    // l'appelant (cf. `timed_transition` dans control_loop.rs).
    (SystemTask::Cooling(CoolingPhase::StartingIpaCirculation), ActuatorPlan {
        cooling: Some(Celsius(PRECOOL_TARGET_C)),
        iso_heater: Some(Celsius(IPA_HEATER_TARGET_C)),
        high_voltage: false,
    })
}

fn saturating_air_with_ipa(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let plan = ActuatorPlan {
        cooling: Some(Celsius(SATURATION_TARGET_C)),
        iso_heater: Some(Celsius(IPA_HEATER_TARGET_C)),
        high_voltage: false,
    };
    match history.temps[CHAMBER_TEMP_IDX].get(0) {
        Ok(m) if !m.value.0.is_nan() && m.value.0 <= SATURATION_TARGET_C =>
            (SystemTask::Cooling(CoolingPhase::HighVoltage), plan),
        _ => (SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa), plan),
    }
}

fn high_voltage(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let plan = ActuatorPlan {
        cooling: Some(Celsius(SATURATION_TARGET_C)),
        iso_heater: Some(Celsius(IPA_HEATER_TARGET_C)),
        high_voltage: true,
    };
    match history.temp_stable(CHAMBER_TEMP_IDX, STABLE_WINDOW_MS, STABLE_TOLERANCE_C) {
        true  => (SystemTask::Cooling(CoolingPhase::FinalCheckBeforeStabilising), plan),
        false => (SystemTask::Cooling(CoolingPhase::HighVoltage), plan),
    }
}

fn final_check_before_stabilising(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let plan = ActuatorPlan {
        cooling: Some(Celsius(SATURATION_TARGET_C)),
        iso_heater: Some(Celsius(IPA_HEATER_TARGET_C)),
        high_voltage: true,
    };
    match history.temps[CHAMBER_TEMP_IDX].get(0) {
        Ok(m) if !m.value.0.is_nan() && m.value.0 <= SATURATION_TARGET_C + 2.0 =>
            (SystemTask::Stabilising, plan),
        _ => (SystemTask::Cooling(CoolingPhase::FinalCheckBeforeStabilising), plan),
    }
}
