//! Séquence d'arrêt propre.
//!
//! Ordre physique : HV off → compresseur off → attendre l'équilibrage
//! pression (l'équilibrage ne peut pas se produire tant que le compresseur
//! tourne, il maintient le ΔP). Comme `cooling.rs`, chaque phase construit
//! son propre `ActuatorPlan` en même temps que sa transition ; les délais
//! fixes (décharge HV, settle compresseur, équilibrage) sont gérés par
//! l'appelant, pas ici.
//!
//! Pas de capteur dédié à l'équilibrage du circuit réfrigérant (l'unique
//! capteur de pression restant mesure la chambre, pas le circuit — cf.
//! `cloud_chamber_hal::config::CHAMBER_PRESSURE_IDX`) : `WaitPressureEquilibrium`
//! est donc purement temporisée, comme `StartingIpaCirculation` dans
//! `cooling.rs`. La transition vers `Idle` est gérée par l'appelant via le
//! timeout de `SystemTask::durations()` (`STOP_EQUALIZE_FALLBACK_MS`).

use crate::cloud_chamber_hal::units::Celsius;
use crate::config::SATURATION_TARGET_C;
use crate::cloud_chamber_hal::actuators::ActuatorPlan;
use crate::logic::probing::{MeasurementHistory, ProbingPlan};
use crate::shared::data::SystemTask;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoppingPhase {
    /// HV coupée immédiatement ; court délai de décharge géré par l'appelant.
    CutHighVoltage,
    /// Compresseur coupé.
    CutCompressor,
    /// Attente d'équilibrage pression — purement temporisée (pas de capteur
    /// dédié sur le circuit réfrigérant), avancement géré par l'appelant.
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

// `iso_pump`/`lights`/`glass_heater` : toujours `false` ci-dessous — aucune
// politique par phase définie pour l'instant, cf. doc de `ActuatorPlan`.

fn cut_high_voltage(_history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    // Délai de décharge géré par l'appelant (STOP_HV_SETTLE_MS) ; HT coupée
    // dès l'entrée en phase. Froid encore actif (l'IPA continue de
    // circuler pendant la décharge) ; chauffage IPA coupé.
    (SystemTask::Stopping(StoppingPhase::CutHighVoltage), ActuatorPlan {
        cooling: Some(Celsius(SATURATION_TARGET_C)), iso_heater: None, high_voltage: false,
        iso_pump: false, lights: None, glass_heater: false,
    })
}

fn cut_compressor(_history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    // Délai de settle géré par l'appelant (STOP_COMPRESSOR_SETTLE_MS) ;
    // compresseur coupé dès l'entrée en phase.
    (SystemTask::Stopping(StoppingPhase::CutCompressor), ActuatorPlan {
        cooling: None, iso_heater: None, high_voltage: false,
        iso_pump: false, lights: None, glass_heater: false,
    })
}

fn wait_pressure_equilibrium(_history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    // Purement temporisé (pas de capteur dédié sur le circuit réfrigérant) —
    // avancement vers Idle décidé par l'appelant (cf. `timed_transition`
    // dans `phase_clock.rs`, même mécanisme que `StartingIpaCirculation`).
    (SystemTask::Stopping(StoppingPhase::WaitPressureEquilibrium), ActuatorPlan {
        cooling: None, iso_heater: None, high_voltage: false,
        iso_pump: false, lights: None, glass_heater: false,
    })
}
