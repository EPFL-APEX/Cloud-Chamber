//! Boucle de sécurité — structure alignée sur la branche
//! add-phase-transition-logic. Ici exécutée sur Core0 (dans la boucle de
//! contrôle); le passage sur Core1 est l'étape suivante prévue.

pub mod monitor;
pub mod safety;
