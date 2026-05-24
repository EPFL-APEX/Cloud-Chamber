//! Structure principale `SecurityLoop` et boucle `run()`.
//!
//! # Architecture de la boucle
//!
//! La boucle tourne sur Core1 à 100 Hz (période de 10 ms = 10 000 µs).
//! Chaque itération comprend :
//!
//! 1. Lecture des capteurs critiques (température, tension, courant)
//! 2. Évaluation des seuils → action sur le disjoncteur si nécessaire
//! 3. Si du temps reste : lecture des capteurs non critiques + partage Core0
//! 4. Busy-wait jusqu'à la prochaine période
//!
//! # Trait `SharedWriter`
//!
//! Abstraction de l'écriture vers Core0. Découple `SecurityLoop` de
//! `critical_section` pour permettre les tests unitaires sans section critique.

use crate::{
    cloud_chamber_hal::{
        actuators::BreakerActuator,
        sensors::{ClosureSensor, CurrentSensor, TemperatureSensor, VoltageSensor},
        timer::{MonotonicTimer, WatchdogFeed},
    },
    security_loop::safety::{evaluate_safety, SafetyConfig},
    security_loop::states::SensorHistory,
    shared::data::{
        SensorSnapshot, SharedState, SystemState, NUMBER_OF_AMP, NUMBER_OF_TEMPS, NUMBER_OF_VOLT,
    },
};

/// Période cible de la boucle en microsecondes (10 ms).
const PERIOD_US: u64 = 10_000;

/// Budget temporel pour la phase 1 (capteurs critiques), en µs.
const PHASE1_BUDGET_US: u64 = 3_000;

// ─── Trait SharedWriter ───────────────────────────────────────────────────────

/// Abstraction de l'écriture du `SharedState` vers Core0.
///
/// En production : implémenté par `CriticalSectionWriter` (section critique).
/// En test : implémenté par un mock qui capture le dernier état écrit.
pub trait SharedWriter {
    fn write(&mut self, state: SharedState);
}

/// Implémentation production utilisant `critical_section`.
pub struct CriticalSectionWriter;

impl SharedWriter for CriticalSectionWriter {
    fn write(&mut self, state: SharedState) {
        critical_section::with(|cs| {
            crate::shared::data::SHARED.borrow(cs).replace(state);
        });
    }
}

// ─── SecurityLoop ─────────────────────────────────────────────────────────────

/// Boucle de sécurité temps-réel.
///
/// Les 8 paramètres génériques correspondent aux implémentations concrètes
/// des périphériques. Cela permet de substituer des mocks en tests sans
/// modifier la logique métier.
pub struct SecurityLoop<Timer, Wdog, Temp, Volt, Amp, Closure, Breaker, Writer>
where
    Timer: MonotonicTimer,
    Wdog: WatchdogFeed,
    Temp: TemperatureSensor,
    Volt: VoltageSensor,
    Amp: CurrentSensor,
    Closure: ClosureSensor,
    Breaker: BreakerActuator,
    Writer: SharedWriter,
{
    timer: Timer,
    watchdog: Wdog,
    temp_sensors: [Temp; NUMBER_OF_TEMPS],
    volt_sensors: [Volt; NUMBER_OF_VOLT],
    amp_sensors: [Amp; NUMBER_OF_AMP],
    closure_sensor: Closure,
    breaker: Breaker,
    writer: Writer,
    history: SensorHistory,
    config: SafetyConfig,
    current_state: SystemState,
}

impl<Timer, Wdog, Temp, Volt, Amp, Closure, Breaker, Writer>
    SecurityLoop<Timer, Wdog, Temp, Volt, Amp, Closure, Breaker, Writer>
