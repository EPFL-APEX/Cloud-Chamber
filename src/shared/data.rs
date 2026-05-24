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

// ─── Structures capteurs externes ────────────────────────────────────────────

/// Lecture d'un capteur de température DS18B20 ou BME280.
#[derive(Clone, Copy, Debug, Default)]
pub struct TemperatureReading {
    pub value:    f32,
    pub valid:    bool,
    /// `true` pour les capteurs dont le dépassement doit déclencher une alarme.
    pub critical: bool,
}

/// Lecture d'un capteur de pression ABP2.
#[derive(Clone, Copy, Debug, Default)]
pub struct PressureReading {
    pub pressure:    f32,
    pub temperature: f32,
    pub valid:       bool,
}

// ─── Constantes de configuration ─────────────────────────────────────────────

/// Nombre de capteurs de température dans le système.
pub const NUMBER_OF_TEMPS: usize = 5;

/// Nombre de capteurs de tension.
pub const NUMBER_OF_VOLT: usize = 3;

/// Nombre de capteurs de courant (ampèremètres).
pub const NUMBER_OF_AMP: usize = 1;

/// Profondeur maximale de la pile de navigation de l'interface utilisateur.
pub const NAV_STACK_DEPTH: usize = 8;

// ─── Structures de données ────────────────────────────────────────────────────

/// Instantané des dernières mesures de tous les capteurs.
///
/// # Pourquoi `Copy` ?
///
/// Le trait `Copy` en Rust signifie qu'une valeur est dupliquée bit-à-bit
/// lors d'une assignation. C'est possible uniquement si tous les champs sont
/// eux-mêmes `Copy` (`f32`, `bool`, et les tableaux de types `Copy` le sont).
#[derive(Debug, Clone, Copy)]
pub struct SensorSnapshot {
    /// Températures mesurées, en degrés Celsius, indexées par numéro de capteur.
    pub temps: [f32; NUMBER_OF_TEMPS],
    /// Tensions mesurées, en Volts.
    pub volts: [f32; NUMBER_OF_VOLT],
    /// Courants mesurés, en Ampères.
    pub amps: [f32; NUMBER_OF_AMP],
    /// `true` si la chambre est physiquement fermée (capteur de fermeture).
    pub is_closed: bool,
}

impl Default for SensorSnapshot {
    fn default() -> Self {
        Self {
            temps: [0.0; NUMBER_OF_TEMPS],
            volts: [0.0; NUMBER_OF_VOLT],
            amps: [0.0; NUMBER_OF_AMP],
            is_closed: false,
        }
    }
}

/// État global du système de sécurité.
///
/// # Pourquoi un `enum` ?
///
/// Les `enum` Rust sont des **types somme** : une valeur de type `SystemState`
/// ne peut être qu'une seule variante à la fois. Le compilateur oblige à gérer
/// tous les cas dans un `match` — impossible d'oublier un état.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemState {
    /// Fonctionnement normal, tous les capteurs dans les limites.
    Normal,
    /// Une valeur approche d'un seuil critique. Avertissement visuel.
    Warning,
    /// Un seuil critique est dépassé. Action corrective requise.
    Alarm,
    /// Situation d'urgence : le disjoncteur a été déclenché.
    Emergency,
}

impl Default for SystemState {
    fn default() -> Self {
        SystemState::Normal
    }
}

/// Données échangées entre Core1 (producteur) et Core0 (consommateur).
pub struct SharedState {
    pub snapshot: SensorSnapshot,
    pub system_state: SystemState,
    /// Mis à `true` par Core1 quand de nouvelles données sont disponibles.
    pub new_data: bool,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            snapshot: SensorSnapshot::default(),
            system_state: SystemState::default(),
            new_data: false,
        }
    }
}

// ─── Point de partage global ─────────────────────────────────────────────────

/// Static partagé entre Core0 et Core1.
///
/// Toujours accédé via `critical_section::with(|cs| { SHARED.borrow(cs)... })`.
pub static SHARED: Mutex<RefCell<SharedState>> = Mutex::new(RefCell::new(SharedState {
    snapshot: SensorSnapshot {
        temps: [0.0; NUMBER_OF_TEMPS],
        volts: [0.0; NUMBER_OF_VOLT],
        amps: [0.0; NUMBER_OF_AMP],
        is_closed: false,
    },
    system_state: SystemState::Normal,
    new_data: false,
}));

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_state_default_is_normal() {
        assert_eq!(SystemState::default(), SystemState::Normal);
    }

    #[test]
    fn sensor_snapshot_default_zeroed() {
        let s = SensorSnapshot::default();
        for &t in &s.temps { assert_eq!(t, 0.0f32); }
        for &v in &s.volts { assert_eq!(v, 0.0f32); }
        assert!(!s.is_closed);
    }

    #[test]
    fn system_state_variants_are_distinct() {
        assert_ne!(SystemState::Normal, SystemState::Warning);
        assert_ne!(SystemState::Warning, SystemState::Alarm);
        assert_ne!(SystemState::Alarm, SystemState::Emergency);
    }

    #[test]
    fn shared_state_default_has_no_new_data() {
        let s = SharedState::default();
        assert_eq!(s.system_state, SystemState::Normal);
        assert!(!s.new_data);
    }

    #[test]
    fn snapshot_is_copy() {
        let a = SensorSnapshot::default();
        let b = a; // Copy — `a` reste valide après cette ligne
        assert_eq!(a.is_closed, b.is_closed);
    }
}
