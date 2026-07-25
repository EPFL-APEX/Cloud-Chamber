//! Contexte de phase et transitions de la machine à états.
//!
//! Ce fichier contient la logique qui vivait auparavant dans `logic/mod.rs`.
//! Déplacée suite à la review de la PR #20 : un `mod.rs` ne doit contenir que
//! des déclarations et ré-exports de modules, jamais de logique ni de type.
//!
//! L'enum `SystemTask` lui-même est défini dans `shared::data` (son
//! emplacement canonique côté branche équipe) ; seules ses transitions et ses
//! libellés sont implémentés ici.

use crate::data::SystemState;
use crate::shared::data::SystemTask;

use super::cooling::CoolingPhase;
use super::history::MeasurementHistory;
use super::stopping::StoppingPhase;

/// Contexte passé aux fonctions de transition des phases.
pub struct PhaseContext<'a> {
    pub state:      &'a SystemState,
    pub history:    &'a MeasurementHistory,
    pub now_ms:     u64,
    /// Temps écoulé depuis l'entrée dans la phase courante.
    pub elapsed_ms: u64,
}

impl SystemTask {
    /// Évalue les conditions de transition et retourne la tâche suivante
    /// (`self` si on reste dans la phase courante).
    pub fn react_to(self, ctx: &PhaseContext) -> SystemTask {
        match self {
            SystemTask::Idle        => SystemTask::Idle, // sortie via CYCLE 1 uniquement
            SystemTask::Cooling(p)  => p.react_to(ctx),
            SystemTask::Stabilising => SystemTask::Stabilising, // sortie via CYCLE 0 / sécurité
            SystemTask::Stopping(p) => p.react_to(ctx),
        }
    }

    /// Code numérique publié dans STATE (`ph=`).
    pub fn code(&self) -> u8 {
        match self {
            SystemTask::Idle => 0,
            SystemTask::Cooling(CoolingPhase::SensorCheck)                 => 1,
            SystemTask::Cooling(CoolingPhase::PreCoolingThePlate)          => 2,
            SystemTask::Cooling(CoolingPhase::StartingIpaCirculation)      => 3,
            SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa)        => 4,
            SystemTask::Cooling(CoolingPhase::HighVoltage)                 => 5,
            SystemTask::Cooling(CoolingPhase::FinalCheckBeforeStabilising) => 6,
            SystemTask::Stabilising => 7,
            SystemTask::Stopping(StoppingPhase::CutHighVoltage)          => 8,
            SystemTask::Stopping(StoppingPhase::CutCompressor)           => 9,
            SystemTask::Stopping(StoppingPhase::WaitPressureEquilibrium) => 10,
        }
    }

    /// Libellé 14 caractères pour le TFT (None en Idle → affichage historique).
    pub fn label(&self) -> Option<&'static str> {
        match self {
            SystemTask::Idle => None,
            SystemTask::Cooling(CoolingPhase::SensorCheck)                 => Some("CHECK CAPTEURS"),
            SystemTask::Cooling(CoolingPhase::PreCoolingThePlate)          => Some("PRE-REFROIDIS."),
            SystemTask::Cooling(CoolingPhase::StartingIpaCirculation)      => Some("CIRCUL. IPA   "),
            SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa)        => Some("SATURATION IPA"),
            SystemTask::Cooling(CoolingPhase::HighVoltage)                 => Some("HAUTE TENSION "),
            SystemTask::Cooling(CoolingPhase::FinalCheckBeforeStabilising) => Some("VERIF. FINALE "),
            SystemTask::Stabilising => Some("STABILISE     "),
            SystemTask::Stopping(StoppingPhase::CutHighVoltage)          => Some("ARRET: HV OFF "),
            SystemTask::Stopping(StoppingPhase::CutCompressor)           => Some("ARRET: COMP.  "),
            SystemTask::Stopping(StoppingPhase::WaitPressureEquilibrium) => Some("ARRET: EQUIL. "),
        }
    }
}
