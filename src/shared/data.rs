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

use crate::{
    cloud_chamber_hal::timer::Instant, config::{
        NUMBER_OF_PRESSURE_SENSOR, NUMBER_OF_TEMP_SENSOR, NUMBER_OF_VOLTMETER,
    }, logic::{
        cooling::CoolingPhase,
        stopping::StoppingPhase,
    }
};

pub trait TimeStamped {
    fn get_instant(&self) -> &Instant;
}


/// Lecture d'un capteur de température DS18B20 ou BME280.
#[derive(Clone, Copy, Debug)]
pub struct TemperatureReading {
    pub time: Instant,
    pub value: f32,
}

impl TimeStamped for TemperatureReading {
    fn get_instant(&self) -> &Instant {
        &self.time
    }
}

/// Lecture d'un capteur de pression ABP2.
#[derive(Clone, Copy, Debug)]
pub struct PressureReading {
    pub time: Instant,
    pub value: f32,
}

impl TimeStamped for PressureReading {
    fn get_instant(&self) -> &Instant {
        &self.time
    }
}

/// Lecture d'un voltmètre
#[derive(Clone, Copy, Debug)]
pub struct VoltsReading {
    pub time: Instant,
    pub value: f32,
}

impl TimeStamped for VoltsReading {
    fn get_instant(&self) -> &Instant {
        &self.time
    }
}

/// Instantané des dernières mesures de tous les capteurs.
#[derive(Debug, Clone, Copy, Default)]
pub struct SensorSnapshot {
    /// Températures mesurées, en degrés Celsius, indexées par numéro de capteur.
    pub temps: [Option<TemperatureReading>; NUMBER_OF_TEMP_SENSOR],
    /// Pressions meseurées,
    pub press: [Option<PressureReading>; NUMBER_OF_PRESSURE_SENSOR],
    /// Tensions mesurées, en Volts.
    pub volts: [Option<VoltsReading>; NUMBER_OF_VOLTMETER],
    /// `true` si la chambre est physiquement fermée (capteur de fermeture).
    pub is_closed: bool,
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

/// Données échangées entre Core1 (producteur) et Core0 (consommateur).
pub struct SharedState {
    pub snapshot: SensorSnapshot,
    pub system_state: SystemTask,
    /// Mis à `true` par Core1 quand de nouvelles données sont disponibles.
    pub new_data: bool,
}

// ─── Point de partage global ─────────────────────────────────────────────────
/// Static partagé entre Core0 et Core1.
///
/// Toujours accéder via `critical_section::with(|cs| { SHARED.borrow(cs)... })`.
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
            SystemTask::Cooling(CoolingPhase::Todo),
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
