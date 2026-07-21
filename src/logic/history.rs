//! Historique glissant des mesures — équivalent de MeasurementHistory de la
//! branche add-phase-transition-logic (logic/probing.rs), adapté aux lectures
//! de SystemState (pas de traits HAL : les capteurs sont lus dans main.rs).

use crate::config::CHAMBER_TEMP_IDX;
use crate::data::{SystemState, MAX_TEMP_SENSORS};
use crate::shared::ring_buffer::RingBuffer;

/// Nombre d'échantillons conservés par capteur (à 1 Hz ≈ 90 s d'historique).
pub const CONTROL_LOOP_HISTORY_SIZE: usize = 90;

/// Échantillon horodaté.
#[derive(Debug, Clone, Copy, Default)]
pub struct TimedSample {
    pub t_ms:  u64,
    pub value: f32,
}

/// Historique par capteur, alimenté à 1 Hz depuis SystemState.
pub struct MeasurementHistory {
    temps:     [RingBuffer<TimedSample, CONTROL_LOOP_HISTORY_SIZE>; MAX_TEMP_SENSORS],
    amb_temp:  RingBuffer<TimedSample, CONTROL_LOOP_HISTORY_SIZE>,
    amb_press: RingBuffer<TimedSample, CONTROL_LOOP_HISTORY_SIZE>,
    last_push_ms: u64,
}

impl MeasurementHistory {
    pub fn new() -> Self {
        Self {
            temps:     core::array::from_fn(|_| RingBuffer::new()),
            amb_temp:  RingBuffer::new(),
            amb_press: RingBuffer::new(),
            last_push_ms: 0,
        }
    }

    /// À appeler régulièrement (p. ex. dans la boucle de contrôle) —
    /// n'enregistre au plus qu'un échantillon par seconde.
    pub fn update(&mut self, state: &SystemState, now_ms: u64) {
        if now_ms.saturating_sub(self.last_push_ms) < 1_000 {
            return;
        }
        self.last_push_ms = now_ms;

        for (i, reading) in state.temperatures.iter().enumerate() {
            if reading.valid {
                self.temps[i].push(TimedSample { t_ms: now_ms, value: reading.value });
            }
        }
        if state.bme280.valid {
            self.amb_temp.push(TimedSample { t_ms: now_ms, value: state.bme280.temp_c });
            self.amb_press.push(TimedSample { t_ms: now_ms, value: state.bme280.pressure_hpa });
        }
    }

    /// Dernière valeur connue du capteur `idx` (None si jamais lu).
    pub fn latest_temp(&self, idx: usize) -> Option<TimedSample> {
        if idx >= MAX_TEMP_SENSORS { return None; }
        self.temps[idx].get(0).ok()
    }

    /// `true` si le capteur `idx` est resté dans une bande de `tolerance`
    /// pendant toute la fenêtre `window_ms`, avec une couverture de données
    /// suffisante (≥ ~80 % d'échantillons attendus à 1 Hz).
    pub fn temp_stable(&self, idx: usize, window_ms: u64, tolerance: f32, now_ms: u64) -> bool {
        if idx >= MAX_TEMP_SENSORS { return false; }
        let cutoff = now_ms.saturating_sub(window_ms);

        let mut n: usize = 0;
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;

        for i in 0..CONTROL_LOOP_HISTORY_SIZE {
            let Ok(s) = self.temps[idx].get(i) else { break };
            if s.t_ms < cutoff { break; }
            if s.value < min { min = s.value; }
            if s.value > max { max = s.value; }
            n += 1;
        }

        let expected = (window_ms / 1_000) as usize;
        n >= expected.saturating_mul(4) / 5 && n >= 2 && (max - min) <= tolerance
    }

    /// Raccourci : température de la base chambre (ds4).
    pub fn chamber_temp(&self) -> Option<f32> {
        self.latest_temp(CHAMBER_TEMP_IDX).map(|s| s.value)
    }
}
