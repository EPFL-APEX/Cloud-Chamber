//! Évaluation des seuils de sécurité.
//!
//! Sous-module de `logic/` plutôt que module à part avec sa propre boucle
//! Core1 : le README décrit une boucle de sécurité indépendante à 100 Hz,
//! mais cette architecture est abandonnée — la sécurité est maintenant une
//! source de transition prioritaire pour `SystemTask`, au même titre que
//! `sensor_loss_abort`/`timed_transition` dans `control_loop.rs`, pas une
//! tâche séparée.
//!
//! # Seuils à deux niveaux
//! - `warn`  : zone d'attention, signalement uniquement (pas de coupure).
//! - `alarm` : seuil critique, déclenche `SystemTask::Tripped` après
//!   `TRIP_CYCLES` cycles consécutifs (anti-rebond).
//!
//! # Capteur sortie-compresseur invalide
//! Contrairement à une lecture ponctuelle invalide (ignorée, pour ne pas
//! générer de faux positif au démarrage), une invalidité prolongée
//! (`SENSOR_LOSS_MS`) est elle-même traitée comme une alarme : un capteur de
//! sécurité débranché ne doit pas désactiver silencieusement la protection
//! qu'il est censé fournir.

use crate::cloud_chamber_hal::config::{COMPRESSOR_OUT_IDX, HP_PRESSURE_IDX, BP_PRESSURE_IDX};
use crate::config::{SAFETY_BP_MIN, SAFETY_HP_MAX, SAFETY_TEMP_COMPRESSOR_MAX, SENSOR_LOSS_MS};
use crate::logic::probing::MeasurementHistory;

/// Nombre de cycles consécutifs en Alarm avant déclenchement (anti-rebond).
const TRIP_CYCLES: u8 = 3;

/// Niveau de sévérité, ordonné (Normal < Warning < Alarm).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Normal,
    Warning,
    Alarm,
}

/// Cause du déclenchement (pour l'affichage opérateur).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyCause {
    /// T° sortie compresseur au-dessus du seuil.
    CompressorOverheat,
    /// Sonde sortie-compresseur invalide depuis trop longtemps.
    CompressorSensorLost,
    /// Pression HP trop haute.
    PressureHigh,
    /// Pression BP trop basse.
    PressureLow,
}

/// Configuration des seuils.
#[derive(Debug, Clone, Copy)]
pub struct SafetyConfig {
    /// T° sortie compresseur (°C) — surchauffe.
    pub temp_compressor_warn: f32,
    pub temp_compressor_alarm: f32,
    /// Pression HP (bar) — risque mécanique.
    pub hp_warn: f32,
    pub hp_alarm: f32,
    /// Pression BP (bar) — perte de réfrigérant (seuils BAS).
    pub bp_warn_low: f32,
    pub bp_alarm_low: f32,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            temp_compressor_warn: 100.0,
            temp_compressor_alarm: SAFETY_TEMP_COMPRESSOR_MAX,
            hp_warn: 12.0,
            hp_alarm: SAFETY_HP_MAX, // TODO CALIBRAGE — cf. commentaire dans config.rs
            bp_warn_low: 0.25,
            bp_alarm_low: SAFETY_BP_MIN,
        }
    }
}

fn check_high(value: f32, warn: f32, alarm: f32) -> Severity {
    if value > alarm { Severity::Alarm }
    else if value > warn { Severity::Warning }
    else { Severity::Normal }
}

fn check_low(value: f32, warn: f32, alarm: f32) -> Severity {
    if value < alarm { Severity::Alarm }
    else if value < warn { Severity::Warning }
    else { Severity::Normal }
}

