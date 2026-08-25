//! Point d'entrée Core0 : boucle de sondage + machine à états + actionneurs.

use crate::{cloud_chamber_hal::{
    actuators::{ActuatorPlan, Actuators, BinaryActuator, TargetActuator}, config::{CHAMBER_TEMP_IDX, ISO_TEMP_IDX, NUMBER_OF_PRESSURE_SENSOR, NUMBER_OF_TEMP_SENSOR}, sensors::{BatchSensor, DeferredBatchSensor, Sensors}, timer::MonotonicTimer, units::{Celsius, HectoPascal},
}};
use crate::logic::timing::CONTROL_LOOP_HISTORY_SIZE;
use crate::logic::phase_clock::{PhaseClock, advance};
use crate::logic::security::{SafetyConfig, SafetyMonitor};
use crate::shared::data::{SHARED_STATE, SensorSnapshot, SystemTask};
use crate::shared::settings;

use super::probing::{MeasurementHistory, ProbingPlan};

/// Point d'entrée Core0 : boucle de sondage + machine à états.
///
/// Panique si un capteur ne retourne aucune mesure valide à l'initialisation
/// (cf. `are_all_some()` ci-dessous) — pas de démarrage dégradé pour l'instant.
///
/// Cette boucle possède le cœur sur lequel elle tourne et n'en rend jamais
/// la main. Sur la carte réelle elle occupe le cœur 1, l'UI gardant le
/// cœur 0 (cf. `src/main.rs`).
pub fn run<Ts, Ps, Hv, Cool, Iso, Pump, Lights, Glass, Clk>(
    mut sensors: Sensors<Ts, Ps>,
    mut actuators: Actuators<Hv, Cool, Iso, Pump, Lights, Glass>,
    clock: Clk,
) -> !
where
    Ts: DeferredBatchSensor<Celsius, NUMBER_OF_TEMP_SENSOR>,
    Ps: BatchSensor<HectoPascal, NUMBER_OF_PRESSURE_SENSOR>,
    Hv: BinaryActuator,
    Cool: TargetActuator<Celsius, CONTROL_LOOP_HISTORY_SIZE>,
    Iso: TargetActuator<Celsius, CONTROL_LOOP_HISTORY_SIZE>,
    Pump: BinaryActuator,
    Lights: BinaryActuator,
    Glass: BinaryActuator,
    Clk: MonotonicTimer,
{
    // Initial values, mais est-ce qu'on veut vraiment ça ?
    let latest_measurement = sensors.probe_all();
    if !latest_measurement.are_all_some() {
        // Le message d'origine ne disait pas *lequel* manquait, ce qui est
        // pourtant la seule information utile : elle désigne le faisceau à
        // aller vérifier. Un masque de bits plutôt qu'un libellé par
        // capteur — `logic/` ne connaît pas le câblage, et ce module reste
        // sans dépendance à une pile de traces pour rester testable sur
        // hôte.
        panic!(
            "Demarrage impossible : capteurs muets au premier sondage. \
             Temperatures manquantes (bit i = index i) : {:#b} sur {}, \
             pressions : {:#b} sur {}.",
            missing_mask(&latest_measurement.temps),
            NUMBER_OF_TEMP_SENSOR,
            missing_mask(&latest_measurement.press),
            NUMBER_OF_PRESSURE_SENSOR,
        );
    }

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
        probing_plan = tick(
            &mut sensors, &mut actuators, &mut phase, &mut safety,
            &mut measurement_history, probing_plan,
        );
    }
}

/// Un tour de la boucle de contrôle : sondage, réconciliation avec
/// `SHARED_STATE`, décision (sécurité en priorité absolue, sinon logique de
/// phase), application des actionneurs, publication. Extrait de `run()`
/// uniquement pour être testable (`run()` ne retourne jamais) — même
/// contenu que l'ancien corps de `loop`, aucun changement de comportement.
fn tick<Ts, Ps, Hv, Cool, Iso, Pump, Lights, Glass, Clk>(
    sensors: &mut Sensors<Ts, Ps>,
    actuators: &mut Actuators<Hv, Cool, Iso, Pump, Lights, Glass>,
    phase: &mut PhaseClock<Clk>,
    safety: &mut SafetyMonitor,
    measurement_history: &mut MeasurementHistory,
    probing_plan: ProbingPlan,
) -> ProbingPlan
where
    Ts: DeferredBatchSensor<Celsius, NUMBER_OF_TEMP_SENSOR>,
    Ps: BatchSensor<HectoPascal, NUMBER_OF_PRESSURE_SENSOR>,
    Hv: BinaryActuator,
    Cool: TargetActuator<Celsius, CONTROL_LOOP_HISTORY_SIZE>,
    Iso: TargetActuator<Celsius, CONTROL_LOOP_HISTORY_SIZE>,
    Pump: BinaryActuator,
    Lights: BinaryActuator,
    Glass: BinaryActuator,
    Clk: MonotonicTimer,
{
    let latest_measurement = sensors.probe(probing_plan);
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
    let (next, plan) = if let Some(cause) = safety.check(measurement_history, phase.now_ms()) {
        SystemTask::Tripped(cause).react_to(measurement_history)
    } else {
        advance(
            phase.current(), measurement_history,
            phase.elapsed_ms(), measurement_history.chamber_stale_ms(phase.now_ms()),
        )
    };
    actuators.apply(plan, measurement_history);

    // Publie et adopte localement `next` seulement si SHARED_STATE
    // vaut encore `synced_task` (lu en tout début de tour) — sinon
    // l'utilisateur a forcé la machine dans un autre état pendant le
    // calcul de ce tour : `next` (basé sur une lecture désormais
    // périmée) est abandonné, le tour suivant repartira de ce que
    // SHARED_STATE contient réellement via la ligne d'adoption ci-dessus.
    if publish_task_if_unchanged(synced_task, next) {
        phase.set(next);
    }

    phase.current().create_probing_plan(measurement_history)
}


