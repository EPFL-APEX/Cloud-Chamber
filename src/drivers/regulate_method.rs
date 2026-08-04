//! Fonctions de régulation pures — hystérésis et PID.
use core::ops::{Add, Sub, Neg, Mul, Div};

use crate::cloud_chamber_hal::actuators::{BinaryActuator, TargetActuator};
use crate::cloud_chamber_hal::measurement::Measurement;
use crate::cloud_chamber_hal::ring_buffer::RingBuffer;


pub enum RegulationDirection {
    Upward,
    Downward,
}


pub fn hysteresis<Unit>(current: Unit, target: Unit, band: Unit, is_on: bool, direction:Regu
    ) -> bool
where
    Unit: Copy + Sub<Output = Unit> + Neg<Output = Unit> + PartialOrd,
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

pub fn pid<Unit, const N: usize>(
    target: Unit, history: &RingBuffer<Measurement<Unit>, N>, gains: PidGains,
) -> f32
where
    Unit: Copy + Add<Output = Unit> + Sub<Output = Unit> + Mul<f32, Output = Unit> + Div<f32, Output = Unit> + PartialOrd,
{
    let newest = history.get(0).unwrap();

    let error_of = |m: Measurement<Unit>| (m.value - target);

    let proportional = error_of(newest).into();
    let mut integral = Unit::new(0.0);
    let mut derivative = Unit::new(0.0);
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

    gains.kp * proportional + gains.ki * integral + gains.kd * derivative
}

