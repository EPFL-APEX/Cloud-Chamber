//! Logique d'évaluation des seuils de sécurité.
//!
//! # Principe
//!
//! `evaluate_safety()` prend un instantané des mesures et une configuration
//! de seuils, et retourne l'état du système : Normal → Warning → Alarm → Emergency.
//!
//! # Seuils à deux niveaux
//!
//! Chaque grandeur a deux niveaux d'alerte :
//! - `warn` : zone d'attention, avertissement visuel uniquement
//! - `alarm` : seuil critique, déclenchement du disjoncteur

use crate::shared::data::{
    SensorSnapshot, SystemState, NUMBER_OF_AMP, NUMBER_OF_TEMPS, NUMBER_OF_VOLT,
};

/// Configuration des seuils de sécurité.
#[derive(Debug, Clone, Copy)]
pub struct SafetyConfig {
    pub temp_warn: [f32; NUMBER_OF_TEMPS],
    pub temp_alarm: [f32; NUMBER_OF_TEMPS],
    pub volt_warn: [f32; NUMBER_OF_VOLT],
    pub volt_alarm: [f32; NUMBER_OF_VOLT],
    pub amp_warn: [f32; NUMBER_OF_AMP],
    pub amp_alarm: [f32; NUMBER_OF_AMP],
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            temp_warn:  [45.0; NUMBER_OF_TEMPS],
            temp_alarm: [60.0; NUMBER_OF_TEMPS],
            volt_warn:  [28.0; NUMBER_OF_VOLT],
            volt_alarm: [32.0; NUMBER_OF_VOLT],
            amp_warn:   [8.0;  NUMBER_OF_AMP],
            amp_alarm:  [10.0; NUMBER_OF_AMP],
        }
    }
}

/// Évalue l'état du système à partir d'un instantané et d'une configuration.
///
/// Retourne le niveau de sévérité le plus élevé trouvé parmi tous les capteurs.
pub fn evaluate_safety(snapshot: &SensorSnapshot, config: &SafetyConfig) -> SystemState {
    let mut worst = SystemState::Normal;

    for (i, &t) in snapshot.temps.iter().enumerate() {
        worst = worst.max_severity(check_threshold(t, config.temp_warn[i], config.temp_alarm[i]));
    }
    for (i, &v) in snapshot.volts.iter().enumerate() {
        worst = worst.max_severity(check_threshold(v, config.volt_warn[i], config.volt_alarm[i]));
    }
    for (i, &a) in snapshot.amps.iter().enumerate() {
        worst = worst.max_severity(check_threshold(a, config.amp_warn[i], config.amp_alarm[i]));
    }

    worst
}

/// Retourne l'état correspondant à une valeur par rapport à ses seuils.
fn check_threshold(value: f32, warn: f32, alarm: f32) -> SystemState {
    if value >= alarm {
        SystemState::Alarm
    } else if value >= warn {
        SystemState::Warning
    } else {
        SystemState::Normal
    }
}

impl SystemState {
    /// Retourne le niveau de sévérité le plus élevé entre `self` et `other`.
    pub fn max_severity(self, other: SystemState) -> SystemState {
        let rank = |s: SystemState| match s {
            SystemState::Normal    => 0u8,
            SystemState::Warning   => 1,
            SystemState::Alarm     => 2,
            SystemState::Emergency => 3,
        };
        if rank(other) > rank(self) { other } else { self }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_with_temp(temp: f32) -> SensorSnapshot {
        let mut s = SensorSnapshot::default();
        s.temps[0] = temp;
        s
    }

    #[test]
    fn all_zero_is_normal() {
        let state = evaluate_safety(&SensorSnapshot::default(), &SafetyConfig::default());
        assert_eq!(state, SystemState::Normal);
    }

    #[test]
    fn temp_above_warn_is_warning() {
        let state = evaluate_safety(&snapshot_with_temp(50.0), &SafetyConfig::default());
        assert_eq!(state, SystemState::Warning);
    }

    #[test]
    fn temp_above_alarm_is_alarm() {
        let state = evaluate_safety(&snapshot_with_temp(65.0), &SafetyConfig::default());
        assert_eq!(state, SystemState::Alarm);
    }

    #[test]
    fn temp_exactly_at_warn_is_warning() {
        let state = evaluate_safety(&snapshot_with_temp(45.0), &SafetyConfig::default());
        assert_eq!(state, SystemState::Warning);
    }

    #[test]
    fn temp_just_below_warn_is_normal() {
        let state = evaluate_safety(&snapshot_with_temp(44.9), &SafetyConfig::default());
        assert_eq!(state, SystemState::Normal);
    }

    #[test]
    fn max_severity_picks_highest() {
        assert_eq!(SystemState::Normal.max_severity(SystemState::Alarm), SystemState::Alarm);
        assert_eq!(SystemState::Warning.max_severity(SystemState::Normal), SystemState::Warning);
        assert_eq!(SystemState::Emergency.max_severity(SystemState::Alarm), SystemState::Emergency);
    }

    #[test]
    fn multiple_sensors_worst_wins() {
        let mut s = SensorSnapshot::default();
        s.temps[0] = 50.0; // Warning
        s.temps[1] = 65.0; // Alarm
        let state = evaluate_safety(&s, &SafetyConfig::default());
        assert_eq!(state, SystemState::Alarm);
    }

    #[test]
    fn voltage_above_alarm_is_alarm() {
        let mut s = SensorSnapshot::default();
        s.volts[0] = 33.0;
        let state = evaluate_safety(&s, &SafetyConfig::default());
        assert_eq!(state, SystemState::Alarm);
    }
}
