use crate::cloud_chamber_hal::sensors::{BatchSensor, DeferredBatchSensor, Sensors};
use crate::cloud_chamber_hal::measurement::Measurement;
use crate::cloud_chamber_hal::units::{Celsius, HectoPascal, Unit};
use crate::cloud_chamber_hal::config::{
    CHAMBER_TEMP_IDX, NUMBER_OF_TEMP_SENSOR, NUMBER_OF_PRESSURE_SENSOR,
};
use crate::logic::timing::CONTROL_LOOP_HISTORY_SIZE;
use crate::shared::data::{SystemTask, SensorSnapshot};
use crate::cloud_chamber_hal::ring_buffer::RingBuffer;
use crate::cloud_chamber_hal::timer::{Instant, Duration};

/// Décide quelles catégories de capteurs sonder ce cycle.
///
/// `BatchSensor::read()` lit toujours les `N` capteurs d'une catégorie en un
/// seul appel (diffusion partagée pour les bus comme le 1-Wire) : impossible
/// de choisir un sous-ensemble de capteurs individuels. Le seul levier est
/// donc "sonder cette catégorie ce cycle, ou pas" — utile pour éviter le
/// délai de conversion température (jusqu'à ~800 ms) à chaque itération.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProbingPlan {
    pub probe_temperature: bool,
    pub probe_pressure: bool,
}

impl ProbingPlan {
    pub const fn new(probe_temperature: bool, probe_pressure: bool) -> Self {
        Self { probe_temperature, probe_pressure }
    }

    pub const fn all() -> Self {
        Self { probe_temperature: true, probe_pressure: true }
    }
}

impl SystemTask {
    pub fn create_probing_plan(&self, sys_hist: &MeasurementHistory) -> ProbingPlan {
        use SystemTask::*;
        match self {
            // Idle/Stabilising/Tripped : rien de spécifique à ces états ne
            // justifie de sonder un sous-ensemble — tout, comme les phases
            // de cooling/stopping (cf. leurs `create_probing_plan`).
            Idle | Stabilising | Tripped(_) => ProbingPlan::all(),
            Cooling(phase) => phase.create_probing_plan(sys_hist),
            Stopping(phase) => phase.create_probing_plan(sys_hist),
        }
    }
}

#[derive(Debug)]
pub struct MeasurementHistory {
    // indexation directe depuis logic::cooling/logic::stopping
    // via les constantes d'index de cloud_chamber_hal::config (pas
    // d'accesseurs par capteur, cf. décisions de conception de logic/).
    pub temps: [RingBuffer<Measurement<Celsius>, CONTROL_LOOP_HISTORY_SIZE>; NUMBER_OF_TEMP_SENSOR],
    pub press: [RingBuffer<Measurement<HectoPascal>, CONTROL_LOOP_HISTORY_SIZE>; NUMBER_OF_PRESSURE_SENSOR],
}

impl MeasurementHistory {

    pub fn new() -> Self {
        let t0 = Instant::from_micros(0);
        let default_temp = Measurement::new(t0, Celsius(f32::NAN));
        let default_press = Measurement::new(t0, HectoPascal(f32::NAN));
        Self {
            temps: core::array::from_fn(|_| RingBuffer::filled(default_temp)),
            press: core::array::from_fn(|_| RingBuffer::filled(default_press)),
        }
    }


    pub fn update(&mut self, latest_measurement: &SensorSnapshot) {
        push_if_newer(&mut self.temps, &latest_measurement.temps);
        push_if_newer(&mut self.press, &latest_measurement.press);
    }

