//! Écrans complets de l'interface utilisateur.
//!
//! Visibles dans tout `ui/` (donc depuis `ui::router`) mais pas au-delà :
//! `ui::router::Screens` est le seul point d'entrée public de la navigation.

// L'écran d'erreur échappe à la règle « visible seulement dans `ui/` » :
// c'est `main.rs` qui le dessine, depuis la boucle du cœur 0, quand le
// gestionnaire de panique a signalé la mort du cœur 1 — le routeur est
// déjà hors jeu à ce point (plus personne ne navigue).
pub mod error;
pub(super) mod data;
pub(super) mod lights;
pub(super) mod menu;
pub(super) mod running;
pub(super) mod settings;
pub(super) mod temp;
pub(super) mod info;
pub(super) mod stats;
pub(super) mod widgets;
