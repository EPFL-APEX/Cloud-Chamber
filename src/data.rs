/// Structures de données partagées entre les deux cœurs.
///
/// Core 0 écrit les lectures capteurs et les alarmes.
/// Core 1 lit ces données pour le serveur HTTP, le logging et l'UI.
///
/// La synchronisation se fait via `embassy_sync::mutex::Mutex`
/// qui est safe pour le multicore sur RP2040.

use embassy_sync::mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use heapless::Vec;

/// Nombre maximum de capteurs de température
const MAX_TEMP_SENSORS: usize = 5;
/// Nombre maximum d'erreurs conservées en mémoire
const MAX_ERRORS: usize = 16;

// ============================================================
// Lecture d'un capteur de température
// ============================================================
#[derive(Clone, Copy, Debug, Default)]
pub struct TemperatureReading {
    /// Température en °C (f32::NAN si invalide)
    pub value: f32,
    /// true si la dernière lecture est valide
    pub valid: bool,
    /// true si ce capteur est critique (sécurité)
    pub critical: bool,
}

// ============================================================
// Lecture d'un capteur de pression ABP2
// ============================================================
#[derive(Clone, Copy, Debug, Default)]
pub struct PressureReading {
    /// Pression dans l'unité configurée (bar)
    pub pressure: f32,
    /// Température interne du capteur ABP2 (°C)
    pub temperature: f32,
    /// true si la dernière lecture est valide
    pub valid: bool,
}

// ============================================================
// Alarme / Erreur système
// ============================================================
#[derive(Clone, Copy, Debug)]
pub enum AlarmLevel {
    /// Information (pas d'action requise)
    Info,
    /// Attention (situation anormale mais pas dangereuse)
    Warning,
    /// Critique (action immédiate : coupure compresseur)
    Critical,
}

#[derive(Clone, Debug)]
pub struct Alarm {
    pub level: AlarmLevel,
    pub source: &'static str,
    pub message: &'static str,
    /// Timestamp approximatif (secondes depuis le boot)
    pub timestamp_s: u64,
}

// ============================================================
// État complet du système (partagé entre les deux cœurs)
// ============================================================
#[derive(Debug)]
pub struct SystemState {
    /// Lectures des 5 capteurs de température
    pub temperatures: [TemperatureReading; MAX_TEMP_SENSORS],
    /// Lecture capteur pression basse (BP)
    pub pressure_bp: PressureReading,
    /// Lecture capteur pression haute (HP)
    pub pressure_hp: PressureReading,
    /// Le compresseur est-il autorisé à fonctionner ?
    pub compressor_allowed: bool,
    /// Liste des alarmes actives
    pub alarms: Vec<Alarm, MAX_ERRORS>,
    /// Compteur de cycles du flow controller (Core 0)
    pub cycle_count: u64,
    /// Uptime en secondes
    pub uptime_s: u64,
}

impl Default for SystemState {
    fn default() -> Self {
        Self {
            temperatures: [TemperatureReading::default(); MAX_TEMP_SENSORS],
            pressure_bp: PressureReading::default(),
            pressure_hp: PressureReading::default(),
            compressor_allowed: true,
            alarms: Vec::new(),
            cycle_count: 0,
            uptime_s: 0,
        }
    }
}

impl SystemState {
    /// Ajoute une alarme (supprime la plus ancienne si plein)
    pub fn push_alarm(&mut self, level: AlarmLevel, source: &'static str, message: &'static str, timestamp_s: u64) {
        if self.alarms.is_full() {
            // Supprimer la plus ancienne
            self.alarms.remove(0);
        }
        let _ = self.alarms.push(Alarm {
            level,
            source,
            message,
            timestamp_s,
        });
    }

    /// Efface les alarmes d'un niveau donné
    pub fn clear_alarms(&mut self, level: AlarmLevel) {
        self.alarms.retain(|a| !matches!((&a.level, &level),
            (AlarmLevel::Info, AlarmLevel::Info) |
            (AlarmLevel::Warning, AlarmLevel::Warning) |
            (AlarmLevel::Critical, AlarmLevel::Critical)
        ));
    }
}

// ============================================================
// Mutex globale pour l'accès inter-cœurs
// ============================================================
/// État partagé protégé par un mutex.
/// Core 0 (flow controller) acquiert le lock pour écrire.
/// Core 1 (réseau/UI) acquiert le lock pour lire.
pub static SHARED_STATE: Mutex<CriticalSectionRawMutex, SystemState> =
    Mutex::new(SystemState {
        temperatures: [TemperatureReading {
            value: 0.0,
            valid: false,
            critical: false,
        }; MAX_TEMP_SENSORS],
        pressure_bp: PressureReading {
            pressure: 0.0,
            temperature: 0.0,
            valid: false,
        },
        pressure_hp: PressureReading {
            pressure: 0.0,
            temperature: 0.0,
            valid: false,
        },
        compressor_allowed: true,
        alarms: Vec::new(),
        cycle_count: 0,
        uptime_s: 0,
    });
