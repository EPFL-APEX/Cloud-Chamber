//! All the logic for controlling the state of the chamber goes here

use crate::shared::data::{SensorSnapshot, SHARED};
use super::probing::ProbingPlan;

use defmt::panic;


fn measure_placeholder(plan:ProbingPlan) -> SensorSnapshot {
    todo!()
}

fn probe_every_sensor() -> SensorSnapshot {
    todo!()
}

fn update_global_state(latest_measurements:&SensorSnapshot) {
    critical_section::with(|cs| {
        let data = SHARED.borrow_ref_mut(cs);
        data.snapshot = latest_measurements.clone();
        data.new_data = true;
    });
}

/// 
/// 
/// Panic if ...
pub fn run() -> ! {

    // Initialisation
    let latest_measurement = probe_every_sensor();
    if !latest_measurement.are_all_valid() {panic!("Not every sensor returned a valid measurement, something goes wrong...")};

    update_global_state(&latest_measurement);


    // Control loop
    loop {
        latest_measurement = measure_placeholder();


    }
}