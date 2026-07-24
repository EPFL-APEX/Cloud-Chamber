//! Structures de données partagées entre Core0 et Core1.
//!
//! # Communication inter-cœurs sur RP2040/RP2350
//!
//! Les deux cœurs ARM partagent la même SRAM. Pour échanger des données
//! sans corruption, on utilise une **section critique** : pendant l'accès,
//! les interruptions sont désactivées sur le cœur courant, ce qui garantit
//! l'atomicité de la lecture ou de l'écriture.
//!
//! # Convergence des branches
//!
//! `SystemTask` vit ici (emplacement canonique de la branche équipe). Ses
//! transitions (`react_to`), codes et libellés sont implémentés dans
//! `logic/` — validés sur matériel (tests A–D).
//!
//! `SensorSnapshot`/`SharedState`/`SHARED` (types `Measurement<Unit>` de la
//! branche équipe) seront réactivés avec le module `cloud_chamber_hal`
//! (cf. lib.rs, plan de convergence) quand Core1 exécutera la SecurityLoop.
//! La version complète est conservée dans l'historique git (branche
//! merge-Kynan-Thomas, src/shared/data.rs). D'ici là, l'état capteurs Core0
//! vit dans `crate::data::SystemState`, inchangé.

use crate::logic::{cooling::CoolingPhase, stopping::StoppingPhase};

/// État global de la machine — mêmes variantes que la branche équipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemTask {
    /// Mode manuel : COMP/HV pilotés par l'opérateur.
    Idle,
    Cooling(CoolingPhase),
    Stabilising,
    Stopping(StoppingPhase),
}

impl Default for SystemTask {
    fn default() -> Self {
        SystemTask::Idle
    }
}
