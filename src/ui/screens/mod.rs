//! Écrans complets de l'interface utilisateur.
//!
//! Visibles dans tout `ui/` (donc depuis `ui::router`) mais pas au-delà :
//! `ui::router::Screens` est le seul point d'entrée public de la navigation.

pub(super) mod menu;
pub(super) mod running;
pub(super) mod settings;
pub(super) mod temp;
pub(super) mod stats;
pub(super) mod widgets;
