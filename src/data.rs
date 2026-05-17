/// Structures de données partagées entre les deux cœurs.
///
/// Synchronisation via `critical_section::Mutex<RefCell<T>>`
/// (stable Rust, pas d'Embassy — identique au projet partenaire).

use critical_section::Mutex;
use core::cell::RefCell;
use heapless::Vec;

pub const MAX_TEMP_SENSORS: usize = 5;
const MAX_ERRORS: usize = 16;

// ============================================================
// Lecture température
// ============================================================
#[derive(Clone, Copy, Debug, Default)]
pub struct TemperatureReading {
    pub value:    f32,
    pub valid:    bool,
    pub critical: bool,
}

// ============================================================
// Lecture pression ABP2
// ============================================================
#[derive(Clone, Copy, Debug, Default)]
pub struct PressureReading {
    pub pressure:    f32,
    pub temperature: f32,
    pub valid:       bool,
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

// ============================================================
// État système
// ============================================================
#[derive(Debug)]
pub struct SystemState {
    pub temperatures:       [TemperatureReading; MAX_TEMP_SENSORS],
    pub pressure_bp:        PressureReading,
    pub pressure_hp:        PressureReading,
    pub compressor_allowed: bool,
    pub alarms:             Vec<Alarm, MAX_ERRORS>,
    pub cycle_count:        u64,
    pub uptime_s:           u64,
}

impl SystemState {
    pub const fn new() -> Self {
        Self {
            temperatures: [TemperatureReading { value: 0.0, valid: false, critical: false };
                MAX_TEMP_SENSORS],
            pressure_bp:        PressureReading { pressure: 0.0, temperature: 0.0, valid: false },
            pressure_hp:        PressureReading { pressure: 0.0, temperature: 0.0, valid: false },
            compressor_allowed: true,
            alarms:             Vec::new(),
            cycle_count:        0,
            uptime_s:           0,
        }
    }

    pub fn push_alarm(&mut self, level: AlarmLevel, source: &'static str,
                      message: &'static str, timestamp_s: u64) {
        if self.alarms.is_full() { self.alarms.remove(0); }
        let _ = self.alarms.push(Alarm { level, source, message, timestamp_s });
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

// ============================================================
// Mutex globale (critical-section)
// ============================================================
/// Accès : critical_section::with(|cs| { SHARED_STATE.borrow(cs).borrow_mut()... });
pub static SHARED_STATE: Mutex<RefCell<SystemState>> =
    Mutex::new(RefCell::new(SystemState::new()));
