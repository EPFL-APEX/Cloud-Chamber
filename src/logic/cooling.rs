//! Séquence de démarrage (refroidissement).
//!
//! Chaque phase ne réagit qu'aux mesures disponibles dans l'historique et
//! construit son propre `ActuatorPlan` en même temps que sa transition — une
//! seule décision par phase, pas de table séparée ailleurs qui pourrait
//! diverger. Aucune notion de durée ici : les délais (attente minimale,
//! abandon si trop long) sont du ressort de l'appelant (`control_loop.rs`),
//! qui seul connaît la durée passée dans la phase courante.

use crate::cloud_chamber_hal::config::CHAMBER_TEMP_IDX;
use crate::config::operating::{STABLE_TOLERANCE_C, STABLE_WINDOW};
use crate::cloud_chamber_hal::actuators::ActuatorPlan;
use crate::logic::probing::{MeasurementHistory, ProbingPlan};
use crate::shared::data::SystemTask;
use crate::shared::settings;

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
    let plan = ActuatorPlan {
        cooling: None, iso_heater: None, high_voltage: false,
        iso_pump: false, lights: None, glass_heater: false,
    };

    // Ajouter le check des autres sensors ?
    // #todo
    match history.temps[CHAMBER_TEMP_IDX].get(0) {
        Ok(m) if !m.value.0.is_nan() => (SystemTask::Cooling(CoolingPhase::PreCoolingThePlate), plan),
        _ => (SystemTask::Cooling(CoolingPhase::SensorCheck), plan),
    }
}

fn pre_cooling_the_plate(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let precool_target = settings::get().precool_target;
    let plan = ActuatorPlan {
        cooling: Some(precool_target), iso_heater: None, high_voltage: false,
        iso_pump: false, lights: None, glass_heater: false,
    };
    match history.temps[CHAMBER_TEMP_IDX].get(0) {
        Ok(m) if !m.value.0.is_nan() && m.value.0 <= precool_target.0 =>
            (SystemTask::Cooling(CoolingPhase::StartingIpaCirculation), plan),
        _ => (SystemTask::Cooling(CoolingPhase::PreCoolingThePlate), plan),
    }
}

fn starting_ipa_circulation(_history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    // Purement temporisé (pas de capteur dédié) — avancement décidé par
    // l'appelant (cf. `timed_transition` dans control_loop.rs).
    let settings = settings::get();
    (SystemTask::Cooling(CoolingPhase::StartingIpaCirculation), ActuatorPlan {
        cooling: Some(settings.precool_target),
        iso_heater: Some(settings.ipa_heater_target),
        high_voltage: false,
        iso_pump: true, lights: None, glass_heater: true,
    })
}

fn saturating_air_with_ipa(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let settings = settings::get();
    let plan = ActuatorPlan {
        cooling: Some(settings.saturation_target),
        iso_heater: Some(settings.ipa_heater_target),
        high_voltage: false,
        iso_pump: true, lights: None, glass_heater: true,
    };
    // #todo faire une vrai estimation de la saturation....
    match history.temps[CHAMBER_TEMP_IDX].get(0) {
        Ok(m) if !m.value.0.is_nan() && m.value.0 <= settings.saturation_target.0 =>
            (SystemTask::Cooling(CoolingPhase::HighVoltage), plan),
        _ => (SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa), plan),
    }
}

fn high_voltage(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let settings = settings::get();
    let plan = ActuatorPlan {
        cooling: Some(settings.saturation_target),
        iso_heater: Some(settings.ipa_heater_target),
        high_voltage: true,
        iso_pump: true, lights: Some(true), glass_heater: true,
    };

    // Est-ce qu'on veut vraiment check la stabilité ? Ou est-ce qu'on veut juste allumer le HV
    match history.is_temp_stable(CHAMBER_TEMP_IDX, STABLE_WINDOW, STABLE_TOLERANCE_C) {
        true  => (SystemTask::Cooling(CoolingPhase::FinalCheckBeforeStabilising), plan),
        false => (SystemTask::Cooling(CoolingPhase::HighVoltage), plan),
    }
}

fn final_check_before_stabilising(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let settings = settings::get();
    let plan = ActuatorPlan {
        cooling: Some(settings.saturation_target),
        iso_heater: Some(settings.ipa_heater_target),
        high_voltage: true,
        iso_pump: true, lights: Some(true), glass_heater: true,
    };

    // Qu'est-ce qu'on veut check ici ??
    match history.temps[CHAMBER_TEMP_IDX].get(0) {
        Ok(m) if !m.value.0.is_nan() && m.value.0 <= settings.saturation_target.0 + 2.0 =>
            (SystemTask::Stabilising, plan),
        _ => (SystemTask::Cooling(CoolingPhase::FinalCheckBeforeStabilising), plan),
    }
}
