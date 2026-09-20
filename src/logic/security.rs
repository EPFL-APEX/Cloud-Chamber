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
//! (`SENSOR_LOSS`) est elle-même traitée comme une alarme : un capteur de
//! sécurité débranché ne doit pas désactiver silencieusement la protection
//! qu'il est censé fournir.

use crate::cloud_chamber_hal::config::COMPRESSOR_OUT_IDX;
use crate::cloud_chamber_hal::timer::Instant;
use crate::cloud_chamber_hal::units::{Celsius, Unit};
use crate::config::operating::SAFETY_TEMP_COMPRESSOR_MAX;
use crate::logic::timing::SENSOR_LOSS;
use crate::logic::probing::MeasurementHistory;

/// Niveau que doit atteindre le compteur d'alarme pour déclencher.
///
/// Ce n'est plus un nombre de cycles *consécutifs* : cf.
/// [`SafetyMonitor::check`] et son compteur à fuite. Trois tours d'alarme
/// d'affilée déclenchent toujours, mais ce n'est plus le seul chemin.
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
}

/// Configuration des seuils.
///
/// Pas de seuil de pression ici : l'unique capteur de pression restant
/// mesure la chambre (`CHAMBER_PRESSURE_IDX`), pas le circuit réfrigérant
#[derive(Debug, Clone, Copy)]
pub struct SafetyConfig {
    /// T° sortie compresseur (°C) — surchauffe.
    pub temp_compressor_warn: Celsius,
    pub temp_compressor_alarm: Celsius,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            temp_compressor_warn: Celsius::new(100.0), // #todo Constante dans le fichier de config
            temp_compressor_alarm: SAFETY_TEMP_COMPRESSOR_MAX,
        }
    }
}

