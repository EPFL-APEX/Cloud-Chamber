//! All the logic for controlling the state of the chamber goes here

use crate::{cloud_chamber_hal::sensors::Sensors, 
    shared::data::{SHARED, SensorSnapshot, SystemTask},
};

use super::probing::MeasurementHistory;

use defmt::panic;

/// 
/// 
/// Panic if ...
pub fn run() -> ! {

    // Sensor Init
    let mut sensors = Sensors::new();

    // Initial values, mais est-ce qu'on veut vraiment ça ?
    let mut latest_measurement = sensors.probe_all();
    if !latest_measurement.are_all_some() {panic!("Not every sensor returned a valid measurement, something goes wrong...")};

    update_global_state(&latest_measurement);
    
    // History
    let mut measurement_history  = MeasurementHistory::new();
    measurement_history.update(&latest_measurement);

    // Task Init
    let mut current_task = SystemTask::default();

    // Probing plan
    let mut probing_plan = current_task.create_probing_plan(&measurement_history);
    

    // Control loop
    loop {
        latest_measurement = sensors.probe(probing_plan);
        if !latest_measurement.are_all_none() {
            update_global_state(&latest_measurement);
        };
        measurement_history.update(&latest_measurement);

        current_task = get_current_task();
        current_task = current_task.react_to(&measurement_history);
        
        probing_plan = current_task.create_probing_plan(&measurement_history);
    }
}


/// Can be expensive due to using the critical section so avoid using it if there is no update.
fn update_global_state(latest_measurement:&SensorSnapshot) {
    critical_section::with(|cs| {
        let mut shared_state = SHARED.borrow_ref_mut(cs);
        let mut shared_sensor_data = &mut shared_state.snapshot;
        
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
    pub fn react_to(self, current_state: &MeasurementHistory) -> SystemTask {
        match self {
            SystemTask::Idle => { todo!() }
            SystemTask::Cooling(phase) => phase.react_to(current_state),
            SystemTask::Stabilising => { todo!() }
            SystemTask::Stopping(phase) => { todo!() }
        }
    }
}
