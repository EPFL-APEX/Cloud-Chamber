//! Module partagé entre Core0 (UI/logging) et Core1 (boucle de sécurité).
//!
//! # Structure des modules en Rust
//!
//! En Rust, un module dans un répertoire `shared/` a besoin d'un fichier
//! `shared/mod.rs` pour servir de racine du module. Ce fichier déclare
//! les sous-modules avec `pub mod <nom>`.
//!
//! `pub mod` rend le sous-module accessible depuis l'extérieur du module parent.
//! Sans `pub`, le module serait privé (utilisable uniquement dans ce module).

/// Structures de données échangées entre les deux cœurs.
pub mod data;

/// Types d'erreurs génériques réutilisés dans tout le projet.
pub mod error;

/// Buffer circulaire générique pour l'historique des mesures.
pub mod ring_buffer;



use crate::config::{
    NUMBER_OF_TEMP_SENSOR,
    NUMBER_OF_PRESSURE_SENSOR,
    NUMBER_OF_VOLTMETER,
};

use data::{SharedState, SensorSnapshot, SystemTask};

use core::cell::RefCell;
use critical_section::Mutex;

// ─── Point de partage global ─────────────────────────────────────────────────
/// Static partagé entre Core0 et Core1.
///
/// Toujours accédé via `critical_section::with(|cs| { SHARED.borrow(cs)... })`.
pub static SHARED: Mutex<RefCell<SharedState>> = Mutex::new(RefCell::new(SharedState {
    snapshot: SensorSnapshot { 
            temps: [None; NUMBER_OF_TEMP_SENSOR],
            press: [None; NUMBER_OF_PRESSURE_SENSOR],
            volts: [None; NUMBER_OF_VOLTMETER],
            is_closed: false 
    },
    system_state: SystemTask::Idle,
    new_data: false,
}));