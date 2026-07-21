//! Évaluation des seuils de sécurité — port de security_loop/safety.rs de la
//! branche add-phase-transition-logic, adapté aux capteurs réellement câblés
//! (DS18B20 sortie compresseur, ABP2 HP/BP) et implémenté (les todo!() de la
//! branche cible sont remplacés).
//!
//! # Seuils à deux niveaux (repris de la branche cible)
//! - `warn`  : zone d'attention, signalement uniquement
//! - `alarm` : seuil critique, coupure (disjoncteur logiciel)

use crate::config::{
    COMPRESSOR_OUT_IDX, SAFETY_BP_MIN, SAFETY_HP_MAX, SAFETY_TEMP_COMPRESSOR_MAX,
};
use crate::data::SystemState;

/// Niveau de sévérité, ordonné (Normal < Warning < Alarm).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Normal,
    Warning,
    Alarm,
}

/// Configuration des seuils.
#[derive(Debug, Clone, Copy)]
pub struct SafetyConfig {
    /// T° sortie compresseur (°C) — surchauffe.
    pub temp_compressor_warn:  f32,
    pub temp_compressor_alarm: f32,
    /// Pression HP (bar) — risque mécanique.
    pub hp_warn:  f32,
    pub hp_alarm: f32,
    /// Pression BP (bar) — perte de réfrigérant (seuils BAS).
    pub bp_warn_low:  f32,
    pub bp_alarm_low: f32,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            temp_compressor_warn:  100.0,
            temp_compressor_alarm: SAFETY_TEMP_COMPRESSOR_MAX, // 120.0
            hp_warn:  12.0,
            hp_alarm: SAFETY_HP_MAX, // 14.0
            bp_warn_low:  0.25,
            bp_alarm_low: SAFETY_BP_MIN, // 0.15
        }
    }
}

/// Niveau d'une valeur par rapport à ses seuils hauts.
fn check_high(value: f32, warn: f32, alarm: f32) -> Severity {
    if value > alarm { Severity::Alarm }
    else if value > warn { Severity::Warning }
    else { Severity::Normal }
}

/// Niveau d'une valeur par rapport à ses seuils bas.
fn check_low(value: f32, warn: f32, alarm: f32) -> Severity {
    if value < alarm { Severity::Alarm }
    else if value < warn { Severity::Warning }
    else { Severity::Normal }
}

/// Évalue la sévérité globale à partir de l'état capteurs.
/// Les capteurs invalides sont ignorés (pas de faux positif au débranchement —
/// le retrait d'un capteur de sécurité est traité par le Controller).
pub fn evaluate_safety(state: &SystemState, config: &SafetyConfig) -> Severity {
    let mut sev = Severity::Normal;

    let t_comp = &state.temperatures[COMPRESSOR_OUT_IDX];
    if t_comp.valid {
        sev = sev.max(check_high(t_comp.value,
                                 config.temp_compressor_warn,
                                 config.temp_compressor_alarm));
    }
    if state.pressure_hp.valid {
        sev = sev.max(check_high(state.pressure_hp.pressure,
                                 config.hp_warn, config.hp_alarm));
    }
    if state.pressure_bp.valid {
        sev = sev.max(check_low(state.pressure_bp.pressure,
                                config.bp_warn_low, config.bp_alarm_low));
    }
    sev
}
