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
    config::{
        NUMBER_OF_TEMP_SENSOR, NUMBER_OF_PRESSURE_SENSOR,
        NUMBER_OF_AMPMETER, NUMBER_OF_VOLTMETER,
    },
    logic::{
        cooling::CoolingPhase,
        stopping::StoppingPhase,
    },
};


/// Lecture d'un capteur de température DS18B20 ou BME280.
#[derive(Clone, Copy, Debug)]
pub struct TemperatureReading {
    pub time: f32,
    pub value: f32,
}

/// Lecture d'un capteur de pression ABP2.
#[derive(Clone, Copy, Debug)]
pub struct PressureReading {
    pub time: f32,
    pub value: f32,
}

/// Lecture d'un voltmètre
pub struct VoltsReading {
    pub time: f32,
    pub value: f32,
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
    IDLE,
    COOLING(CoolingPhase),
    STABILISING,
    STOPPING(StoppingPhase),
}

impl Default for SystemTask {
    fn default() -> Self {
        SystemTask::IDLE
    }
}

/// Données échangées entre Core1 (producteur) et Core0 (consommateur).
pub struct SharedState {
    pub snapshot: SensorSnapshot,
    pub system_state: SystemTask,
    /// Mis à `true` par Core1 quand de nouvelles données sont disponibles.
    pub new_data: bool,
}


// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_task_default_is_idle() {
        assert_eq!(SystemTask::default(), SystemTask::IDLE);
    }

    #[test]
    fn sensor_snapshot_default_is_none() {
        let s = SensorSnapshot::default();
        for &t in &s.temps { assert_eq!(t, None); }
        for &p in &s.press { assert_eq!(p, None); }
        for &v in &s.volts { assert_eq!(v, None); }
        assert!(!s.is_closed);
    }

    #[test]
    fn system_state_variants_are_distinct() {
        assert_ne!(SystemTask::IDLE, SystemTask::STABILISING);
        assert_ne!(SystemTask::IDLE , SystemTask::COOLING(cp));
        assert_ne!(SystemTask::COOLING(cp), SystemTask::STOPPING(sp));
    }

    #[test]
    fn snapshot_is_copy() {
        let a = SensorSnapshot::default();
        let b = a; // Copy — `a` reste valide après cette ligne
        assert_eq!(a.is_closed, b.is_closed);
    }
}
