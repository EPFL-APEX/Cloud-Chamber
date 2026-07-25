//! Machine à états de la chambre — arborescence alignée sur la branche
//! équipe.
//!
//! Ce module ne contient que des déclarations et des ré-exports (review
//! PR #20) : la logique de transition vit dans `task.rs`, les phases dans
//! `cooling.rs` / `stopping.rs`, l'historique dans `history.rs`.

pub mod cooling;
pub mod history;
pub mod stopping;
pub mod task;

// Fichiers de la branche équipe conservés mais pas encore compilés :
// ils dépendent des traits cloud_chamber_hal (cf. lib.rs, plan de
// convergence). Leur logique de sonde (ProbingPlan) sera branchée sur la
// boucle d'acquisition réelle à l'étape « traits HAL ».
// pub mod control_loop;
// pub mod probing;

pub use crate::shared::data::SystemTask;
pub use task::PhaseContext;
