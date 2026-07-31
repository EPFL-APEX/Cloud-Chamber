//! Séquence d'arrêt propre.
//!
//! Ordre physique : HV off → compresseur off → attendre l'équilibrage HP
//! (l'équilibrage ne peut pas se produire tant que le compresseur tourne,
//! il maintient le ΔP). Comme `cooling.rs`, chaque phase construit son
//! propre `ActuatorPlan` en même temps que sa transition ; les délais fixes
//! (décharge HV, settle compresseur, équilibrage sans capteur) sont gérés
//! par l'appelant, pas ici.

use crate::cloud_chamber_hal::config::HP_PRESSURE_IDX;
use crate::config::STOP_EQUALIZE_HP_MAX;
use crate::cloud_chamber_hal::actuators::ActuatorPlan;
use crate::logic::probing::{MeasurementHistory, ProbingPlan};
use crate::shared::data::SystemTask;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoppingPhase {
    /// HV coupée immédiatement ; court délai de décharge géré par l'appelant.
    CutHighVoltage,
    /// Compresseur coupé.
    CutCompressor,
    /// Attente d'équilibrage HP (capteur si présent, sinon temporisation côté appelant).
    WaitPressureEquilibrium,
}

impl StoppingPhase {
    /// Sonde tout à chaque cycle — cf. commentaire équivalent dans
    /// `logic::cooling::CoolingPhase::create_probing_plan`.
    pub fn create_probing_plan(&self, _prob_hist: &MeasurementHistory) -> ProbingPlan {
        ProbingPlan::all()
    }

    pub fn react_to(self, history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
        use StoppingPhase::*;
        match self {
            CutHighVoltage          => cut_high_voltage(history),
            CutCompressor           => cut_compressor(history),
            WaitPressureEquilibrium => wait_pressure_equilibrium(history),
        }
    }
}

fn cut_high_voltage(_history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    // Délai de décharge géré par l'appelant (STOP_HV_SETTLE_MS) ; HT coupée
    // dès l'entrée en phase.
    (SystemTask::Stopping(StoppingPhase::CutHighVoltage),
     ActuatorPlan { compressor: true, iso_heater: false, high_voltage: false })
}

fn cut_compressor(_history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    // Délai de settle géré par l'appelant (STOP_COMPRESSOR_SETTLE_MS) ;
    // compresseur coupé dès l'entrée en phase.
    (SystemTask::Stopping(StoppingPhase::CutCompressor),
     ActuatorPlan { compressor: false, iso_heater: false, high_voltage: false })
}

fn wait_pressure_equilibrium(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let plan = ActuatorPlan { compressor: false, iso_heater: false, high_voltage: false };
    match history.press[HP_PRESSURE_IDX].get(0) {
        Ok(m) if !m.value.0.is_nan() && m.value.0 < STOP_EQUALIZE_HP_MAX => (SystemTask::Idle, plan),
        // Pas de capteur / pas encore équilibré → l'appelant tranche par
        // timeout (STOP_EQUALIZE_FALLBACK_MS).
        _ => (SystemTask::Stopping(StoppingPhase::WaitPressureEquilibrium), plan),
    }
}
