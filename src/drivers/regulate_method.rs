//! Fonctions de régulation pures — hystérésis et PID.
use core::ops::{Add, AddAssign, Div, Mul, Neg, Sub};

use crate::cloud_chamber_hal::actuators::{BinaryActuator, TargetActuator};
use crate::cloud_chamber_hal::measurement::Measurement;
use crate::cloud_chamber_hal::ring_buffer::RingBuffer;

/// Tout ce dont `hysteresis`/`pid` ont besoin sur l'unité physique régulée
pub trait Unit:
    Copy
    + PartialOrd
    + Add<Output = Self>
    + AddAssign
    + Sub<Output = Self>
    + Neg<Output = Self>
    + Mul<f32, Output = Self>
    + Div<f32, Output = Self>
{
    /// Valeur neutre pour initialiser un accumulateur (intégrale/dérivée).
    fn zero() -> Self;
}

pub enum RegulationDirection {
    Upward,
    Downward,
}


pub fn hysteresis<U: Unit>(current: U, target: U, band: U, is_on: bool, direction: RegulationDirection
    ) -> bool
{
    let error = match direction {
        RegulationDirection::Upward => (current - target),
        RegulationDirection::Downward => (target - current),
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
    target: U, history: &RingBuffer<Measurement<U>, N>, gains: PidGains,
) -> U
{
    let newest = history.get(0).unwrap();

    let error_of = |m: Measurement<U>| (m.value - target);

    let proportional = error_of(newest);
    let mut integral = U::zero();
    let mut derivative = U::zero();
    let mut previous = newest;

    for i in 1..N {
        let sample = history.get(i).unwrap();
        let dt_s = previous.time.since(sample.time).as_millis() as f32 / 1_000.0;
        integral += (error_of(previous) + error_of(sample)) * 0.5 * dt_s;
        if i == 1 && dt_s > 0.0 {
            derivative = (error_of(newest) - error_of(sample)) / dt_s;
        }
        previous = sample;
    }

    proportional * gains.kp + integral * gains.ki + derivative * gains.kd
}
