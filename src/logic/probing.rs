use crate::cloud_chamber_hal::sensors::{BatchSensor, DeferredBatchSensor, Measurement, Sensors};
use crate::cloud_chamber_hal::units::{Celsius, HectoPascal, Volt};
use crate::config::{
    NUMBER_OF_TEMP_SENSOR, NUMBER_OF_PRESSURE_SENSOR,
    NUMBER_OF_VOLTMETER, CONTROL_LOOP_HISTORY_SIZE
};
use crate::shared::{
    data::{SystemTask, SensorSnapshot},
    ring_buffer::RingBuffer
};
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
        match self {
            SystemTask::Idle => todo!(),
            SystemTask::Cooling(phase) => phase.create_probing_plan(sys_hist),
            SystemTask::Stabilising => todo!(),
            SystemTask::Stopping(phase) => phase.create_probing_plan(sys_hist),
        }
    }
}

#[derive(Debug)]
pub struct MeasurementHistory {
    temps: [RingBuffer<Measurement<Celsius>, CONTROL_LOOP_HISTORY_SIZE>; NUMBER_OF_TEMP_SENSOR],
    press: [RingBuffer<Measurement<HectoPascal>, CONTROL_LOOP_HISTORY_SIZE>; NUMBER_OF_PRESSURE_SENSOR],
    volts: [RingBuffer<Measurement<Volt>, CONTROL_LOOP_HISTORY_SIZE>; NUMBER_OF_VOLTMETER],
}

impl MeasurementHistory {

    pub fn new() -> Self {
        let t0 = Instant::new(0);
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
            self.temperature_source.start_conversion();
        }

        if probing_plan.probe_pressure {
            for (slot, reading) in result.press.iter_mut().zip(self.pressure_source.read()) {
                if reading.is_ok() {
                    *slot = Some(reading.unwrap());
                } else {
                    todo!("Error handling for pressure probing");
                }
            }
        }
        if probing_plan.probe_voltage {
            for (slot, reading) in result.volts.iter_mut().zip(self.voltage_source.read()) {
                if reading.is_ok() {
                    *slot = reading.ok();
                } else {
                    todo!("Error handling for voltage probing");
                }
            }
        }
        
        if probing_plan.probe_temperature {
            for (slot, reading) in result.temps.iter_mut().zip(self.temperature_source.read_result()) {
                if reading.is_ok() {
                    *slot = reading.ok();
                } else {
                    todo!("Error handling for temperature probing");
                }
            }
        }

        result
    }

    pub fn probe_all(&mut self) -> SensorSnapshot {
        self.probe(ProbingPlan::all())
    }
}