where
    Timer: MonotonicTimer,
    Wdog: WatchdogFeed,
    Temp: TemperatureSensor,
    Volt: VoltageSensor,
    Amp: CurrentSensor,
    Closure: ClosureSensor,
    Breaker: BreakerActuator,
    Writer: SharedWriter,
{
    pub fn new(
        timer: Timer,
        watchdog: Wdog,
        temp_sensors: [Temp; NUMBER_OF_TEMPS],
        volt_sensors: [Volt; NUMBER_OF_VOLT],
        amp_sensors: [Amp; NUMBER_OF_AMP],
        closure_sensor: Closure,
        breaker: Breaker,
        writer: Writer,
        config: SafetyConfig,
    ) -> Self {
        Self {
            timer,
            watchdog,
            temp_sensors,
            volt_sensors,
            amp_sensors,
            closure_sensor,
            breaker,
            writer,
            history: SensorHistory::new(),
            config,
            current_state: SystemState::Normal,
        }
    }

    /// Point d'entrée de la tâche Core1. Ne retourne jamais.
    pub fn run(&mut self) -> ! {
        loop {
            let start = self.timer.get_counter_us();
            self.run_one_iteration(start);
            // Busy-wait jusqu'à la prochaine période
            while self.timer.get_counter_us().wrapping_sub(start) < PERIOD_US {
                core::hint::spin_loop();
            }
        }
    }

    /// Exécute une itération complète. Retourne `true` si phase 2 a eu lieu.
    ///
    /// Séparé de `run()` pour permettre les tests unitaires.
    pub fn run_one_iteration(&mut self, start_time: u64) -> bool {
        // Phase 1 : capteurs critiques + réaction
        let snapshot = self.read_critical_sensors();
        self.evaluate_and_react(&snapshot);
        self.watchdog.feed();

        // Phase 2 : capteurs non critiques si budget restant
        let elapsed = self.timer.get_counter_us().wrapping_sub(start_time);
        if elapsed < PHASE1_BUDGET_US {
            let is_closed = self.closure_sensor.is_closed().unwrap_or(false);
            let _ = self.history.push_closeness(is_closed);
            self.writer.write(SharedState {
                snapshot,
                system_state: self.current_state,
                new_data: true,
            });
            true
        } else {
            false
        }
    }

    fn read_critical_sensors(&mut self) -> SensorSnapshot {
        let mut snapshot = SensorSnapshot::default();
        for (i, sensor) in self.temp_sensors.iter_mut().enumerate() {
            if let Ok(v) = sensor.read_celsius() {
                snapshot.temps[i] = v;
                let _ = self.history.push_temp(i, v);
            }
        }
        for (i, sensor) in self.volt_sensors.iter_mut().enumerate() {
            if let Ok(v) = sensor.read_voltage() {
                snapshot.volts[i] = v;
                let _ = self.history.push_voltage(i, v);
            }
        }
        for (i, sensor) in self.amp_sensors.iter_mut().enumerate() {
            if let Ok(v) = sensor.read_amperes() {
                snapshot.amps[i] = v;
                let _ = self.history.push_amperage(i, v);
            }
        }
        snapshot
    }

    fn evaluate_and_react(&mut self, snapshot: &SensorSnapshot) {
        let new_state = evaluate_safety(snapshot, &self.config);
        self.current_state = new_state;
        if new_state == SystemState::Alarm {
            let _ = self.breaker.trip();
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    // ── Mocks ──

    struct MockTimer { counter: u64 }
    impl MonotonicTimer for MockTimer {
        fn get_counter_us(&self) -> u64 { self.counter }
    }

    struct MockWdog;
    impl WatchdogFeed for MockWdog { fn feed(&mut self) {} }

    struct MockTemp { value: f32 }
    impl TemperatureSensor for MockTemp {
        type Error = core::convert::Infallible;
        fn start_measurement(&mut self) -> Result<(), Self::Error> { Ok(()) }
        fn read_celsius(&mut self) -> Result<f32, Self::Error> { Ok(self.value) }
    }

    struct MockVolt { value: f32 }
    impl VoltageSensor for MockVolt {
        type Error = core::convert::Infallible;
        fn read_voltage(&mut self) -> Result<f32, Self::Error> { Ok(self.value) }
    }

    struct MockAmp { value: f32 }
    impl CurrentSensor for MockAmp {
        type Error = core::convert::Infallible;
        fn read_amperes(&mut self) -> Result<f32, Self::Error> { Ok(self.value) }
    }

    struct MockClosure { closed: bool }
    impl ClosureSensor for MockClosure {
        type Error = core::convert::Infallible;
        fn is_closed(&mut self) -> Result<bool, Self::Error> { Ok(self.closed) }
    }

    struct MockBreaker { tripped: bool }
    impl BreakerActuator for MockBreaker {
        type Error = core::convert::Infallible;
        fn trip(&mut self) -> Result<(), Self::Error> { self.tripped = true; Ok(()) }
        fn reset(&mut self) -> Result<(), Self::Error> { self.tripped = false; Ok(()) }
        fn is_tripped(&self) -> Result<bool, Self::Error> { Ok(self.tripped) }
    }

    struct MockWriter { pub last: Option<SystemState> }
    impl SharedWriter for MockWriter {
        fn write(&mut self, state: SharedState) { self.last = Some(state.system_state); }
    }

    fn make_loop(temp: f32) -> SecurityLoop<
        MockTimer, MockWdog,
        MockTemp, MockVolt, MockAmp,
        MockClosure, MockBreaker, MockWriter,
    > {
        SecurityLoop::new(
            MockTimer { counter: 0 },
            MockWdog,
            core::array::from_fn(|_| MockTemp { value: temp }),
            core::array::from_fn(|_| MockVolt { value: 5.0 }),
            core::array::from_fn(|_| MockAmp { value: 0.5 }),
            MockClosure { closed: false },
            MockBreaker { tripped: false },
            MockWriter { last: None },
            SafetyConfig::default(),
        )
    }

    #[test]
    fn normal_temp_does_not_trip_breaker() {
        let mut lp = make_loop(25.0);
        lp.run_one_iteration(0);
        assert!(!lp.breaker.tripped);
    }

    #[test]
    fn alarm_temp_trips_breaker() {
        let mut lp = make_loop(65.0);
        lp.run_one_iteration(0);
        assert!(lp.breaker.tripped);
    }

    #[test]
    fn phase2_runs_when_budget_available() {
        let mut lp = make_loop(25.0);
        let ran_phase2 = lp.run_one_iteration(0);
        assert!(ran_phase2);
    }

    #[test]
    fn writer_receives_system_state() {
        let mut lp = make_loop(25.0);
        lp.run_one_iteration(0);
        assert_eq!(lp.writer.last, Some(SystemState::Normal));
    }

    #[test]
    fn alarm_state_written_to_shared() {
        let mut lp = make_loop(65.0);
        lp.run_one_iteration(0);
        assert_eq!(lp.writer.last, Some(SystemState::Alarm));
    }
}
