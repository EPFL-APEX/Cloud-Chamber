use core::fmt::Debug;

use crate::cloud_chamber_hal::sensors::{PressureSensor, Sensor, Sensors, TemperatureSensor, VoltageSensor};
use crate::config::{
    NUMBER_OF_TEMP_SENSOR, NUMBER_OF_PRESSURE_SENSOR,
    NUMBER_OF_VOLTMETER, CONTROL_LOOP_HISTORY_SIZE
};
use crate::shared::{
    data::{SystemTask, TimeStamped, TemperatureReading, PressureReading, VoltsReading, SensorSnapshot},
    ring_buffer::RingBuffer
};
use crate::cloud_chamber_hal::timer::Instant;

#[derive(Clone, Copy, Debug)]
pub struct ProbingPlan {
    temp_sensor_mask:u8,
    pressure_sensor_mask:u8,
    voltmeter_mask:u8,
}

impl ProbingPlan {
    pub const fn new() -> Self {
        todo!()
    }

    pub const fn all() -> Self {
        Self { 
            temp_sensor_mask: 0b1111_1111,
            pressure_sensor_mask: 0b1111_1111,
            voltmeter_mask: 0b1111_1111
        }
    }

    pub const fn should_probe<TemperatureSensor>(&self, id:u8) -> bool {
        self.temp_sensor_mask & (1 << id) != 0
    }

    pub fn should_probe<PressureSensor>(&self, id:u8) -> bool {
        self.pressure_sensor_mask & (1 << id) != 0
    }

    pub fn should_probe<VoltageSensor>(&self, id:u8) -> bool {
        self.voltmeter_mask & (1 << id) != 0
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
    temps: [RingBuffer<TemperatureReading, CONTROL_LOOP_HISTORY_SIZE>; NUMBER_OF_TEMP_SENSOR],
    press: [RingBuffer<PressureReading, CONTROL_LOOP_HISTORY_SIZE>; NUMBER_OF_PRESSURE_SENSOR],
    volts: [RingBuffer<VoltsReading, CONTROL_LOOP_HISTORY_SIZE>; NUMBER_OF_VOLTMETER],
}

impl MeasurementHistory {

    pub fn new() -> Self {
        let t0 = Instant::new(0);
        let default_temp = TemperatureReading {time: t0, value: f32::NAN};
        let default_press = PressureReading {time: t0, value: f32::NAN};
        let default_volts = VoltsReading {time: t0, value: f32::NAN};
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

fn push_if_newer<T: Copy + TimeStamped, const N: usize>(dst: &mut [RingBuffer<T, N>], src: &[Option<T>]) {
    for (d_buffer, s_data) in dst.iter_mut().zip(src.iter()) {
        let Some(s_value) = s_data else { continue; };

        match d_buffer.get(0) {
            Ok(newest) if !s_value.get_instant().is_newer_than(newest.get_instant()) => {}
            _ => d_buffer.push(*s_value),
        }
    }
}

impl<T: TemperatureSensor, P: PressureSensor, V: VoltageSensor> Sensors<T, P, V> {
    pub fn probe(&mut self, probing_plan: ProbingPlan) -> SensorSnapshot {
        let mut result = SensorSnapshot::default();

        probe_and_insert(&mut self.temperature_sensors, &probing_plan, &mut result.temps);
        probe_and_insert(&mut self.pressure_sensors, &probing_plan, &mut result.press);
        probe_and_insert(&mut self.voltage_sensors, &probing_plan, &mut result.volts);

        result
    }

    pub fn probe_all(&mut self) -> SensorSnapshot {
        self.probe(ProbingPlan::all())
    }
}

fn probe_and_insert<S: Sensor<T>, T: Debug>(sensors: &mut [S], plan: &ProbingPlan, dest: &mut [Option<T>]) -> Result<(), (usize, S::Error)>{
    for (id, (sensor, d_val)) in sensors.iter_mut().zip(dest.iter_mut()).enumerate() {
        if !plan.should_probe<T>(id) {continue;}
        
        let result = sensor.read();
        if result.is_err() { return Err((id, result.unwrap_err())) };

        *d_val = Some(result.unwrap());
    };

    Ok(())
}