/// Masque des index sans mesure : bit `i` à 1 = capteur `i` muet.
///
/// Sert au diagnostic de démarrage ci-dessus.
fn missing_mask<T>(readings: &[Option<T>]) -> u32 {
    let mut mask = 0;
    for (i, reading) in readings.iter().enumerate() {
        if reading.is_none() {
            mask |= 1 << i;
        }
    }
    mask
}

/// Can be expensive due to using the critical section so avoid using it if there is no update.
fn update_global_state(latest_measurement:&SensorSnapshot) {
    critical_section::with(|cs| {
        let mut shared_state = SHARED_STATE.borrow_ref_mut(cs);
        let shared_sensor_data = &mut shared_state.snapshot;

        merge_new_readings(&mut shared_sensor_data.temps, &latest_measurement.temps);
        merge_new_readings(&mut shared_sensor_data.press, &latest_measurement.press);

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
            // `iso_pump`/`lights`/`glass_heater` : toujours `false` ci-dessous,
            // cf. commentaire équivalent dans `logic::cooling` — aucune
            // politique par phase définie pour l'instant.
            Idle => (
                SystemTask::Idle,
                ActuatorPlan {
                    cooling: None, iso_heater: None, high_voltage: false,
                    iso_pump: false, lights: None, glass_heater: false,
                },
            ),
            Cooling(phase) => phase.react_to(history),
            // Régime permanent après la séquence de refroidissement : les
            // cibles restent celles de fin de FinalCheckBeforeStabilising,
            // désormais réellement régulées (cycle on/off autour de la
            // cible) plutôt que "tout allumé en continu" — plus réaliste
            // pour un compresseur en régime permanent. Pas de sortie
            // automatique — seul un arrêt opérateur explicite (signal UI,
            // pas câblé ici) fait sortir de cet état.
            Stabilising => {
                let settings = settings::get();
                (
                    SystemTask::Stabilising,
                    ActuatorPlan {
                        cooling: Some(settings.saturation_target),
                        iso_heater: Some(settings.ipa_heater_target),
                        high_voltage: true,
                        iso_pump: false, lights: None, glass_heater: false,
                    },
                )
            }
            Stopping(phase) => phase.react_to(history),
            // Coupure de secours : tout éteint, verrouillé jusqu'au
            // réarmement opérateur explicite (`SafetyMonitor::reset`, appelé
            // depuis `control_loop.rs::run()` en priorité absolue).
            Tripped(cause) => (
                SystemTask::Tripped(cause),
                ActuatorPlan {
                    cooling: None, iso_heater: None, high_voltage: false,
                    iso_pump: false, lights: None, glass_heater: false,
                },
            ),
        }
    }
}


impl<Hv, Cool, Iso, Pump, Lights, Glass> Actuators<Hv, Cool, Iso, Pump, Lights, Glass>
where
    Hv: BinaryActuator,
    Cool: TargetActuator<Celsius, CONTROL_LOOP_HISTORY_SIZE>,
    Iso: TargetActuator<Celsius, CONTROL_LOOP_HISTORY_SIZE>,
    Pump: BinaryActuator,
    Lights: BinaryActuator,
    Glass: BinaryActuator,
{
    pub fn apply(&mut self, plan: ActuatorPlan, hist: &MeasurementHistory) {
        let cooling_hist = &hist.temps[CHAMBER_TEMP_IDX];
        let iso_temp_hist = &hist.temps[ISO_TEMP_IDX];

        let _ = self.cooling.regulate(cooling_hist, plan.cooling);
        let _ = self.iso_heater.regulate(iso_temp_hist, plan.iso_heater);

        set_binary(&mut self.high_voltage, plan.high_voltage);
        set_binary(&mut self.iso_pump, plan.iso_pump);
        if plan.lights.is_some() {
            set_binary(&mut self.lights, plan.lights.unwrap());
        }
        set_binary(&mut self.glass_heater, plan.glass_heater);
    }
}

fn set_binary<A: BinaryActuator>(actuator: &mut A, on: bool) {
    let _ = if on { actuator.turn_on() } else { actuator.turn_off() };
}


