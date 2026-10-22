//! Fonctions de régulation pures — hystérésis et PID.

use crate::cloud_chamber_hal::measurement::Measurement;
use crate::cloud_chamber_hal::ring_buffer::RingBuffer;
use crate::cloud_chamber_hal::units::Unit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegulationDirection {
    Upward,
    Downward,
}

pub fn hysteresis<U: Unit>(
    current: U,
    target: U,
    band: U,
    is_on: bool,
    direction: RegulationDirection,
) -> bool {
    let error = match direction {
        RegulationDirection::Upward => current - target,
        RegulationDirection::Downward => target - current,
    };

    if is_on { error > -band } else { error > band }
}

#[derive(Debug, Clone, Copy)]
pub struct PidGains {
    pub kp: f32,
    pub ki: f32,
    pub kd: f32,
}

pub fn pid<U: Unit, const N: usize>(
    target: U,
    history: &RingBuffer<Measurement<U>, N>,
    gains: PidGains,
) -> U {
    // Historique vide : pas de mesure, pas de commande — zéro, pas une
    // panique. Un `unwrap()` ici tuait la boucle de contrôle sur le
    // premier tour d'un capteur muet.
    let Some(newest) = history.get(0).ok() else {
        return U::zero();
    };

    let error_of = |m: Measurement<U>| m.value - target;

    let proportional = error_of(newest);
    let mut integral = U::zero();
    let mut derivative = U::zero();
    let mut previous = newest;

    for i in 1..N {
        // Le buffer peut contenir moins de N échantillons : le premier
        // `get` en erreur termine le balayage, il ne fait pas planter.
        let Ok(sample) = history.get(i) else { break };
        let dt_s = previous.time.since(sample.time).as_millis() as f32 / 1_000.0;
        integral += (error_of(previous) + error_of(sample)) * 0.5 * dt_s;
        if i == 1 && dt_s > 0.0 {
            derivative = (error_of(newest) - error_of(sample)) / dt_s;
        }
        previous = sample;
    }

    proportional * gains.kp + integral * gains.ki + derivative * gains.kd
}
