//! Structures de données partagées entre Core0 et Core1.
//!
//! # Communication inter-cœurs sur RP2040/RP2350
//!
//! Les deux cœurs ARM partagent la même SRAM. Pour échanger des données
//! sans corruption, on utilise une **section critique** : pendant l'accès,
//! les interruptions sont désactivées sur le cœur courant, ce qui garantit
//! l'atomicité de la lecture ou de l'écriture.
//!
//! # Pattern `Mutex<RefCell<T>>`
//!
//! Ce pattern permet de muter un `static` en bare-metal :
//!
//! - [`critical_section::Mutex`] protège l'accès via des sections critiques.
//!   Sa méthode `borrow(cs)` retourne `&T`, valide uniquement pendant la
//!   section critique (le lifetime `'cs` le garantit au niveau des types).
//!
//! - [`core::cell::RefCell<T>`] ajoute la mutabilité intérieure : depuis une
//!   `&RefCell<T>`, on peut obtenir une `&mut T` via `borrow_mut()`.
//!   C'est nécessaire car Rust n'autorise pas `&mut T` depuis un `static`.

use core::cell::RefCell;
use critical_section::Mutex;
use heapless::Vec;

use crate::{
    cloud_chamber_hal::sensors::Measurement,
    cloud_chamber_hal::units::{Celsius, HectoPascal, Volt},
    config::{
        NUMBER_OF_PRESSURE_SENSOR, NUMBER_OF_TEMP_SENSOR, NUMBER_OF_VOLTMETER,
    }, logic::{
        cooling::CoolingPhase,
        stopping::StoppingPhase,
    }
};

/// Instantané des dernières mesures de tous les capteurs.
#[derive(Debug, Clone, Copy, Default)]
pub struct SensorSnapshot {
    /// Températures mesurées, indexées par numéro de capteur.
    pub temps: [Option<Measurement<Celsius>>; NUMBER_OF_TEMP_SENSOR],
    /// Pressions mesurées.
    pub press: [Option<Measurement<HectoPascal>>; NUMBER_OF_PRESSURE_SENSOR],
    /// Tensions mesurées.
    pub volts: [Option<Measurement<Volt>>; NUMBER_OF_VOLTMETER],
    /// `true` si la chambre est physiquement fermée (capteur de fermeture).
    pub is_closed: bool,
}

impl SensorSnapshot {
    pub fn are_all_none(&self) -> bool {
        self.temps.iter().all(Option::is_none)
            && self.press.iter().all(Option::is_none)
            && self.volts.iter().all(Option::is_none)
    }

    pub fn are_all_some(&self) -> bool {
        self.temps.iter().all(Option::is_some)
            && self.press.iter().all(Option::is_some)
            && self.volts.iter().all(Option::is_some)
    }
}

/// État global de la machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemTask {
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

// ============================================================
// Alarmes
// ============================================================
#[derive(Clone, Copy, Debug)]
pub enum AlarmLevel { Info, Warning, Critical }

#[derive(Clone, Debug)]
pub struct Alarm {
    pub level:       AlarmLevel,
    pub source:      &'static str,
    pub message:     &'static str,
    pub timestamp_s: u64,
}

const MAX_ERRORS: usize = 16;

/// Données échangées entre Core1 (producteur) et Core0 (consommateur).
pub struct SharedState {
    pub snapshot: SensorSnapshot,
    pub task: SystemTask,
    /// Mis à `true` par Core1 quand de nouvelles données sont disponibles.
    pub new_data: bool,
    pub alarms: Vec<Alarm, MAX_ERRORS>,
}

impl SharedState {
    pub fn push_alarm(&mut self, alarm: Alarm) -> Result<(), Alarm> {
        self.alarms.push(alarm)
    }

    pub fn clear_alarms(&mut self, level: AlarmLevel) {
        self.alarms.retain(|a| !matches!(
            (&a.level, &level),
            (AlarmLevel::Info,     AlarmLevel::Info)     |
            (AlarmLevel::Warning,  AlarmLevel::Warning)  |
            (AlarmLevel::Critical, AlarmLevel::Critical)
        ));
    }


}

// ─── Point de partage global ─────────────────────────────────────────────────
/// Static partagé entre Core0 et Core1.
///
/// Toujours accéder via `critical_section::with(|cs| { SHARED.borrow(cs)... })`.
pub static SHARED_STATE: Mutex<RefCell<SharedState>> = Mutex::new(RefCell::new(SharedState {
    snapshot: SensorSnapshot {
            temps: [None; NUMBER_OF_TEMP_SENSOR],
            press: [None; NUMBER_OF_PRESSURE_SENSOR],
            volts: [None; NUMBER_OF_VOLTMETER],
            is_closed: false
    },
    task: SystemTask::Idle,
    new_data: false,
    alarms: Vec::new(),
}));


// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_task_default_is_idle() {
        assert_eq!(SystemTask::default(), SystemTask::Idle);
    }

    #[test]
    fn sensor_snapshot_default_is_none() {
        let s = SensorSnapshot::default();
        for &t in &s.temps { assert!(t.is_none()); }
        for &p in &s.press { assert!(p.is_none()); }
        for &v in &s.volts { assert!(v.is_none()); }
        assert!(!s.is_closed);
    }

    #[test]
    fn system_state_variants_are_distinct() {
        assert_ne!(SystemTask::Idle, SystemTask::Stabilising);
        assert_ne!(SystemTask::Idle, SystemTask::Cooling(CoolingPhase::Todo));
        assert_ne!(
            SystemTask::Cooling(CoolingPhase::SensorCheck),
            SystemTask::Stopping(StoppingPhase::Todo)
        );
    }

    #[test]
    fn snapshot_is_copy() {
        let a = SensorSnapshot::default();
        let b = a; // Copy — `a` reste valide après cette ligne
        assert_eq!(a.is_closed, b.is_closed);
    }
}
