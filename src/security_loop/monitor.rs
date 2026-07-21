//! Moniteur de sécurité — équivalent Core0 de la SecurityLoop de la branche
//! add-phase-transition-logic. Même logique (historique anti-rebond, seuils
//! deux niveaux, disjoncteur), exécutée dans la boucle de contrôle à 10 Hz.
//! Le déménagement sur Core1 devient un simple déplacement d'appel.

use crate::data::SystemState;
use super::safety::{evaluate_safety, SafetyConfig, Severity};

/// Nombre de cycles consécutifs en Alarm avant déclenchement (anti-parasite,
/// ~300 ms à 10 Hz — même valeur que l'ancien safety_triggered).
const TRIP_CYCLES: u8 = 3;

pub struct SecurityMonitor {
    config: SafetyConfig,
    alarm_cycles: u8,
    /// Disjoncteur : une fois déclenché, reste verrouillé jusqu'à un
    /// réarmement explicite (CYCLE 0 ou reset système).
    tripped: bool,
    pub last_severity: Severity,
}

impl SecurityMonitor {
    pub fn new(config: SafetyConfig) -> Self {
        Self { config, alarm_cycles: 0, tripped: false, last_severity: Severity::Normal }
    }

    /// À appeler à chaque cycle de contrôle. Retourne `true` si le système
    /// est sûr (pas de trip verrouillé).
    pub fn check(&mut self, state: &SystemState) -> bool {
        let sev = evaluate_safety(state, &self.config);
        self.last_severity = sev;

        if sev == Severity::Alarm {
            self.alarm_cycles = self.alarm_cycles.saturating_add(1);
            if self.alarm_cycles >= TRIP_CYCLES {
                self.tripped = true;
            }
        } else {
            self.alarm_cycles = 0;
        }
        !self.tripped
    }

    pub fn is_tripped(&self) -> bool {
        self.tripped
    }

    /// Réarme le disjoncteur (l'opérateur reconnaît l'incident).
    /// Sans effet si la condition d'alarme est toujours présente : le
    /// prochain check() re-déclenchera après TRIP_CYCLES.
    pub fn reset(&mut self) {
        self.tripped = false;
        self.alarm_cycles = 0;
    }
}
