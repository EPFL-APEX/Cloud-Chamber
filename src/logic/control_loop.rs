//! All the logic for controlling the state of the chamber goes here

use core::todo;

use crate::{cloud_chamber_hal::{sensors::{BatchSensor, Sensors, DeferredBatchSensor}, units::{Celsius, HectoPascal, Volt}}, shared::data::{SHARED, SensorSnapshot, SystemTask},
};
use super::probing::MeasurementHistory;
use crate::logic::actuators::ActuatorPlan;
use crate::cloud_chamber_hal::config::{
    NUMBER_OF_TEMP_SENSOR, NUMBER_OF_PRESSURE_SENSOR, NUMBER_OF_VOLTMETER,
};

use defmt::panic;


/// Point d'entrée Core0 : boucle de sondage + machine à états.
///
/// Panique si un capteur ne retourne aucune mesure valide à l'initialisation
/// (cf. `are_all_some()` ci-dessous) — pas de démarrage dégradé pour l'instant.
pub fn run<Ts, Ps, Vs>(mut sensors: Sensors<Ts, Ps, Vs>) -> !
where
    Ts : DeferredBatchSensor<Celsius, NUMBER_OF_TEMP_SENSOR>,
    Ps : BatchSensor<HectoPascal, NUMBER_OF_PRESSURE_SENSOR>,
    Vs : BatchSensor<Volt, NUMBER_OF_VOLTMETER>,
{

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
        // TODO: react_to() retourne maintenant (SystemTask, ActuatorPlan) —
        // cette boucle doit migrer vers PhaseClock::new()/next_task() pour
        // gérer timeouts/délais et appliquer le plan via Actuators::apply().
        // Pas fait ici : ni le timer ni les Actuators ne sont encore
        // possédés par cette fonction (Sensors::new() ci-dessus est
        // lui-même déjà incomplet, hors périmètre de ce passage).
        let (next_task, plan) = current_task.react_to(&measurement_history);
        current_task = next_task;
        todo!();

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
    pub fn react_to(self, history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
        use SystemTask::*;
        match self {
            // Mode manuel pas encore codé (cf. plan de réconciliation) —
            // tout coupé par défaut plutôt qu'un todo!() qui paniquerait.
            Idle => (
                SystemTask::Idle,
                ActuatorPlan { compressor: false, iso_heater: false, high_voltage: false },
            ),
            Cooling(phase) => phase.react_to(history),
            // Régime permanent après la séquence de refroidissement : on
            // maintient les sorties de fin de FinalCheckBeforeStabilising.
            Stabilising => todo!(),
            Stopping(phase) => phase.react_to(history),
            Tripped(cause) => todo!(),
        }
    }
}