// ─── Tests ───────────────────────────────────────────────────────────────────
//
// Portée : ces tests exercent `tick()` enchaîné sur plusieurs tours, comme
// en usage réel — sondage, réconciliation SHARED_STATE (dans les deux
// sens), priorité sécurité, application actionneurs, publication. Ils ne
// re-testent pas individuellement chaque condition de transition par phase
// (déjà couvert par cooling.rs/stopping.rs), ni chaque timeout/durée pris
// isolément (déjà couvert par phase_clock.rs), ni SafetyMonitor isolément
// (déjà couvert par security.rs) — seulement leur enchaînement correct.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_chamber_hal::config::{
        CHAMBER_PRESSURE_IDX, CHAMBER_TEMP_IDX, COMPRESSOR_OUT_IDX, ISO_TEMP_IDX,
    };
    use crate::cloud_chamber_hal::measurement::Measurement;
    use crate::cloud_chamber_hal::timer::Instant;
    use crate::config::operating::{IPA_HEATER_TARGET_C, PRECOOL_TARGET_C, SATURATION_TARGET_C};
    use crate::logic::timing::{
        IPA_CIRCULATION_MS, PRECOOL_TIMEOUT_MS, SENSOR_CHECK_TIMEOUT_MS, SENSOR_LOSS_MS,
        STOP_COMPRESSOR_SETTLE_MS, STOP_EQUALIZE_FALLBACK_MS, STOP_HV_SETTLE_MS,
    };
    use crate::drivers::mock::{
        MockActuator, MockClock, MockPressureSensor, MockSensorError, MockTempSensor,
    };
    use crate::logic::cooling::CoolingPhase;
    use crate::logic::security::SafetyCause;
    use crate::logic::stopping::StoppingPhase;

    // ─── Isolation entre tests ──────────────────────────────────────────────
    // `SHARED_STATE` est un `static` unique pour tout le process — sans ce
    // verrou, des tests de ce module (parallèles par défaut sous `cargo
    // test`) pourraient lire/écrire l'état l'un de l'autre.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Remet `SHARED_STATE` à un état par défaut connu et exécute `body`
    /// sous verrou exclusif — chaque test démarre donc d'un état propre,
    /// indépendant de l'ordre d'exécution.
    ///
    /// Réinitialise aussi `shared::settings` (verrou séparé, static
    /// différent) : `cooling.rs`/`stopping.rs`/`Stabilising` lisent
    /// `shared::settings::get()` dans leur chemin normal, donc tout test
    /// qui les exerce (la quasi-totalité de ce module) y est exposé au
    /// même risque de parallélisme que sur `SHARED_STATE`.
    fn with_isolated_shared_state<T>(body: impl FnOnce() -> T) -> T {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        critical_section::with(|cs| {
            let mut s = SHARED_STATE.borrow_ref_mut(cs);
            s.snapshot = SensorSnapshot::default();
            s.task = SystemTask::Idle;
            s.new_data = false;
        });
        crate::shared::settings::with_isolated_settings(body)
    }

    // ─── Harness ──────────────────────────────────────────────────────────

    struct Harness<'a> {
        clock: &'a MockClock,
        sensors: Sensors<MockTempSensor, MockPressureSensor>,
        actuators: Actuators<MockActuator, MockActuator, MockActuator, MockActuator, MockActuator, MockActuator>,
        phase: PhaseClock<&'a MockClock>,
        safety: SafetyMonitor,
        history: MeasurementHistory,
        probing_plan: ProbingPlan,
    }

    impl<'a> Harness<'a> {
        /// Démarre à `SystemTask::Idle`, comme `run()` en conditions
        /// réelles — le déclenchement d'un cycle passe par une écriture
        /// externe dans `SHARED_STATE` (`write_shared_task`), pas par un
        /// paramètre ici.
        fn new(clock: &'a MockClock) -> Self {
            Self::starting_at(clock, SystemTask::Idle)
        }

        /// Comme `new`, mais démarre à `task` — et aligne `SHARED_STATE` sur
        /// la même valeur, pour que le premier `tick()` n'adopte pas `Idle`
        /// (valeur par défaut de `SHARED_STATE`) à la place de l'état de
        /// départ voulu par le test.
        fn starting_at(clock: &'a MockClock, task: SystemTask) -> Self {
            critical_section::with(|cs| SHARED_STATE.borrow_ref_mut(cs).task = task);

            let mut sensors = Sensors::new(
                MockTempSensor::new(f32::NAN),
                MockPressureSensor::new(f32::NAN),
            );
            // Température sortie-compresseur valide par défaut (loin des
            // seuils d'alarme), horodatée avant tout instant utilisable par
            // un test (`Instant::from_micros(1)` : après le t0 par défaut
            // de `MeasurementHistory`, mais avant toute horloge de test qui
            // suit la convention `MockClock::new(1)` ou plus tard) — sinon
            // `SafetyMonitor` considérerait le capteur perdu après
            // `SENSOR_LOSS_MS` et déclencherait un trip parasite dans
            // n'importe quel test qui tourne plus de quelques secondes de
            // temps simulé. Les tests qui exercent spécifiquement ce chemin
            // (sécurité) l'écrasent explicitement via `set_compressor_temp`.
            sensors.temperature_source.set(
                COMPRESSOR_OUT_IDX, Ok(Measurement::new(Instant::from_micros(1), Celsius(20.0))),
            );

            let actuators = Actuators {
                high_voltage: MockActuator::new(),
                cooling: MockActuator::new(),
                iso_heater: MockActuator::new(),
                iso_pump: MockActuator::new(),
                lights: MockActuator::new(),
                glass_heater: MockActuator::new(),
            };
            let history = MeasurementHistory::new();
            let probing_plan = task.create_probing_plan(&history);
            let phase = PhaseClock::new(clock, task);
            let safety = SafetyMonitor::new(SafetyConfig::default(), phase.now_ms());
            Self { clock, sensors, actuators, phase, safety, history, probing_plan }
        }

        fn tick(&mut self) {
            self.probing_plan = tick(
                &mut self.sensors, &mut self.actuators, &mut self.phase, &mut self.safety,
                &mut self.history, self.probing_plan,
            );
        }

        fn tick_after_ms(&mut self, ms: u64) {
            self.clock.advance_ms(ms);
            self.tick();
        }

        fn set_temp(&mut self, idx: usize, value_c: f32) {
            let m = Measurement::new(self.clock.now(), Celsius(value_c));
            self.sensors.temperature_source.set(idx, Ok(m));
        }

        fn set_chamber_temp(&mut self, value_c: f32) {
            self.set_temp(CHAMBER_TEMP_IDX, value_c);
        }

        fn set_iso_temp(&mut self, value_c: f32) {
            self.set_temp(ISO_TEMP_IDX, value_c);
        }

        fn set_compressor_temp(&mut self, value_c: f32) {
            self.set_temp(COMPRESSOR_OUT_IDX, value_c);
        }

        fn lose_chamber_temp(&mut self) {
            self.sensors.temperature_source.set(CHAMBER_TEMP_IDX, Err(MockSensorError));
        }

        fn set_pressure(&mut self, idx: usize, value: f32) {
            let m = Measurement::new(self.clock.now(), HectoPascal(value));
            self.sensors.pressure_source.set(idx, Ok(m));
        }

        /// Avance l'horloge, poste une nouvelle lecture chambre horodatée à
        /// ce nouvel instant, puis exécute le tour — le motif répété pour
        /// construire les échantillons à intervalle régulier qu'exige
        /// `temp_stable` en `HighVoltage`.
        fn tick_with_chamber_temp_after_ms(&mut self, ms: u64, value_c: f32) {
            self.clock.advance_ms(ms);
            self.set_chamber_temp(value_c);
            self.tick();
        }

        /// Répète `tick_with_chamber_temp_after_ms` jusqu'à ce que
        /// `phase.current()` change, ou jusqu'à `max_ticks` (panique si
        /// jamais atteint — une boucle qui n'aboutit pas doit faire
        /// échouer le test bruyamment, pas silencieusement s'arrêter).
        fn tick_until_task_changes(&mut self, step_ms: u64, value_c: f32, max_ticks: usize) {
            let starting = self.phase.current();
            for _ in 0..max_ticks {
                self.tick_with_chamber_temp_after_ms(step_ms, value_c);
                if self.phase.current() != starting {
                    return;
                }
            }
            panic!("tick_until_task_changes: pas de transition après {max_ticks} tours");
        }

        fn shared_task(&self) -> SystemTask {
            critical_section::with(|cs| SHARED_STATE.borrow_ref(cs).task)
        }

        fn write_shared_task(&self, task: SystemTask) {
            critical_section::with(|cs| SHARED_STATE.borrow_ref_mut(cs).task = task);
        }
    }

    // ─── Diagnostic de démarrage ─────────────────────────────────────────

    #[test]
    fn missing_mask_flags_each_silent_sensor_by_index() {
        // Bit i à 1 = capteur i muet. C'est ce masque que le message de
        // panique affiche, et qui désigne le faisceau à vérifier.
        let readings = [Some(1u8), None, Some(3), None];
        assert_eq!(missing_mask(&readings), 0b1010);
    }

    #[test]
    fn missing_mask_is_zero_when_every_sensor_answers() {
        let readings = [Some(1u8), Some(2), Some(3)];
        assert_eq!(missing_mask(&readings), 0);
    }

    #[test]
    fn missing_mask_flags_everything_when_nothing_answers() {
        let readings: [Option<u8>; 3] = [None, None, None];
        assert_eq!(missing_mask(&readings), 0b111);
    }

    // ─── F — Base ─────────────────────────────────────────────────────────

    #[test]
    fn idle_stays_idle_without_external_request() {
        with_isolated_shared_state(|| {
            let clock = MockClock::new(1);
            let mut h = Harness::new(&clock);
            for _ in 0..20 {
                h.tick_after_ms(1_000);
            }
            assert_eq!(h.phase.current(), SystemTask::Idle);
            assert!(!h.actuators.high_voltage.is_on);
            assert!(!h.actuators.cooling.is_on);
            assert!(!h.actuators.iso_heater.is_on);
            assert_eq!(h.shared_task(), SystemTask::Idle);
        });
    }

    // ─── A — Cycle complet (bout en bout) ────────────────────────────────

    #[test]
    fn full_cycle_idle_to_stabilising_and_back_to_idle() {
        with_isolated_shared_state(|| {
            let clock = MockClock::new(1);
            let mut h = Harness::new(&clock);

            // ─── Démarrage (signal UI) ────────────────────────────────────
            h.write_shared_task(SystemTask::Cooling(CoolingPhase::SensorCheck));
            h.set_chamber_temp(20.0);
            h.tick();
            assert_eq!(h.phase.current(), SystemTask::Cooling(CoolingPhase::PreCoolingThePlate));

            // ─── PreCoolingThePlate ─────────────────────────────────────────
            h.tick_with_chamber_temp_after_ms(1_000, 0.0); // encore au-dessus du seuil
            assert_eq!(h.phase.current(), SystemTask::Cooling(CoolingPhase::PreCoolingThePlate));
            assert!(h.actuators.cooling.is_on);
            assert!(!h.actuators.high_voltage.is_on);

            h.tick_with_chamber_temp_after_ms(1_000, PRECOOL_TARGET_C);
            assert_eq!(h.phase.current(), SystemTask::Cooling(CoolingPhase::StartingIpaCirculation));

            // ─── StartingIpaCirculation (purement temporisé) ─────────────────
            // Lecture iso posée une fois (pas de garde perte-capteur sur ce
            // canal, contrairement à la chambre — pas besoin de la
            // rafraîchir) : au-dessus de la cible pour que la règle de seuil
            // simple de `MockActuator` (`current > target`) le dise "actif" —
            // le sens réel d'hystérésis pour un chauffage (`ActivateBelow`,
            // s'active quand c'est FROID) est testé séparément et précisément
            // dans `drivers::regulated`, pas ici.
            h.set_iso_temp(IPA_HEATER_TARGET_C + 10.0);

            // Rafraîchit une lecture chambre régulièrement pendant l'attente
            // (2 min) : cette phase ignore la valeur (avancement temporisé),
            // mais reste soumise à l'abandon perte-capteur comme toute phase
            // de Cooling hors SensorCheck — sans rafraîchissement, un seul
            // grand saut d'horloge déclencherait cet abandon avant même
            // d'atteindre IPA_CIRCULATION_MS.
            let step = SENSOR_LOSS_MS / 2;
            let mut elapsed = 0u64;
            while elapsed + step < IPA_CIRCULATION_MS {
                h.tick_with_chamber_temp_after_ms(step, PRECOOL_TARGET_C);
                elapsed += step;
            }
            assert_eq!(h.phase.current(), SystemTask::Cooling(CoolingPhase::StartingIpaCirculation));
            assert!(h.actuators.iso_heater.is_on);
            h.tick_with_chamber_temp_after_ms(IPA_CIRCULATION_MS - elapsed, PRECOOL_TARGET_C);
            assert_eq!(h.phase.current(), SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa));

            // ─── SaturatingAirWithIpa ─────────────────────────────────────────
            h.tick_with_chamber_temp_after_ms(1_000, SATURATION_TARGET_C);
            assert_eq!(h.phase.current(), SystemTask::Cooling(CoolingPhase::HighVoltage));

            // ─── HighVoltage : lectures stables jusqu'à satisfaire temp_stable ──
            h.tick_until_task_changes(1_000, SATURATION_TARGET_C, 100);
            assert_eq!(h.phase.current(), SystemTask::Cooling(CoolingPhase::FinalCheckBeforeStabilising));
            assert!(h.actuators.high_voltage.is_on);

            // ─── FinalCheckBeforeStabilising ──────────────────────────────────
            h.tick_with_chamber_temp_after_ms(1_000, SATURATION_TARGET_C);
            assert_eq!(h.phase.current(), SystemTask::Stabilising);

            // ─── Stabilising : régime permanent, pas de sortie automatique ────
            // Chambre légèrement au-dessus de la cible (règle de seuil
            // simple du mock, cf. commentaire sur `set_iso_temp` plus haut)
            // pour que le froid soit "actif" pendant cette longue attente —
            // horloge avancée d'abord pour que cette lecture soit bien
            // horodatée après la précédente (sinon `push_if_newer` l'ignore).
            h.tick_with_chamber_temp_after_ms(1_000, SATURATION_TARGET_C + 5.0);
            for _ in 0..20 {
                h.tick_after_ms(60 * 60 * 1_000); // sauts d'une heure
            }
            assert_eq!(h.phase.current(), SystemTask::Stabilising);
            assert!(h.actuators.high_voltage.is_on);
            assert!(h.actuators.cooling.is_on);
            assert!(h.actuators.iso_heater.is_on);
            assert_eq!(h.shared_task(), SystemTask::Stabilising);

            // ─── Arrêt (signal UI) ─────────────────────────────────────────────
            h.write_shared_task(SystemTask::Stopping(StoppingPhase::CutHighVoltage));
            h.tick();
            assert_eq!(h.phase.current(), SystemTask::Stopping(StoppingPhase::CutHighVoltage));

            h.tick_after_ms(STOP_HV_SETTLE_MS);
            assert_eq!(h.phase.current(), SystemTask::Stopping(StoppingPhase::CutCompressor));
            assert!(!h.actuators.high_voltage.is_on);

            h.tick_after_ms(STOP_COMPRESSOR_SETTLE_MS);
            assert_eq!(h.phase.current(), SystemTask::Stopping(StoppingPhase::WaitPressureEquilibrium));
            assert!(!h.actuators.cooling.is_on);

            // Pas de capteur dédié au circuit réfrigérant : purement
            // temporisé désormais (cf. logic::stopping), le seul chemin vers
            // Idle est le timeout d'équilibrage.
            h.tick_after_ms(STOP_EQUALIZE_FALLBACK_MS + 1);
            assert_eq!(h.phase.current(), SystemTask::Idle);
            assert!(!h.actuators.high_voltage.is_on);
            assert!(!h.actuators.cooling.is_on);
            assert!(!h.actuators.iso_heater.is_on);
            assert_eq!(h.shared_task(), SystemTask::Idle);
        });
    }

    #[test]
    fn cooling_aborts_to_idle_on_sensor_check_timeout() {
        with_isolated_shared_state(|| {
            let clock = MockClock::new(1);
            let mut h = Harness::starting_at(&clock, SystemTask::Cooling(CoolingPhase::SensorCheck));
            // Chambre jamais valide : react_to reste en SensorCheck, c'est
            // le timeout de phase qui doit finir par abandonner (perte
            // capteur exemptée pendant SensorCheck, cf. test dédié plus bas).
            h.tick_after_ms(SENSOR_CHECK_TIMEOUT_MS + 1);
            assert_eq!(h.phase.current(), SystemTask::Idle);
            assert_eq!(h.shared_task(), SystemTask::Idle);
        });
    }

    #[test]
    fn cooling_aborts_to_idle_on_precool_timeout() {
        with_isolated_shared_state(|| {
            let clock = MockClock::new(1);
            let mut h = Harness::starting_at(&clock, SystemTask::Cooling(CoolingPhase::PreCoolingThePlate));
            // Rafraîchit une lecture chambre valide mais jamais sous le
            // seuil, assez souvent pour ne jamais déclencher l'abandon
            // perte-capteur (SENSOR_LOSS_MS) avant le timeout de phase —
            // isole le chemin "timeout" de celui de "perte capteur".
            let step = SENSOR_LOSS_MS / 2;
            let mut elapsed = 0u64;
            while elapsed <= PRECOOL_TIMEOUT_MS {
                h.tick_with_chamber_temp_after_ms(step, 0.0);
                elapsed += step;
            }
            assert_eq!(h.phase.current(), SystemTask::Idle);
        });
    }

    #[test]
    fn stopping_falls_back_to_idle_without_pressure_sensor() {
        with_isolated_shared_state(|| {
            let clock = MockClock::new(1);
            let mut h = Harness::starting_at(&clock, SystemTask::Stopping(StoppingPhase::WaitPressureEquilibrium));
            h.tick_after_ms(STOP_EQUALIZE_FALLBACK_MS + 1);
            assert_eq!(h.phase.current(), SystemTask::Idle);
        });
    }

    #[test]
    fn cooling_survives_a_transient_sensor_blip() {
        with_isolated_shared_state(|| {
            let clock = MockClock::new(1);
            let mut h = Harness::starting_at(&clock, SystemTask::Cooling(CoolingPhase::PreCoolingThePlate));
            h.set_chamber_temp(0.0);
            h.tick_after_ms(1_000);
            assert_eq!(h.phase.current(), SystemTask::Cooling(CoolingPhase::PreCoolingThePlate));

            // Coupure brève (bien en dessous de SENSOR_LOSS_MS), puis reprise.
            h.lose_chamber_temp();
            h.tick_after_ms(2_000);
            assert_eq!(h.phase.current(), SystemTask::Cooling(CoolingPhase::PreCoolingThePlate));

            h.set_chamber_temp(PRECOOL_TARGET_C);
            h.tick_after_ms(1_000);
            assert_eq!(h.phase.current(), SystemTask::Cooling(CoolingPhase::StartingIpaCirculation));
        });
    }

    // ─── B — Abandon perte-capteur mi-cycle ──────────────────────────────

    #[test]
    fn sensor_loss_aborts_mid_cooling() {
        with_isolated_shared_state(|| {
            let clock = MockClock::new(1);
            let mut h = Harness::starting_at(&clock, SystemTask::Cooling(CoolingPhase::PreCoolingThePlate));
            h.set_chamber_temp(0.0); // valide, au-dessus du seuil (pas de transition)
            h.tick();
            assert_eq!(h.phase.current(), SystemTask::Cooling(CoolingPhase::PreCoolingThePlate));

            h.lose_chamber_temp();
            h.tick_after_ms(SENSOR_LOSS_MS + 1);
            assert_eq!(h.phase.current(), SystemTask::Idle);
            assert_eq!(h.shared_task(), SystemTask::Idle);
        });
    }

    #[test]
    fn sensor_loss_is_exempted_during_sensor_check() {
        with_isolated_shared_state(|| {
            let clock = MockClock::new(1);
            let mut h = Harness::starting_at(&clock, SystemTask::Cooling(CoolingPhase::SensorCheck));
            // Chambre jamais valide ; horloge avancée au-delà de
            // SENSOR_LOSS_MS mais en dessous de SENSOR_CHECK_TIMEOUT_MS.
            assert!(SENSOR_LOSS_MS + 1_000 < SENSOR_CHECK_TIMEOUT_MS);
            h.tick_after_ms(SENSOR_LOSS_MS + 1_000);
            assert_eq!(h.phase.current(), SystemTask::Cooling(CoolingPhase::SensorCheck));
        });
    }

    // ─── C — Priorité sécurité ────────────────────────────────────────────

    #[test]
    fn safety_trip_cuts_actuators_and_overrides_cooling() {
        with_isolated_shared_state(|| {
            let clock = MockClock::new(1);
            let mut h = Harness::starting_at(&clock, SystemTask::Cooling(CoolingPhase::HighVoltage));
            h.set_chamber_temp(SATURATION_TARGET_C);
            h.tick();
            assert!(h.actuators.high_voltage.is_on);

            h.set_compressor_temp(150.0); // > seuil alarme (120.0)
            h.tick_after_ms(1_000);
            h.tick_after_ms(1_000);
            h.tick_after_ms(1_000); // 3e tour consécutif en alarme -> trip

            assert_eq!(h.phase.current(), SystemTask::Tripped(SafetyCause::CompressorOverheat));
            assert!(!h.actuators.high_voltage.is_on);
            assert!(!h.actuators.cooling.is_on);
            assert!(!h.actuators.iso_heater.is_on);
            assert_eq!(h.shared_task(), SystemTask::Tripped(SafetyCause::CompressorOverheat));
        });
    }

    #[test]
    fn safety_trip_persists_without_explicit_reset() {
        with_isolated_shared_state(|| {
            let clock = MockClock::new(1);
            let mut h = Harness::starting_at(&clock, SystemTask::Cooling(CoolingPhase::HighVoltage));
            h.set_compressor_temp(150.0);
            h.tick_after_ms(1_000);
            h.tick_after_ms(1_000);
            h.tick_after_ms(1_000);
            assert_eq!(h.phase.current(), SystemTask::Tripped(SafetyCause::CompressorOverheat));

            // La condition redevient normale, mais reste Tripped : pas de
            // réarmement automatique (`SafetyMonitor::reset` n'est jamais
            // appelé depuis `control_loop.rs` aujourd'hui — gap documenté,
            // pas caché).
            h.set_compressor_temp(20.0);
            for _ in 0..10 {
                h.tick_after_ms(1_000);
            }
            assert_eq!(h.phase.current(), SystemTask::Tripped(SafetyCause::CompressorOverheat));
        });
    }

    #[test]
    fn safety_trip_reasserts_itself_over_an_external_idle_request() {
        with_isolated_shared_state(|| {
            let clock = MockClock::new(1);
            let mut h = Harness::starting_at(&clock, SystemTask::Cooling(CoolingPhase::HighVoltage));
            h.set_compressor_temp(150.0);
            h.tick_after_ms(1_000);
            h.tick_after_ms(1_000);
            h.tick_after_ms(1_000);
            assert_eq!(h.phase.current(), SystemTask::Tripped(SafetyCause::CompressorOverheat));

            // Acquittement UI simulé : écrit Idle directement dans SHARED_STATE.
            h.write_shared_task(SystemTask::Idle);
            h.tick_after_ms(1_000);

            // `tick()` adopte brièvement Idle en tout début de tour, mais
            // `SafetyMonitor` (état interne, indépendant de `phase`/
            // SHARED_STATE) republie Tripped le même tour : sans `reset()`
            // câblé, l'acquittement UI seul ne suffit pas à sortir de
            // Tripped — même gap que ci-dessus, vu ici depuis SHARED_STATE.
            assert_eq!(h.phase.current(), SystemTask::Tripped(SafetyCause::CompressorOverheat));
            assert_eq!(h.shared_task(), SystemTask::Tripped(SafetyCause::CompressorOverheat));
        });
    }

    // ─── D — Réconciliation SHARED_STATE ─────────────────────────────────

    #[test]
    fn external_write_is_adopted_on_the_next_tick() {
        with_isolated_shared_state(|| {
            let clock = MockClock::new(1);
            let mut h = Harness::new(&clock);
            h.write_shared_task(SystemTask::Cooling(CoolingPhase::SensorCheck));
            h.set_chamber_temp(-5.0);
            h.tick();
            assert_eq!(h.phase.current(), SystemTask::Cooling(CoolingPhase::PreCoolingThePlate));
        });
    }

    #[test]
    fn external_write_of_the_same_task_is_a_no_op() {
        with_isolated_shared_state(|| {
            let clock = MockClock::new(1);
            let mut h = Harness::starting_at(&clock, SystemTask::Cooling(CoolingPhase::StartingIpaCirculation));
            h.tick_after_ms(1_000);
            assert_eq!(h.phase.current(), SystemTask::Cooling(CoolingPhase::StartingIpaCirculation));

            // Écrit exactement la même tâche dans SHARED_STATE : ne doit
            // pas remettre `elapsed_ms` à zéro (sinon la transition
            // temporisée ci-dessous n'arriverait jamais). Rafraîchit la
            // lecture chambre régulièrement pendant l'attente pour ne pas
            // déclencher l'abandon perte-capteur (cf. commentaire équivalent
            // dans `full_cycle_...`).
            h.write_shared_task(SystemTask::Cooling(CoolingPhase::StartingIpaCirculation));
            let step = SENSOR_LOSS_MS / 2;
            let mut elapsed = 1_000u64;
            while elapsed + step < IPA_CIRCULATION_MS {
                h.tick_with_chamber_temp_after_ms(step, PRECOOL_TARGET_C);
                elapsed += step;
            }
            assert_eq!(h.phase.current(), SystemTask::Cooling(CoolingPhase::StartingIpaCirculation));
            h.tick_with_chamber_temp_after_ms(IPA_CIRCULATION_MS - elapsed, PRECOOL_TARGET_C);
            assert_eq!(h.phase.current(), SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa));
        });
    }

    #[test]
    fn subphase_change_is_published_without_a_special_case() {
        with_isolated_shared_state(|| {
            let clock = MockClock::new(1);
            let mut h = Harness::starting_at(&clock, SystemTask::Cooling(CoolingPhase::PreCoolingThePlate));
            h.set_chamber_temp(PRECOOL_TARGET_C);
            h.tick();
            assert_eq!(h.phase.current(), SystemTask::Cooling(CoolingPhase::StartingIpaCirculation));
            assert_eq!(h.shared_task(), SystemTask::Cooling(CoolingPhase::StartingIpaCirculation));
        });
    }

    /// Enveloppe un `MockActuator` et déclenche, au premier appel de
    /// `turn_on`/`turn_off`, une écriture directe dans `SHARED_STATE` —
    /// simule une écriture externe (UI) survenant pendant
    /// `Actuators::apply()`, exactement dans la fenêtre entre la capture de
    /// `synced_task` et `publish_task_if_unchanged` dans `tick()`. Local à
    /// ce test : ne va pas dans `drivers::mock`, qui reste générique et ne
    /// connaît pas `SHARED_STATE`.
    struct RacyActuator {
        inner: MockActuator,
        inject: Option<SystemTask>,
    }

    impl BinaryActuator for RacyActuator {
        type Error = core::convert::Infallible;

        fn turn_on(&mut self) -> Result<(), Self::Error> {
            self.fire();
            self.inner.turn_on()
        }

        fn turn_off(&mut self) -> Result<(), Self::Error> {
            self.fire();
            self.inner.turn_off()
        }
    }

    impl RacyActuator {
        fn fire(&mut self) {
            if let Some(task) = self.inject.take() {
                critical_section::with(|cs| SHARED_STATE.borrow_ref_mut(cs).task = task);
            }
        }
    }

    #[test]
    fn external_write_during_tick_is_not_clobbered_by_publish() {
        with_isolated_shared_state(|| {
            let clock = MockClock::new(1);
            let mut sensors = Sensors::new(
                MockTempSensor::new(-20.0), MockPressureSensor::new(f32::NAN),
            );
            sensors.temperature_source.set(
                COMPRESSOR_OUT_IDX, Ok(Measurement::new(Instant::from_micros(1), Celsius(20.0))),
            );
            // Chambre déjà sous PRECOOL_TARGET_C : `advance()` va vouloir
            // avancer de PreCoolingThePlate vers StartingIpaCirculation ce
            // tour — c'est cette transition que la course va invalider.
            let mut actuators = Actuators {
                high_voltage: RacyActuator { inner: MockActuator::new(), inject: Some(SystemTask::Idle) },
                cooling: MockActuator::new(),
                iso_heater: MockActuator::new(),
                iso_pump: MockActuator::new(),
                lights: MockActuator::new(),
                glass_heater: MockActuator::new(),
            };
            let mut history = MeasurementHistory::new();
            let mut phase = PhaseClock::new(&clock, SystemTask::Cooling(CoolingPhase::PreCoolingThePlate));
            let mut safety = SafetyMonitor::new(SafetyConfig::default(), phase.now_ms());
            let probing_plan = phase.current().create_probing_plan(&history);

            // `with_isolated_shared_state` a remis SHARED_STATE.task à
            // `Idle` — il faut l'aligner sur l'état local avant le premier
            // tick, sinon `tick()` adopterait cet `Idle` de départ au lieu
            // de calculer la transition qu'on veut mettre en course avec
            // l'injection.
            critical_section::with(|cs| {
                SHARED_STATE.borrow_ref_mut(cs).task = SystemTask::Cooling(CoolingPhase::PreCoolingThePlate);
            });

            let probing_plan = tick(&mut sensors, &mut actuators, &mut phase, &mut safety, &mut history, probing_plan);

            // L'écriture injectée (survenue "pendant" ce tour) n'a pas été
            // écrasée par la publication de `next` :
            let shared = critical_section::with(|cs| SHARED_STATE.borrow_ref(cs).task);
            assert_eq!(shared, SystemTask::Idle);
            // Et l'état local n'a pas non plus adopté `next` (resté à la
            // valeur synchronisée en début de tour) :
            assert_eq!(phase.current(), SystemTask::Cooling(CoolingPhase::PreCoolingThePlate));

            // Le tour suivant adopte correctement la valeur injectée.
            tick(&mut sensors, &mut actuators, &mut phase, &mut safety, &mut history, probing_plan);
            assert_eq!(phase.current(), SystemTask::Idle);
        });
    }

    // ─── E — Sondage / robustesse capteurs ───────────────────────────────

    #[test]
    fn partial_sensor_failure_keeps_last_known_reading() {
        with_isolated_shared_state(|| {
            let clock = MockClock::new(1);
            let mut h = Harness::new(&clock);
            h.set_chamber_temp(-10.0);
            h.tick_after_ms(1_000);
            let before = critical_section::with(|cs| SHARED_STATE.borrow_ref(cs).snapshot.temps[CHAMBER_TEMP_IDX]);
            assert_eq!(before.unwrap().value.0, -10.0);

            // La sonde chambre tombe en erreur ce tour ; une autre catégorie
            // continue de fonctionner normalement (pour que ce tour ne soit
            // pas un échec total, cf. test suivant).
            h.lose_chamber_temp();
            h.set_pressure(CHAMBER_PRESSURE_IDX, 0.5);
            h.tick_after_ms(1_000);

            let kept = critical_section::with(|cs| SHARED_STATE.borrow_ref(cs).snapshot.temps[CHAMBER_TEMP_IDX]);
            assert_eq!(kept.unwrap().value.0, -10.0);
        });
    }

    #[test]
    fn fully_failed_probe_leaves_shared_snapshot_untouched() {
        with_isolated_shared_state(|| {
            let clock = MockClock::new(1);
            let mut h = Harness::new(&clock);
            h.set_chamber_temp(-10.0);
            h.tick_after_ms(1_000);

            for i in 0..NUMBER_OF_TEMP_SENSOR {
                h.sensors.temperature_source.set(i, Err(MockSensorError));
            }
            for i in 0..NUMBER_OF_PRESSURE_SENSOR {
                h.sensors.pressure_source.set(i, Err(MockSensorError));
            }
            h.tick_after_ms(1_000);

            let after = critical_section::with(|cs| SHARED_STATE.borrow_ref(cs).snapshot.temps[CHAMBER_TEMP_IDX]);
            assert_eq!(after.unwrap().value.0, -10.0);
        });
    }
}
