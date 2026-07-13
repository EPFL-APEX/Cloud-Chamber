//! All the logic for controlling the state of the chamber goes here

use crate::shared::{
    data::{PressureReading, SHARED, SensorSnapshot, SystemTask, TemperatureReading, TimeStamped, VoltsReading},
    ring_buffer::RingBuffer,
};

use super::probing::ProbingPlan;

use crate::config::{
    CONTROL_LOOP_HISTORY_SIZE,
    NUMBER_OF_TEMP_SENSOR, NUMBER_OF_VOLTMETER,
    NUMBER_OF_PRESSURE_SENSOR, 
};

use defmt::panic;

/// 
/// 
/// Panic if ...
pub fn run() -> ! {

    // Sensor Init
    let latest_measurement = probe_every_sensor();
    if !latest_measurement.are_all_valid() {panic!("Not every sensor returned a valid measurement, something goes wrong...")};

    update_global_state(&latest_measurement);
    
    // History
    let mut measurement_history: MeasurementHistory = MeasurementHistory::new();
    measurement_history.update(&latest_measurement);

    // Task Init
    let current_task = SystemTask::default();
    

    // Control loop
    loop {
        latest_measurement = measure_placeholder();
        if latest_measurement.has_new_data() {
            update_global_state(&latest_measurement);
        };
        measurement_history.update(&latest_measurement);
        current_task.react_to(&measurement_history);
    }
}


fn measure_placeholder(plan:ProbingPlan) -> SensorSnapshot {
    todo!()
}

fn probe_every_sensor() -> SensorSnapshot {
    todo!()
}


/// Can be expensive due to using the critical section so avoid using it if there is no update.
fn update_global_state(latest_measurement:&SensorSnapshot) {
    critical_section::with(|cs| {
        let mut shared_state = SHARED.borrow_ref_mut(cs);
        let mut shared_sensor_data = &shared_state.snapshot;
        
        merge_new_readings(&mut shared_sensor_data.temps, &latest_measurement.temps);
        merge_new_readings(&mut shared_sensor_data.press, &latest_measurement.press);
        merge_new_readings(&mut shared_sensor_data.volts, &latest_measurement.volts);
        
        shared_state.new_data = true;
    });
}

fn merge_new_readings<T: Copy>(dst: &mut [Option<T>], src: &[Option<T>]) {
    for (d_item, s_item) in dst.iter_mut().zip(src.iter()) {
        if s_item.is_some() {
            *d_item = *s_item;
        }
    }
}


fn get_current_task() -> SystemTask {
    critical_section::with(|cs| {
        SHARED.borrow_ref(cs).system_state
    })
}


impl SystemTask {
    fn react_to(&self, current_state: &MeasurementHistory) {
        match self {
            SystemTask::Idle => { todo!() }
            SystemTask::Cooling(phase) => { todo!() }
            SystemTask::Stabilising => { todo!() }
            SystemTask::Stopping(phase) => { todo!() }
        }
    }
}


#[derive(Debug)]
struct MeasurementHistory {
    temps: [RingBuffer<TemperatureReading, CONTROL_LOOP_HISTORY_SIZE>; NUMBER_OF_TEMP_SENSOR],
    press: [RingBuffer<PressureReading, CONTROL_LOOP_HISTORY_SIZE>; NUMBER_OF_PRESSURE_SENSOR],
    volts: [RingBuffer<VoltsReading, CONTROL_LOOP_HISTORY_SIZE>; NUMBER_OF_VOLTMETER],
}

impl MeasurementHistory {
    fn update(&mut self, latest_measurement: &SensorSnapshot) {
        push_if_newer(&mut self.temps, &latest_measurement.temps);
        push_if_newer(&mut self.press, &latest_measurement.press);
        push_if_newer(&mut self.volts, &latest_measurement.volts);
    }
}

fn push_if_newer<T: Copy + TimeStamped, const N: usize>(dst: &mut [RingBuffer<T, N>], src: &[Option<T>]) {
    for (d_buffer, s_data) in dst.iter_mut().zip(src.iter()) {
        if s_data.is_none() {continue;}
        
        let newest_buffer_item = d_buffer.get(0);

        if newest_buffer_item.is_err() {continue;}
        
        let newest_buffer_item_instant = d_buffer.get(0).unwrap().get_instant();
        let s_data_instant = s_data.unwrap().get_instant();

        if s_data_instant.is_newer_than(newest_buffer_item_instant) {
            d_buffer.push(s_data.unwrap());
        }
    }
}