/// Évalue la sévérité globale et sa cause dominante à partir de l'historique.
/// Une lecture jamais faite (NaN) est ignorée ponctuellement — l'invalidité
/// prolongée du capteur sortie-compresseur est traitée séparément par
/// `SafetyMonitor::check` (elle a besoin de mémoriser depuis quand).
fn evaluate(history: &MeasurementHistory, config: &SafetyConfig) -> (Severity, Option<SafetyCause>) {
    let mut worst_sev = Severity::Normal;
    let mut worst_cause = None;

    if let Ok(m) = history.temps[COMPRESSOR_OUT_IDX].get(0) {
        if !m.value.0.is_nan() {
            let s = check_high(m.value.0, config.temp_compressor_warn, config.temp_compressor_alarm);
            if s > worst_sev { worst_sev = s; worst_cause = Some(SafetyCause::CompressorOverheat); }
        }
    }
    if let Ok(m) = history.press[HP_PRESSURE_IDX].get(0) {
        if !m.value.0.is_nan() {
            let s = check_high(m.value.0, config.hp_warn, config.hp_alarm);
            if s > worst_sev { worst_sev = s; worst_cause = Some(SafetyCause::PressureHigh); }
        }
    }
    if let Ok(m) = history.press[BP_PRESSURE_IDX].get(0) {
        if !m.value.0.is_nan() {
            let s = check_low(m.value.0, config.bp_warn_low, config.bp_alarm_low);
            if s > worst_sev { worst_sev = s; worst_cause = Some(SafetyCause::PressureLow); }
        }
    }
    (worst_sev, worst_cause)
}

/// Moniteur de sécurité — anti-rebond, verrouillage, et suivi de fraîcheur
/// du capteur sortie-compresseur.
pub struct SafetyMonitor {
    config: SafetyConfig,
    alarm_cycles: u8,
    tripped: bool,
    trip_cause: Option<SafetyCause>,
    last_compressor_valid_ms: u64,
}

impl SafetyMonitor {
    pub fn new(config: SafetyConfig, now_ms: u64) -> Self {
        Self {
            config,
            alarm_cycles: 0,
            tripped: false,
            trip_cause: None,
            last_compressor_valid_ms: now_ms,
        }
    }

    /// À appeler à chaque cycle. Retourne `Some(cause)` si le disjoncteur
    /// doit être (ou rester) déclenché.
    pub fn check(&mut self, history: &MeasurementHistory, now_ms: u64) -> Option<SafetyCause> {
        let compressor_valid = matches!(
            history.temps[COMPRESSOR_OUT_IDX].get(0),
            Ok(m) if !m.value.0.is_nan()
        );
        if compressor_valid {
            self.last_compressor_valid_ms = now_ms;
        }
        let compressor_lost = now_ms.saturating_sub(self.last_compressor_valid_ms) > SENSOR_LOSS_MS;

        let (sev, cause) = evaluate(history, &self.config);
        let (sev, cause) = if compressor_lost && sev < Severity::Alarm {
            (Severity::Alarm, Some(SafetyCause::CompressorSensorLost))
        } else {
            (sev, cause)
        };

        if sev == Severity::Alarm {
            self.alarm_cycles = self.alarm_cycles.saturating_add(1);
            if self.alarm_cycles >= TRIP_CYCLES {
                if !self.tripped {
                    self.trip_cause = cause;
                }
                self.tripped = true;
            }
        } else {
            self.alarm_cycles = 0;
        }

        if self.tripped { self.trip_cause } else { None }
    }

    pub fn is_tripped(&self) -> bool {
        self.tripped
    }

    /// Réarme le disjoncteur (reconnaissance opérateur). Sans effet durable
    /// si la condition d'alarme est toujours présente : `check()` re-
    /// déclenchera après `TRIP_CYCLES` au prochain appel.
    pub fn reset(&mut self, now_ms: u64) {
        self.tripped = false;
        self.alarm_cycles = 0;
        self.trip_cause = None;
        // Évite un trip immédiat sur "capteur perdu" si la sonde était déjà
        // invalide au moment du réarmement.
        self.last_compressor_valid_ms = now_ms;
    }
}
