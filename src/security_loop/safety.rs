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
    SensorSnapshot, SystemTask
};
use crate::config::{
    NUMBER_OF_TEMP_SENSOR,
    NUMBER_OF_PRESSURE_SENSOR,
    NUMBER_OF_VOLTMETER,
    NUMBER_OF_AMPMETER,
};

/// Configuration des seuils de sécurité.
#[derive(Debug, Clone, Copy)]
pub struct SafetyConfig {
    pub temp_warn: [f32; NUMBER_OF_TEMP_SENSOR],
    pub temp_alarm: [f32; NUMBER_OF_TEMP_SENSOR],
    pub volt_warn: [f32; NUMBER_OF_VOLTMETER],
    pub volt_alarm: [f32; NUMBER_OF_VOLTMETER],
    pub amp_warn: [f32; NUMBER_OF_AMPMETER],
    pub amp_alarm: [f32; NUMBER_OF_AMPMETER],
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            temp_warn:  [45.0; NUMBER_OF_TEMP_SENSOR],
            temp_alarm: [60.0; NUMBER_OF_TEMP_SENSOR],
            volt_warn:  [28.0; NUMBER_OF_VOLTMETER],
            volt_alarm: [32.0; NUMBER_OF_VOLTMETER],
            amp_warn:   [8.0;  NUMBER_OF_AMPMETER],
            amp_alarm:  [10.0; NUMBER_OF_AMPMETER],
        }
    }
}

/// Évalue l'état du système à partir d'un instantané et d'une configuration.
///
/// Retourne le niveau de sévérité le plus élevé trouvé parmi tous les capteurs.
pub fn evaluate_safety(snapshot: &SensorSnapshot, config: &SafetyConfig) -> SystemTask {
    todo!()
}

/// Retourne l'état correspondant à une valeur par rapport à ses seuils.
fn check_threshold(value: f32, warn: f32, alarm: f32) -> SystemTask {
    todo!()
}

impl SystemState {
    /// Retourne le niveau de sévérité le plus élevé entre `self` et `other`.
    pub fn max_severity(self, other: SystemState) -> SystemTask {
        todo!()
    }
}