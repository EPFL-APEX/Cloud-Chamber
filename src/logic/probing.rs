use crate::cloud_chamber_hal::sensors::{BatchSensor, DeferredBatchSensor, Sensors};
use crate::cloud_chamber_hal::measurement::Measurement;
use crate::cloud_chamber_hal::units::{Celsius, HectoPascal, Volt};
use crate::cloud_chamber_hal::config::{
    CHAMBER_TEMP_IDX, NUMBER_OF_TEMP_SENSOR, NUMBER_OF_PRESSURE_SENSOR, NUMBER_OF_VOLTMETER,
};
use crate::config::CONTROL_LOOP_HISTORY_SIZE;
use crate::shared::data::{SystemTask, SensorSnapshot};
use crate::cloud_chamber_hal::ring_buffer::RingBuffer;
use crate::cloud_chamber_hal::timer::Instant;

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
    pub probe_voltage: bool,
}

impl ProbingPlan {
    pub const fn new(probe_temperature: bool, probe_pressure: bool, probe_voltage: bool) -> Self {
        Self { probe_temperature, probe_pressure, probe_voltage }
    }

    pub const fn all() -> Self {
        Self { probe_temperature: true, probe_pressure: true, probe_voltage: true }
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
    // pub(crate) : indexation directe depuis logic::cooling/logic::stopping
    // via les constantes d'index de cloud_chamber_hal::config (pas
    // d'accesseurs par capteur, cf. décisions de conception de logic/).
    pub temps: [RingBuffer<Measurement<Celsius>, CONTROL_LOOP_HISTORY_SIZE>; NUMBER_OF_TEMP_SENSOR],
    pub press: [RingBuffer<Measurement<HectoPascal>, CONTROL_LOOP_HISTORY_SIZE>; NUMBER_OF_PRESSURE_SENSOR],
    pub volts: [RingBuffer<Measurement<Volt>, CONTROL_LOOP_HISTORY_SIZE>; NUMBER_OF_VOLTMETER],
}

impl MeasurementHistory {

    pub fn new() -> Self {
        let t0 = Instant::from_micros(0);
        let default_temp = Measurement::new(t0, Celsius(f32::NAN));
        let default_press = Measurement::new(t0, HectoPascal(f32::NAN));
        let default_volts = Measurement::new(t0, Volt(f32::NAN));
        Self {
            temps: core::array::from_fn(|_| RingBuffer::filled(default_temp)),
            press: core::array::from_fn(|_| RingBuffer::filled(default_press)),
            volts: core::array::from_fn(|_| RingBuffer::filled(default_volts)),
        }
    }


    pub fn update(&mut self, latest_measurement: &SensorSnapshot) {
        push_if_newer(&mut self.temps, &latest_measurement.temps);
        push_if_newer(&mut self.press, &latest_measurement.press);
        push_if_newer(&mut self.volts, &latest_measurement.volts);
    }

    /// `true` si la température `idx` est restée dans une bande de
    /// `tolerance` sur les `window_ms` précédant son échantillon le plus
    /// récent, avec une couverture de données suffisante (≥ 80% des
    /// échantillons attendus). Pas de paramètre `now` externe : la
    /// référence temporelle est l'échantillon le plus récent lui-même, pour
    /// rester appelable depuis `logic::cooling` sans lui donner accès à
    /// l'horloge.
    pub fn temp_stable(&self, idx: usize, window_ms: u64, tolerance: f32) -> bool {
        if idx >= NUMBER_OF_TEMP_SENSOR {
            return false;
        }
        let Ok(newest) = self.temps[idx].get(0) else { return false };
        if newest.value.0.is_nan() {
            return false;
        }
        let cutoff_ms = newest.time.as_millis().saturating_sub(window_ms);

        let mut n: usize = 0;
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for i in 0..CONTROL_LOOP_HISTORY_SIZE {
            let Ok(m) = self.temps[idx].get(i) else { break };
            if m.value.0.is_nan() || m.time.as_millis() < cutoff_ms {
                break;
            }
            min = min.min(m.value.0);
            max = max.max(m.value.0);
            n += 1;
        }

        let expected = (window_ms / 1_000) as usize;
        n >= expected.saturating_mul(4) / 5 && n >= 2 && (max - min) <= tolerance
    }

    /// Depuis combien de temps la sonde base-chambre n'a pas fourni de
    /// lecture. Pas de champ dédié à maintenir ailleurs : le ring buffer ne
    /// se met à jour que sur une lecture réussie (`push_if_newer` ignore
    /// les absences), son horodatage le plus récent EST la fraîcheur
    /// cherchée. `0` si jamais aucune lecture (buffer encore à sa valeur
    /// par défaut `Instant::from_micros(0)`) — traité comme "infiniment
    /// périmé" par l'appelant via `now_ms - 0`.
    pub fn chamber_stale_ms(&self, now_ms: u64) -> u64 {
        let last_valid_ms = self.temps[CHAMBER_TEMP_IDX]
            .get(0)
            .map(|m| m.time.as_millis())
            .unwrap_or(0);
        now_ms.saturating_sub(last_valid_ms)
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

impl<Tmp, Prs, Vlt> Sensors<Tmp, Prs, Vlt>
where
    Tmp: DeferredBatchSensor<Celsius, NUMBER_OF_TEMP_SENSOR>,
    Prs: BatchSensor<HectoPascal, NUMBER_OF_PRESSURE_SENSOR>,
    Vlt: BatchSensor<Volt, NUMBER_OF_VOLTMETER>,
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
        if probing_plan.probe_voltage {
            for (slot, reading) in result.volts.iter_mut().zip(self.voltage_source.read()) {
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

    #[test]
    fn chamber_stale_ms_is_infinite_when_never_read() {
        let history = MeasurementHistory::new();
        assert_eq!(history.chamber_stale_ms(60_000), 60_000);
    }

    #[test]
    fn chamber_stale_ms_reflects_last_valid_reading() {
        let mut history = MeasurementHistory::new();
        history.temps[CHAMBER_TEMP_IDX]
            .push(Measurement::new(Instant::from_micros(10_000_000), Celsius(-20.0))); // 10s
        assert_eq!(history.chamber_stale_ms(15_000), 5_000);
    }

    #[test]
    fn chamber_stale_ms_zero_right_after_a_reading() {
        let mut history = MeasurementHistory::new();
        history.temps[CHAMBER_TEMP_IDX]
            .push(Measurement::new(Instant::from_micros(10_000_000), Celsius(-20.0))); // 10s
        assert_eq!(history.chamber_stale_ms(10_000), 0);
    }
}
