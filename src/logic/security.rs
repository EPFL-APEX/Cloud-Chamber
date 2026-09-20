//! Évaluation des seuils de sécurité.
//!
//! Sous-module de `logic/` plutôt que module à part avec sa propre boucle
//! Core1 : le README décrit une boucle de sécurité indépendante à 100 Hz,
//! mais cette architecture est abandonnée — la sécurité est maintenant une
//! source de transition prioritaire pour `SystemTask`, au même titre que
//! `sensor_loss_abort`/`timed_transition` dans `control_loop.rs`, pas une
//! tâche séparée.
//!
//! # Un danger, une variante
//!
//! Tout ce que le moniteur surveille est une variante de [`SafetyCause`].
//! [`SafetyMonitor::severity_of`] dit comment mesurer chacune — en `match`
//! exhaustif, donc le compilateur réclame la réponse — et
//! [`SafetyMonitor::evaluate`] parcourt [`SafetyCause::ALL`] et garde la
//! pire. Ajouter un danger, c'est ajouter une variante ; le reste suit ou
//! ne compile pas.
//!
//! C'était le sujet d'un TODO : `evaluate` était une fonction libre qui ne
//! traitait que la surchauffe, et `check` traitait la perte de capteur à la
//! main juste en dessous, avec sa propre règle de priorité. Deux listes de
//! dangers, dont une seule avait l'air d'en être une.
//!
//! # Seuils à deux niveaux
//! - `warn`  : zone d'attention, signalement uniquement (pas de coupure).
//! - `alarm` : seuil critique, déclenche `SystemTask::Tripped` quand le
//!   compteur à fuite atteint `TRIP_CYCLES` (anti-rebond — cf.
//!   [`SafetyMonitor::check`]).

use crate::cloud_chamber_hal::config::COMPRESSOR_OUT_IDX;
use crate::cloud_chamber_hal::timer::Instant;
use crate::cloud_chamber_hal::units::{Celsius, Unit};
use crate::config::operating::SAFETY_TEMP_COMPRESSOR_MAX;
use crate::logic::probing::MeasurementHistory;
use crate::logic::timing::SENSOR_LOSS;

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

/// Un danger surveillé — et, quand il déclenche, la cause affichée à
/// l'opérateur.
///
/// Cet enum porte **la liste des dangers**, pas seulement des étiquettes.
/// [`SafetyMonitor::severity_of`] est un `match` exhaustif sans bras `_` :
/// ajouter une variante ici ne compile pas tant qu'on n'a pas dit comment la
/// mesurer, et [`SafetyMonitor::evaluate`] la prend alors en compte sans
/// qu'on ait à y toucher.
///
/// C'est le remplaçant d'une organisation où une fonction `evaluate` libre
/// traitait la surchauffe pendant que `check` traitait la perte de capteur à
/// la main, juste à côté : deux listes de dangers en deux endroits, dont une
/// seule se voyait.
#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, num_enum::TryFromPrimitive, num_enum::IntoPrimitive)]
pub enum SafetyCause {
    /// T° sortie compresseur au-dessus du seuil.
    CompressorOverheat,
    /// Sonde sortie-compresseur invalide depuis trop longtemps.
    ///
    /// Contrairement à une lecture ponctuelle invalide — ignorée, pour ne
    /// pas générer de faux positif au démarrage — une invalidité prolongée
    /// (`SENSOR_LOSS`) est un danger à part entière : un capteur de sécurité
    /// débranché ne doit pas désactiver silencieusement la protection qu'il
    /// est censé fournir.
    CompressorSensorLost,
}