    /// `true` si la température `idx` est restée dans une bande de
    /// `tolerance` sur les `window` précédant son échantillon le plus
    /// récent. Pas de paramètre `now` externe : la référence temporelle est
    /// l'échantillon le plus récent lui-même, pour rester appelable depuis
    /// `logic::cooling` sans lui donner accès à l'horloge.
    ///
    /// # Ce que « couvrir la fenêtre » veut dire
    ///
    /// Le critère porte donc sur l'étendue réelle des données : on remonte
    /// l'historique tant que les échantillons sont valides et dans la
    /// fenêtre, et la couverture n'est acquise que si l'on atteint un
    /// échantillon *antérieur* au bord de la fenêtre. Autrement dit, il
    /// faut une chaîne ininterrompue de lectures valides qui enjambe toute
    /// la fenêtre, vrai quelle que soit la cadence, et faux dès qu'il
    /// manque des données au milieu.
    ///
    /// Fail-closed sur les trous : un `NaN` (lecture jamais faite, ou case
    /// encore à sa valeur d'initialisation) interrompt le balayage et fait
    /// échouer la couverture. Sur une chambre sous haute tension, refuser
    /// d'avancer faute de données est le défaut sûr.
    pub fn is_temp_stable(&self, idx: usize, window: Duration, tolerance: Celsius) -> bool {
        if idx >= NUMBER_OF_TEMP_SENSOR {
            return false;
        }
        let Ok(newest) = self.temps[idx].get(0) else { return false };
        if newest.value.is_nan() {
            return false;
        }
        let cutoff = newest.time.saturating_sub(window);

        let mut n: usize = 0;
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        // Passe à `true` seulement si le balayage atteint un échantillon
        // antérieur au bord de la fenêtre, c'est *la* preuve de couverture.
        // Sortir de la boucle autrement (NaN, fin du buffer) le laisse à
        // `false`, et la fenêtre est déclarée non couverte.
        let mut spans_window = false;

        for i in 0..CONTROL_LOOP_HISTORY_SIZE {
            let Ok(m) = self.temps[idx].get(i) else { break };
            if m.value.is_nan() {
                break;
            }
            // Le ring buffer est ordonné du plus récent au plus ancien :
            // le premier échantillon hors fenêtre borne le balayage.
            if m.time < cutoff {
                spans_window = true;
                break;
            }
            min = min.min(m.value.0);
            max = max.max(m.value.0);
            n += 1;
        }

        // `n >= 2` : garde-fou contre une fenêtre nulle ou minuscule, que
        // `spans_window` seul laisserait passer avec un unique échantillon.
        spans_window && n >= 2 && (max - min) <= tolerance.0
    }

    /// Depuis combien de temps la sonde base-chambre n'a pas fourni de
    /// lecture. Pas de champ dédié à maintenir ailleurs : le ring buffer ne
    /// se met à jour que sur une lecture réussie (`push_if_newer` ignore
    /// les absences), son horodatage le plus récent EST la fraîcheur
    /// cherchée.
    ///
    /// Aucune lecture depuis le démarrage ⇒ l'horodatage vaut encore
    /// `Instant::from_micros(0)`, et le résultat est donc l'uptime complet :
    /// « infiniment périmé » du point de vue de l'appelant, sans cas
    /// particulier à traiter chez lui. C'est cette convention que la méthode
    /// encapsule, sa raison d'exister, plutôt que d'exposer l'horodatage
    /// brut et de laisser chaque appelant la réinventer.
    pub fn chamber_stale_duration(&self, now: Instant) -> Duration {
        let last_valid = self.temps[CHAMBER_TEMP_IDX]
            .get(0)
            .map(|m| m.time)
            .unwrap_or(Instant::from_micros(0));
        now.since(last_valid)
    }
}

fn push_if_newer<Unit: Copy, const N: usize>(
    dst: &mut [RingBuffer<Measurement<Unit>, N>], src: &[Option<Measurement<Unit>>],
) {
    for (d_buffer, s_data) in dst.iter_mut().zip(src.iter()) {
        let Some(s_value) = s_data else { continue; };

        match d_buffer.get(0) {
            Ok(newest) if !s_value.is_newer_than(&newest) => {}
            _ => d_buffer.push(*s_value),
        }
    }
}

