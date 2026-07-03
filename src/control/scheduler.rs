use crate::config::{
    CRITICAL_READ_INTERVAL_MS, NON_CRITICAL_READ_INTERVAL_MS, CRITICAL_TEMP_INDICES,
    SENSOR_FAILURE_RETRY_MS,
    BME280_FAST_TEMP_C, BME280_FAST_PRESSURE_HPA, BME280_FAST_HUMIDITY_PCT,
};
use crate::data::MAX_TEMP_SENSORS;

/// Si |ΔT| depuis la dernière mesure dépasse ce seuil, on resamples au rythme rapide.
const FAST_CHANGE_THRESHOLD_C: f32 = 1.0;

/// En régime stable (faible ΔT), l'intervalle est multiplié par ce facteur.
const SLOW_INTERVAL_FACTOR: u64 = 4;

/// Planificateur de mesures des sondes de température.
///
/// Principe : **un seul capteur est mesuré par cycle**. Cela évite les 2.5 s de blocage
/// que provoquerait la lecture séquentielle des 5 sondes DS18B20.
///
/// Priorité de sélection :
///   1. Capteurs critiques (sortie_compresseur) passent avant les non-critiques.
///   2. À criticité égale, le plus en retard sur son `next_due` passe en premier.
///
/// Fréquence adaptative :
///   - Si le capteur vient de changer vite (|ΔT| > FAST_CHANGE_THRESHOLD_C) → intervalle court.
///   - Sinon (régime stable) → intervalle ×SLOW_INTERVAL_FACTOR.
#[derive(Debug)]
pub struct TempScheduler {
    next_due_ms: [u64; MAX_TEMP_SENSORS],
    last_value:  [f32; MAX_TEMP_SENSORS],
}

impl TempScheduler {
    pub const fn new() -> Self {
        Self {
            next_due_ms: [0; MAX_TEMP_SENSORS],
            last_value:  [0.0; MAX_TEMP_SENSORS],
        }
    }

    fn is_critical(idx: usize) -> bool {
        CRITICAL_TEMP_INDICES.contains(&idx)
    }

    /// Retourne l'index du capteur à mesurer maintenant, ou `None` si aucun n'est dû.
    pub fn next_to_measure(&self, now_ms: u64) -> Option<usize> {
        let mut best_idx:      Option<usize> = None;
        let mut best_overdue:  u64           = 0;
        let mut best_critical: bool          = false;

        for idx in 0..MAX_TEMP_SENSORS {
            if self.next_due_ms[idx] > now_ms { continue; }

            let overdue  = now_ms.saturating_sub(self.next_due_ms[idx]);
            let critical = Self::is_critical(idx);

            let replace = match best_idx {
                None    => true,
                Some(_) => (critical && !best_critical)
                    || (critical == best_critical && overdue > best_overdue),
            };

            if replace {
                best_idx      = Some(idx);
                best_overdue  = overdue;
                best_critical = critical;
            }
        }
        best_idx
    }

    /// Enregistre la valeur mesurée et planifie la prochaine échéance.
    pub fn record_measurement(&mut self, idx: usize, value: f32, now_ms: u64) {
        let delta = (value - self.last_value[idx]).abs();
        self.last_value[idx] = value;

        let base = if Self::is_critical(idx) {
            CRITICAL_READ_INTERVAL_MS
        } else {
            NON_CRITICAL_READ_INTERVAL_MS
        };

        let interval = if delta > FAST_CHANGE_THRESHOLD_C {
            base
        } else {
            base * SLOW_INTERVAL_FACTOR
        };

        self.next_due_ms[idx] = now_ms + interval;
    }

    /// À appeler quand la lecture d'un capteur échoue (erreur driver).
    ///
    /// Planifie un retry après `SENSOR_FAILURE_RETRY_MS` sans toucher à `last_value`
    /// (on garde la dernière valeur connue). Évite de harceler un capteur défaillant
    /// à chaque cycle.
    pub fn record_failure(&mut self, idx: usize, now_ms: u64) {
        self.next_due_ms[idx] = now_ms + SENSOR_FAILURE_RETRY_MS;
    }

    /// Force tous les capteurs à être mesurés dès le prochain cycle (démarrage, reprise).
    pub fn mark_all_due(&mut self) {
        self.next_due_ms = [0; MAX_TEMP_SENSORS];
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Scheduler BME280 (capteur ambiance : T / P_atmo / H)
//
// Même logique de dérivée que TempScheduler, mais sur 3 canaux scalaires.
// Si au moins un canal dépasse son seuil de variation → intervalle rapide.
// Sinon (régime stable) → intervalle lent.
// ─────────────────────────────────────────────────────────────────────────────

/// Planificateur adaptatif pour le BME280.
#[derive(Debug)]
pub struct Bme280Scheduler {
    next_due_ms:   u64,
    last_temp:     f32,
    last_pressure: f32,
    last_humidity: f32,
}

impl Bme280Scheduler {
    pub const fn new() -> Self {
        Self {
            next_due_ms:   0,
            last_temp:     0.0,
            last_pressure: 0.0,
            last_humidity: 0.0,
        }
    }

    /// Retourne `true` si une mesure est due maintenant.
    pub fn is_due(&self, now_ms: u64) -> bool {
        now_ms >= self.next_due_ms
    }

    /// Millisecondes restantes avant la prochaine mesure (0 si déjà due).
    pub fn ms_until_due(&self, now_ms: u64) -> u64 {
        self.next_due_ms.saturating_sub(now_ms)
    }

    /// Enregistre une mesure réussie et planifie la prochaine échéance.
    ///
    /// Si au moins un canal a varié au-delà de son seuil → intervalle rapide.
    /// Sinon → intervalle lent (ambiance stable).
    pub fn record_measurement(
        &mut self,
        temp_c:       f32,
        pressure_hpa: f32,
        humidity_pct: f32,
        now_ms:       u64,
    ) {
        let fast = (temp_c       - self.last_temp    ).abs() > BME280_FAST_TEMP_C
                || (pressure_hpa - self.last_pressure).abs() > BME280_FAST_PRESSURE_HPA
                || (humidity_pct - self.last_humidity).abs() > BME280_FAST_HUMIDITY_PCT;

        self.last_temp     = temp_c;
        self.last_pressure = pressure_hpa;
        self.last_humidity = humidity_pct;

        self.next_due_ms = now_ms + if fast {
            CRITICAL_READ_INTERVAL_MS
        } else {
            CRITICAL_READ_INTERVAL_MS * SLOW_INTERVAL_FACTOR
        };
    }

    /// À appeler sur échec I²C — planifie un retry sans écraser `last_*`.
    pub fn record_failure(&mut self, now_ms: u64) {
        self.next_due_ms = now_ms + SENSOR_FAILURE_RETRY_MS;
    }
}