impl SafetyCause {
    /// Tous les dangers, dans l'ordre où [`SafetyMonitor::evaluate`] les
    /// interroge.
    ///
    /// L'ordre ne décide presque de rien : c'est la sévérité la plus haute
    /// qui gagne, et l'ordre ne départage qu'une égalité. Tenu complet par
    /// `every_cause_is_listed_in_all` — le `match` exhaustif bloque déjà la
    /// compilation, ce test ferme l'autre moitié : une variante traitée
    /// partout mais absente d'ici ne serait jamais interrogée.
    pub const ALL: [SafetyCause; 2] =
        [SafetyCause::CompressorOverheat, SafetyCause::CompressorSensorLost];
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
    U: Unit,
{
    if value > alarm {
        Severity::Alarm
    } else if value > warn {
        Severity::Warning
    } else {
        Severity::Normal
    }
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
        self.note_sensor_freshness(history, now);
        let (severity, cause) = self.evaluate(history, now);

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
        if severity == Severity::Alarm {
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

    /// Mémorise la dernière fois que la sonde sortie-compresseur a répondu.
    ///
    /// Isolé du reste parce que c'est la seule partie de l'évaluation qui
    /// **écrit** : [`Self::evaluate`] et [`Self::severity_of`] ne font que
    /// lire, donc on peut les appeler deux fois de suite sans changer le
    /// verdict, et un test peut interroger un danger sans faire avancer
    /// l'état du moniteur.
    fn note_sensor_freshness(&mut self, history: &MeasurementHistory, now: Instant) {
        if history.has_valid_reading(COMPRESSOR_OUT_IDX) {
            self.last_compressor_valid = now;
        }
    }

    /// Sévérité de **ce danger-là**, maintenant.
    ///
    /// C'est ici que le compilateur réclame une réponse pour chaque variante
    /// de [`SafetyCause`] : pas de bras `_`, donc un danger ajouté et pas
    /// mesuré donne une erreur E0004 nommant la variante manquante.
    fn severity_of(
        &self,
        cause: SafetyCause,
        history: &MeasurementHistory,
        now: Instant,
    ) -> Severity {
        match cause {
            // Lecture absente ou NaN : `Normal`, pas d'alarme. Le ring
            // buffer s'initialise à NaN, donc au démarrage la sonde n'a
            // simplement rien dit encore — une alarme ici serait un faux
            // positif systématique. L'invalidité qui *dure* est le danger
            // d'en dessous.
            SafetyCause::CompressorOverheat => match history.newest_valid(COMPRESSOR_OUT_IDX) {
                Some(temp) => check_high(
                    temp,
                    self.config.temp_compressor_warn,
                    self.config.temp_compressor_alarm,
                ),
                None => Severity::Normal,
            },
            SafetyCause::CompressorSensorLost => {
                match now.since(self.last_compressor_valid) > SENSOR_LOSS {
                    true => Severity::Alarm,
                    false => Severity::Normal,
                }
            }
        }
    }

    /// Sévérité la plus haute parmi tous les dangers, et le premier qui
    /// l'atteint.
    ///
    /// La comparaison est stricte (`>`), donc à sévérité égale c'est l'ordre
    /// de [`SafetyCause::ALL`] qui tranche. En pratique les deux dangers
    /// actuels s'excluent — une sonde perdue ne peut pas être en surchauffe,
    /// elle ne lit rien — mais la règle est écrite pour ne pas dépendre de
    /// cette coïncidence.
    ///
    /// Ne modifie rien : appelable à volonté.
    fn evaluate(
        &self,
        history: &MeasurementHistory,
        now: Instant,
    ) -> (Severity, Option<SafetyCause>) {
        SafetyCause::ALL.iter().fold(
            (Severity::Normal, None),
            |(worst, worst_cause), &cause| match self.severity_of(cause, history, now) {
                severity if severity > worst => (severity, Some(cause)),
                _ => (worst, worst_cause),
            },
        )
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

    /// `ALL` doit contenir chaque danger exactement une fois.
    ///
    /// Le `match` exhaustif de `severity_of` empêche déjà d'ajouter une
    /// variante sans dire comment la mesurer ; ce test ferme l'autre moitié,
    /// celle que le typage ne couvre pas : une variante déclarée, mesurable,
    /// mais absente de `ALL` — donc jamais interrogée par `evaluate`. Un
    /// danger surveillé sur le papier et par personne en pratique.
    #[test]
    fn every_cause_is_listed_in_all() {
        let mut variants = 0usize;
        while SafetyCause::try_from(variants).is_ok() {
            variants += 1;
        }

        assert_eq!(variants, SafetyCause::ALL.len(), "un danger manque dans SafetyCause::ALL");

        for (i, a) in SafetyCause::ALL.iter().enumerate() {
            for b in &SafetyCause::ALL[i + 1..] {
                assert_ne!(a, b, "doublon dans ALL");
            }
        }
    }

    /// `evaluate` ne touche à rien : deux appels d'affilée rendent le même
    /// verdict et ne font pas avancer l'anti-rebond. C'est ce qui permet de
    /// l'appeler pour interroger l'état sans le modifier.
    #[test]
    fn evaluate_is_free_of_side_effects() {
        let safety = SafetyMonitor::new(SafetyConfig::default(), at_ms(0));
        let alarm = history_with_compressor_temp(150.0);

        let first = safety.evaluate(&alarm, at_ms(1));
        let second = safety.evaluate(&alarm, at_ms(1));

        assert_eq!(first, second);
        assert_eq!(first, (Severity::Alarm, Some(SafetyCause::CompressorOverheat)));
        assert_eq!(safety.alarm_cycles, 0, "evaluate ne doit pas faire avancer l'anti-rebond");
    }

    /// La règle de priorité, maintenant qu'elle est portée par la sévérité
    /// et non par un `if` ad hoc : à sévérité égale, l'ordre de `ALL`
    /// tranche ; une sévérité plus haute l'emporte quel que soit l'ordre.
    #[test]
    fn the_worst_severity_wins_over_the_listing_order() {
        let mut safety = SafetyMonitor::new(SafetyConfig::default(), at_ms(0));
        let mute = MeasurementHistory::new();

        // La sonde n'a jamais répondu : seule `CompressorSensorLost` monte,
        // et elle est pourtant la dernière de `ALL`.
        let late = at_ms(SENSOR_LOSS.as_millis() + 1);
        safety.note_sensor_freshness(&mute, late);
        assert_eq!(
            safety.evaluate(&mute, late),
            (Severity::Alarm, Some(SafetyCause::CompressorSensorLost)),
        );
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
