//! Machine à états de la chambre — arborescence alignée sur la branche
//! équipe. L'enum `SystemTask` est défini dans `shared::data` (son
//! emplacement canonique chez l'équipe) et ré-exporté ici; ses transitions
//! et libellés sont implémentés dans ce module.

pub mod cooling;
pub mod history;
pub mod stopping;

// Fichiers de la branche équipe conservés mais pas encore compilés :
// ils dépendent des traits cloud_chamber_hal (cf. lib.rs, plan de
// convergence). Leur logique de sonde (ProbingPlan) sera branchée sur la
// boucle d'acquisition réelle à l'étape « traits HAL ».
// pub mod control_loop;
// pub mod probing;

use crate::data::SystemState;
use cooling::CoolingPhase;
use history::MeasurementHistory;
use stopping::StoppingPhase;

pub use crate::shared::data::SystemTask;

/// Contexte passé aux fonctions de transition des phases.
pub struct PhaseCtx<'a> {
    pub state:      &'a SystemState,
    pub hist:       &'a MeasurementHistory,
    pub now_ms:     u64,
    /// Temps écoulé depuis l'entrée dans la phase courante.
    pub elapsed_ms: u64,
}

impl SystemTask {
    /// Évalue les conditions de transition et retourne la tâche suivante
    /// (`self` si on reste dans la phase courante).
    pub fn react_to(self, ctx: &PhaseCtx) -> SystemTask {
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
