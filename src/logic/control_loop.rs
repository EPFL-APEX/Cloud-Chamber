//! Point d'entrée Core0 : boucle de sondage + machine à états + actionneurs.

use crate::cloud_chamber_hal::{
    actuators::{ActuatorPlan, Actuators, BinaryActuator},
    config::{NUMBER_OF_PRESSURE_SENSOR, NUMBER_OF_TEMP_SENSOR, NUMBER_OF_VOLTMETER},
    sensors::{BatchSensor, DeferredBatchSensor, Sensors},
    timer::MonotonicTimer,
    units::{Celsius, HectoPascal, Volt},
};
use crate::logic::phase_clock::{PhaseClock, advance};
use crate::logic::security::{SafetyConfig, SafetyMonitor};
use crate::shared::data::{SHARED_STATE, SensorSnapshot, SystemTask};

use super::probing::MeasurementHistory;

/// Point d'entrée Core0 : boucle de sondage + machine à états.
///
/// Panique si un capteur ne retourne aucune mesure valide à l'initialisation
/// (cf. `are_all_some()` ci-dessous) — pas de démarrage dégradé pour l'instant.
pub fn run<Ts, Ps, Vs, Hv, Comp, Iso, Clk>(
    mut sensors: Sensors<Ts, Ps, Vs>, mut actuators: Actuators<Hv, Comp, Iso>, clock: Clk,
) -> !
where
    Ts: DeferredBatchSensor<Celsius, NUMBER_OF_TEMP_SENSOR>,
    Ps: BatchSensor<HectoPascal, NUMBER_OF_PRESSURE_SENSOR>,
    Vs: BatchSensor<Volt, NUMBER_OF_VOLTMETER>,
    Hv: BinaryActuator,
    Comp: BinaryActuator,
    Iso: BinaryActuator,
    Clk: MonotonicTimer,
{
    // Initial values, mais est-ce qu'on veut vraiment ça ?
    let mut latest_measurement = sensors.probe_all();
    if !latest_measurement.are_all_some() {panic!("Not every sensor returned a valid measurement, something goes wrong...")};

    update_global_state(&latest_measurement);

    // History
    let mut measurement_history = MeasurementHistory::new();
    measurement_history.update(&latest_measurement);

    // État de phase + sécurité — démarre à Idle, aucun cycle en cours tant
    // que rien ne force une transition (pas câblé ici : le déclenchement
    // vient de l'UI, câblage prévu séparément).
    let mut phase = PhaseClock::new(clock, SystemTask::default());
    let mut safety = SafetyMonitor::new(SafetyConfig::default(), phase.now_ms());

    // Probing plan
    let mut probing_plan = phase.current().create_probing_plan(&measurement_history);

    // Control loop
    loop {
        latest_measurement = sensors.probe(probing_plan);
        if !latest_measurement.are_all_none() {
            update_global_state(&latest_measurement);
        };
        measurement_history.update(&latest_measurement);

        // Adopte une écriture externe (UI) survenue depuis le tour précédent ;
        // no-op si SHARED_STATE contient encore exactement ce que ce
        // contrôleur y a lui-même écrit la dernière fois — donc aucune
        // transition autonome décidée ci-dessous (abandon, timeout, fin de
        // cycle...) ne peut être écrasée par une valeur restée en retard.
        phase.set(read_task());
        let synced_task = phase.current();

        // Sécurité en priorité absolue sur la logique de phase — décision
        // explicite ici (orchestration), pas cachée dans une méthode.
        let (next, plan) = if let Some(cause) = safety.check(&measurement_history, phase.now_ms()) {
            SystemTask::Tripped(cause).react_to(&measurement_history)
        } else {
            advance(
                phase.current(), &measurement_history,
                phase.elapsed_ms(), measurement_history.chamber_stale_ms(phase.now_ms()),
            )
        };
        actuators.apply(plan);

        // Publie et adopte localement `next` seulement si SHARED_STATE
        // vaut encore `synced_task` (lu en tout début de tour) — sinon
        // l'utilisateur a forcé la machine dans un autre état pendant le
        // calcul de ce tour : `next` (basé sur une lecture désormais
        // périmée) est abandonné, le tour suivant repartira de ce que
        // SHARED_STATE contient réellement via la ligne d'adoption ci-dessus.
        if publish_task_if_unchanged(synced_task, next) {
            phase.set(next);
        }

        probing_plan = phase.current().create_probing_plan(&measurement_history);
    }
}


/// Can be expensive due to using the critical section so avoid using it if there is no update.
fn update_global_state(latest_measurement:&SensorSnapshot) {
    critical_section::with(|cs| {
        let mut shared_state = SHARED_STATE.borrow_ref_mut(cs);
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

/// Publie `next` dans `SHARED_STATE.task` uniquement si sa valeur
/// actuelle vaut encore `expected` (lu en début de tour) — lecture et
/// écriture dans la même section critique, pour empêcher une écriture
/// externe (UI) de s'intercaler entre le contrôle et la publication.
/// Retourne `true` si la publication a eu lieu.
fn publish_task_if_unchanged(expected: SystemTask, next: SystemTask) -> bool {
    critical_section::with(|cs| {
        let mut shared = SHARED_STATE.borrow_ref_mut(cs);
        if shared.task == expected {
            shared.task = next;
            true
        } else {
            false
        }
    })
}

/// Relit `task` — seul moyen de détecter une écriture externe (UI)
/// survenue depuis le tour précédent, cf. `run()`.
fn read_task() -> SystemTask {
    critical_section::with(|cs| SHARED_STATE.borrow_ref(cs).task)
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
            // Pas de sortie automatique — seul un arrêt opérateur explicite
            // (signal UI, pas câblé ici) fait sortir de cet état.
            Stabilising => (
                SystemTask::Stabilising,
                ActuatorPlan { compressor: true, iso_heater: true, high_voltage: true },
            ),
            Stopping(phase) => phase.react_to(history),
            // Coupure de secours : tout éteint, verrouillé jusqu'au
            // réarmement opérateur explicite (`SafetyMonitor::reset`, appelé
            // depuis `control_loop.rs::run()` en priorité absolue).
            Tripped(cause) => (
                SystemTask::Tripped(cause),
                ActuatorPlan { compressor: false, iso_heater: false, high_voltage: false },
            ),
        }
    }
}