fn check_high<U>(value: U, warn: U, alarm: U) -> Severity
where
    U: Unit
{
    if value > alarm { Severity::Alarm }
    else if value > warn { Severity::Warning }
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
        if !m.value.is_nan() {
            let s = check_high(m.value, config.temp_compressor_warn, config.temp_compressor_alarm);
            if s > worst_sev { worst_sev = s; worst_cause = Some(SafetyCause::CompressorOverheat); }
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
    last_compressor_valid: Instant,
}

impl SafetyMonitor {
    pub fn new(config: SafetyConfig, now: Instant) -> Self {
        Self {
            config,
            alarm_cycles: 0,
            tripped: false,
            trip_cause: None,
            last_compressor_valid: now,
        }
    }

    /// À appeler à chaque cycle. Retourne `Some(cause)` si le disjoncteur
    /// doit être (ou rester) déclenché.
    pub fn check(&mut self, history: &MeasurementHistory, now: Instant) -> Option<SafetyCause> {
        let compressor_valid = matches!(
            history.temps[COMPRESSOR_OUT_IDX].get(0),
            Ok(m) if !m.value.is_nan()
        );
        if compressor_valid {
            self.last_compressor_valid = now;
        }
        let compressor_lost = now.since(self.last_compressor_valid) > SENSOR_LOSS;

        // Est-ce que c'est pas un peu mal foutu de faire une fonction evaluate et après de faire le
        // check à la main pour le compresseur ? Il faut arranger ça... #todo
        let (sev, cause) = evaluate(history, &self.config);
        let (sev, cause) = if compressor_lost && sev < Severity::Alarm {
            (Severity::Alarm, Some(SafetyCause::CompressorSensorLost))
        } else {
            (sev, cause)
        };

        // Anti-rebond à compteur de fuite : +1 par tour en alarme, -1 par
        // tour normal — et non une remise à zéro.
        //
        // La remise à zéro laissait passer les alarmes intermittentes. Avec
        // `TRIP_CYCLES = 3`, la séquence `A A N A A N ...` faisait 1, 2, 0,
        // 1, 2, 0 : elle ne déclenchait **jamais**, quelle que soit sa
        // durée. Un compresseur qui oscille autour de son seuil avec du
        // bruit de mesure produit exactement ça, et la surchauffe est bien
        // réelle pendant ce temps.
        //
        // La fuite garde l'anti-rebond qui justifiait la remise à zéro — un
        // pic isolé ne déclenche toujours pas — mais rend le critère
        // « plus de la moitié des tours récents sont en alarme » au lieu de
        // « trois tours d'affilée ». Une alternance exactement à 50 %
        // (`A N A N`) reste sous le seuil ; c'est une crête sans épaisseur,
        // le bruit réel tombe d'un côté ou de l'autre.
        //
        // Plafonné à `TRIP_CYCLES` : sans ça, une alarme longue ferait
        // monter le compteur indéfiniment et il faudrait autant de tours
        // normaux pour le vider une fois la cause disparue.
        if sev == Severity::Alarm {
            self.alarm_cycles = (self.alarm_cycles + 1).min(TRIP_CYCLES);
            if self.alarm_cycles >= TRIP_CYCLES {
                if !self.tripped {
                    self.trip_cause = cause;
                }
                self.tripped = true;
            }
        } else {
            self.alarm_cycles = self.alarm_cycles.saturating_sub(1);
        }

        if self.tripped { self.trip_cause } else { None }
    }

    pub fn is_tripped(&self) -> bool {
        self.tripped
    }

    /// Réarme le disjoncteur (reconnaissance opérateur). Sans effet durable
    /// si la condition d'alarme est toujours présente : `check()` re-
    /// déclenchera après `TRIP_CYCLES` au prochain appel.
    pub fn reset(&mut self, now: Instant) {
        self.tripped = false;
        self.alarm_cycles = 0;
        self.trip_cause = None;
        // Évite un trip immédiat sur "capteur perdu" si la sonde était déjà
        // invalide au moment du réarmement.
        self.last_compressor_valid = now;
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_chamber_hal::measurement::Measurement;
    use crate::cloud_chamber_hal::timer::Instant;
    use crate::cloud_chamber_hal::units::Celsius;

    /// Les instants de ces tests sont exprimés en millisecondes : c'est
    /// l'échelle des seuils qu'ils exercent (`SENSOR_LOSS`).
    fn at_ms(ms: u64) -> Instant {
        Instant::from_micros(ms * 1_000)
    }

    fn history_with_compressor_temp(value_c: f32) -> MeasurementHistory {
        let mut h = MeasurementHistory::new();
        h.temps[COMPRESSOR_OUT_IDX].push(Measurement::new(Instant::from_micros(0), Celsius(value_c)));
        h
    }

    #[test]
    fn stays_untripped_under_threshold() {
        let mut safety = SafetyMonitor::new(SafetyConfig::default(), at_ms(0));
        let history = history_with_compressor_temp(50.0); // très sous les 120°C d'alarme
        for ms in 1..=5 {
            assert_eq!(safety.check(&history, at_ms(ms)), None);
        }
        assert!(!safety.is_tripped());
    }

    #[test]
    fn does_not_trip_before_trip_cycles() {
        let mut safety = SafetyMonitor::new(SafetyConfig::default(), at_ms(0));
        let history = history_with_compressor_temp(150.0); // > 120°C alarme
        assert_eq!(safety.check(&history, at_ms(1)), None);
        assert_eq!(safety.check(&history, at_ms(2)), None); // 2 cycles seulement, TRIP_CYCLES = 3
        assert!(!safety.is_tripped());
    }

    #[test]
    fn trips_after_trip_cycles_consecutive_alarms() {
        let mut safety = SafetyMonitor::new(SafetyConfig::default(), at_ms(0));
        let history = history_with_compressor_temp(150.0);
        safety.check(&history, at_ms(1));
        safety.check(&history, at_ms(2));
        let cause = safety.check(&history, at_ms(3));
        assert_eq!(cause, Some(SafetyCause::CompressorOverheat));
        assert!(safety.is_tripped());
    }

    /// Un tour normal fait redescendre le compteur d'un cran — il ne le
    /// remet pas à zéro. Deux tours d'alarme, un normal, un d'alarme font
    /// donc 1, 2, 1, 2 : toujours sous le seuil, l'anti-rebond joue son
    /// rôle.
    #[test]
    fn a_normal_reading_drains_the_counter_by_one() {
        let mut safety = SafetyMonitor::new(SafetyConfig::default(), at_ms(0));
        let alarm = history_with_compressor_temp(150.0);
        let normal = history_with_compressor_temp(50.0);

        safety.check(&alarm, at_ms(1));
        safety.check(&alarm, at_ms(2));
        safety.check(&normal, at_ms(3));
        safety.check(&alarm, at_ms(4));

        assert!(!safety.is_tripped());
        assert_eq!(safety.alarm_cycles, 2, "1, 2, 1, 2 — pas de remise a zero");
    }

    /// Le défaut que la fuite corrige : avec une remise à zéro, la séquence
    /// `A A N` répétée faisait 1, 2, 0, 1, 2, 0… et ne déclenchait **jamais**,
    /// quelle que soit sa durée. Un compresseur qui oscille autour de son
    /// seuil avec du bruit de mesure produit exactement ça.
    #[test]
    fn an_intermittent_alarm_eventually_trips() {
        let mut safety = SafetyMonitor::new(SafetyConfig::default(), at_ms(0));
        let alarm = history_with_compressor_temp(150.0);
        let normal = history_with_compressor_temp(50.0);

        let mut now = 0;
        for _ in 0..5 {
            for history in [&alarm, &alarm, &normal] {
                now += 1;
                safety.check(history, at_ms(now));
            }
        }

        assert!(safety.is_tripped(), "deux tours sur trois en alarme doit finir par declencher");
        assert_eq!(safety.trip_cause, Some(SafetyCause::CompressorOverheat));
    }

    /// La contrepartie, énoncée pour qu'elle ne soit pas une surprise : le
    /// critère est « plus de la moitié des tours récents », donc une
    /// alternance exactement à 50 % reste sous le seuil. C'est une crête
    /// sans épaisseur — un bruit réel tombe d'un côté ou de l'autre.
    #[test]
    fn a_fifty_fifty_alternation_stays_below_the_threshold() {
        let mut safety = SafetyMonitor::new(SafetyConfig::default(), at_ms(0));
        let alarm = history_with_compressor_temp(150.0);
        let normal = history_with_compressor_temp(50.0);

        let mut now = 0;
        for _ in 0..20 {
            for history in [&alarm, &normal] {
                now += 1;
                safety.check(history, at_ms(now));
            }
        }

        assert!(!safety.is_tripped());
    }

    /// L'anti-rebond reste un anti-rebond : un pic isolé ne déclenche pas.
    #[test]
    fn an_isolated_spike_never_trips() {
        let mut safety = SafetyMonitor::new(SafetyConfig::default(), at_ms(0));
        let alarm = history_with_compressor_temp(150.0);
        let normal = history_with_compressor_temp(50.0);

        let mut now = 0;
        for _ in 0..10 {
            for history in [&normal, &normal, &normal, &alarm] {
                now += 1;
                safety.check(history, at_ms(now));
            }
        }

        assert!(!safety.is_tripped());
    }

    /// Le compteur est plafonné : sans ça, une alarme longue le ferait
    /// monter indéfiniment et il faudrait autant de tours normaux pour le
    /// vider une fois la cause disparue.
    #[test]
    fn the_counter_does_not_run_away_during_a_long_alarm() {
        let mut safety = SafetyMonitor::new(SafetyConfig::default(), at_ms(0));
        let alarm = history_with_compressor_temp(150.0);

        for now in 1..=50 {
            safety.check(&alarm, at_ms(now));
        }

        assert_eq!(safety.alarm_cycles, TRIP_CYCLES);
    }

    #[test]
    fn reset_clears_trip_when_condition_has_cleared() {
        let mut safety = SafetyMonitor::new(SafetyConfig::default(), at_ms(0));
        let alarm = history_with_compressor_temp(150.0);
        safety.check(&alarm, at_ms(1));
        safety.check(&alarm, at_ms(2));
        safety.check(&alarm, at_ms(3));
        assert!(safety.is_tripped());

        safety.reset(at_ms(4));
        assert!(!safety.is_tripped());

        let normal = history_with_compressor_temp(50.0);
        assert_eq!(safety.check(&normal, at_ms(5)), None);
    }

    #[test]
    fn reset_retrips_if_condition_still_present() {
        let mut safety = SafetyMonitor::new(SafetyConfig::default(), at_ms(0));
        let alarm = history_with_compressor_temp(150.0);
        safety.check(&alarm, at_ms(1));
        safety.check(&alarm, at_ms(2));
        safety.check(&alarm, at_ms(3));
        safety.reset(at_ms(4));
        assert!(!safety.is_tripped());

        // Condition toujours présente : re-déclenche après TRIP_CYCLES.
        safety.check(&alarm, at_ms(5));
        safety.check(&alarm, at_ms(6));
        let cause = safety.check(&alarm, at_ms(7));
        assert_eq!(cause, Some(SafetyCause::CompressorOverheat));
        assert!(safety.is_tripped());
    }

    #[test]
    fn prolonged_compressor_sensor_loss_is_treated_as_alarm() {
        let mut safety = SafetyMonitor::new(SafetyConfig::default(), at_ms(0));
        let history = MeasurementHistory::new(); // sonde jamais valide (NaN)
        safety.check(&history, at_ms(1));
        safety.check(&history, at_ms(SENSOR_LOSS.as_millis() + 1));
        safety.check(&history, at_ms(SENSOR_LOSS.as_millis() + 2));
        let cause = safety.check(&history, at_ms(SENSOR_LOSS.as_millis() + 3));
        assert_eq!(cause, Some(SafetyCause::CompressorSensorLost));
        assert!(safety.is_tripped());
    }

    #[test]
    fn brief_invalid_reading_at_startup_is_not_an_alarm() {
        // Lecture ponctuelle invalide (NaN), pas encore assez longtemps pour
        // dépasser SENSOR_LOSS — pas de fausse alarme au démarrage.
        let mut safety = SafetyMonitor::new(SafetyConfig::default(), at_ms(0));
        let history = MeasurementHistory::new();
        assert_eq!(safety.check(&history, at_ms(1)), None);
        assert!(!safety.is_tripped());
    }
}