impl<Tmp, Prs> Sensors<Tmp, Prs>
where
    Tmp: DeferredBatchSensor<Celsius, NUMBER_OF_TEMP_SENSOR>,
    Prs: BatchSensor<HectoPascal, NUMBER_OF_PRESSURE_SENSOR>,
{
    pub fn probe(&mut self, probing_plan: ProbingPlan) -> SensorSnapshot {
        let mut result = SensorSnapshot::default();

        if probing_plan.probe_temperature {
            // Une erreur ici laisse simplement la lecture différée absente
            let _ = self.temperature_source.start_conversion();
        }

        // Une lecture en erreur laisse la case à `None` (comme une absence
        // de mesure) plutôt que de paniquer le cœur de contrôle sur un défi
        // capteur transitoire — même traitement que `push_if_newer` pour une
        // donnée absente. `MeasurementHistory` garde alors la dernière
        // valeur connue (pas de régression), et `SafetyMonitor`/`temp_stable`
        // traitent déjà l'absence prolongée de donnée comme une alarme.
        if probing_plan.probe_pressure {
            for (slot, reading) in result.press.iter_mut().zip(self.pressure_source.read()) {
                *slot = reading.ok();
            }
        }

        if probing_plan.probe_temperature {
            for (slot, reading) in result.temps.iter_mut().zip(self.temperature_source.read_result()) {
                *slot = reading.ok();
            }
        }

        result
    }

    pub fn probe_all(&mut self) -> SensorSnapshot {
        self.probe(ProbingPlan::all())
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn at_secs(s: u64) -> Instant {
        Instant::from_micros(s * 1_000_000)
    }

    // ─── chamber_stale_duration ─────────────────────────────────────────

    #[test]
    fn chamber_stale_is_the_full_uptime_when_never_read() {
        let history = MeasurementHistory::new();
        assert_eq!(history.chamber_stale_duration(at_secs(60)), Duration::from_secs(60));
    }

    #[test]
    fn chamber_stale_reflects_last_valid_reading() {
        let mut history = MeasurementHistory::new();
        history.temps[CHAMBER_TEMP_IDX].push(Measurement::new(at_secs(10), Celsius(-20.0)));
        assert_eq!(history.chamber_stale_duration(at_secs(15)), Duration::from_secs(5));
    }

    #[test]
    fn chamber_stale_is_zero_right_after_a_reading() {
        let mut history = MeasurementHistory::new();
        history.temps[CHAMBER_TEMP_IDX].push(Measurement::new(at_secs(10), Celsius(-20.0)));
        assert!(history.chamber_stale_duration(at_secs(10)).is_zero());
    }

    // ─── is_temp_stable ─────────────────────────────────────────────────

    /// Remplit `CHAMBER_TEMP_IDX` avec `count` echantillons espaces de
    /// `step_s`, du plus ancien au plus recent, en terminant a `t = 0 +
    /// count * step_s`.
    fn fill_chamber(history: &mut MeasurementHistory, count: u64, step_s: u64, value_c: f32) {
        for i in 1..=count {
            history.temps[CHAMBER_TEMP_IDX]
                .push(Measurement::new(at_secs(i * step_s), Celsius(value_c)));
        }
    }

    #[test]
    fn stable_when_an_unbroken_chain_spans_the_window() {
        let mut history = MeasurementHistory::new();
        // 80 echantillons a 1 s : la chaine enjambe largement 60 s.
        fill_chamber(&mut history, 80, 1, -40.0);
        assert!(history.is_temp_stable(
            CHAMBER_TEMP_IDX, Duration::from_secs(60), Celsius(1.0)
        ));
    }

    #[test]
    fn unstable_when_the_data_does_not_reach_back_far_enough() {
        let mut history = MeasurementHistory::new();
        // 10 echantillons a 1 s : rien avant t = 1 s, donc la fenetre de
        // 60 s n'est pas couverte, meme si la valeur ne bouge pas.
        fill_chamber(&mut history, 10, 1, -40.0);
        assert!(!history.is_temp_stable(
            CHAMBER_TEMP_IDX, Duration::from_secs(60), Celsius(1.0)
        ));
    }

    /// Le point de la reecriture : la couverture ne suppose plus un
    /// echantillon par seconde. Quatre echantillons a 30 s d'intervalle
    /// enjambent une fenetre de 60 s — l'ancienne regle (80 % de 60
    /// echantillons) les aurait rejetes.
    #[test]
    fn coverage_does_not_assume_one_sample_per_second() {
        let mut history = MeasurementHistory::new();
        fill_chamber(&mut history, 4, 30, -40.0);
        assert!(history.is_temp_stable(
            CHAMBER_TEMP_IDX, Duration::from_secs(60), Celsius(1.0)
        ));
    }

    #[test]
    fn unstable_when_the_spread_exceeds_the_tolerance() {
        let mut history = MeasurementHistory::new();
        fill_chamber(&mut history, 80, 1, -40.0);
        history.temps[CHAMBER_TEMP_IDX].push(Measurement::new(at_secs(81), Celsius(-35.0)));
        assert!(!history.is_temp_stable(
            CHAMBER_TEMP_IDX, Duration::from_secs(60), Celsius(1.0)
        ));
    }

    #[test]
    fn a_nan_in_the_window_fails_closed() {
        let mut history = MeasurementHistory::new();
        fill_chamber(&mut history, 40, 1, -40.0);
        history.temps[CHAMBER_TEMP_IDX]
            .push(Measurement::new(at_secs(41), Celsius(f32::NAN)));
        fill_chamber_from(&mut history, 42, 80, -40.0);
        assert!(!history.is_temp_stable(
            CHAMBER_TEMP_IDX, Duration::from_secs(60), Celsius(1.0)
        ));
    }

    fn fill_chamber_from(history: &mut MeasurementHistory, from_s: u64, to_s: u64, value_c: f32) {
        for i in from_s..=to_s {
            history.temps[CHAMBER_TEMP_IDX]
                .push(Measurement::new(at_secs(i), Celsius(value_c)));
        }
    }

    #[test]
    fn a_fresh_history_is_never_stable() {
        let history = MeasurementHistory::new();
        assert!(!history.is_temp_stable(
            CHAMBER_TEMP_IDX, Duration::from_secs(60), Celsius(1.0)
        ));
    }

    #[test]
    fn an_out_of_range_index_is_never_stable() {
        let history = MeasurementHistory::new();
        assert!(!history.is_temp_stable(
            NUMBER_OF_TEMP_SENSOR, Duration::from_secs(60), Celsius(1.0)
        ));
    }
}
