//! Capteurs mock — aucun matériel, valeurs fixées par le test.
//!
//! Compilé uniquement en `#[cfg(test)]` (cf. la déclaration du module dans
//! `drivers::mod`) : ne fait jamais partie d'un build embarqué ou release.

use crate::cloud_chamber_hal::actuators::{BinaryActuator, TargetActuator};
use crate::cloud_chamber_hal::config::{
    NUMBER_OF_PRESSURE_SENSOR, NUMBER_OF_TEMP_SENSOR,
};
use crate::cloud_chamber_hal::measurement::Measurement;
use crate::cloud_chamber_hal::ring_buffer::RingBuffer;
use crate::cloud_chamber_hal::sensors::{BatchSensor, DeferredBatchSensor};
use crate::cloud_chamber_hal::timer::{Duration, Instant, MonotonicTimer};
use crate::cloud_chamber_hal::units::{Celsius, HectoPascal};

/// Erreur simulée : les mocks ne renvoient une erreur que si le test le
/// demande explicitement via [`MockTempSensor::set`] et consorts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MockSensorError;

/// Capteur de température mock (bus 1-Wire simulé) — implémente
/// `DeferredBatchSensor<Celsius, NUMBER_OF_TEMP_SENSOR>`.
pub struct MockTempSensor {
    readings: [Result<Measurement<Celsius>, MockSensorError>; NUMBER_OF_TEMP_SENSOR],
}

impl MockTempSensor {
    /// Toutes les cases valent `value_c`, horodatées à `Instant::from_micros(0)`.
    pub fn new(value_c: f32) -> Self {
        let m = Measurement::new(Instant::from_micros(0), Celsius(value_c));
        Self { readings: core::array::from_fn(|_| Ok(m)) }
    }

    /// Fixe la lecture (ou l'erreur) d'un capteur individuel.
    pub fn set(&mut self, index: usize, reading: Result<Measurement<Celsius>, MockSensorError>) {
        self.readings[index] = reading;
    }
}

impl BatchSensor<Celsius, NUMBER_OF_TEMP_SENSOR> for MockTempSensor {
    type Error = MockSensorError;

    fn read(&mut self) -> [Result<Measurement<Celsius>, Self::Error>; NUMBER_OF_TEMP_SENSOR] {
        self.readings
    }
}

impl DeferredBatchSensor<Celsius, NUMBER_OF_TEMP_SENSOR> for MockTempSensor {
    fn start_conversion(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn conversion_time_ms(&self) -> Duration {
        Duration::from_millis(0)
    }

    fn read_result(&mut self) -> [Result<Measurement<Celsius>, Self::Error>; NUMBER_OF_TEMP_SENSOR] {
        self.readings
    }
}

/// Capteur de pression mock — implémente
/// `BatchSensor<HectoPascal, NUMBER_OF_PRESSURE_SENSOR>`.
pub struct MockPressureSensor {
    readings: [Result<Measurement<HectoPascal>, MockSensorError>; NUMBER_OF_PRESSURE_SENSOR],
}

impl MockPressureSensor {
    /// Toutes les cases valent `value_hpa`, horodatées à `Instant::from_micros(0)`.
    pub fn new(value_hpa: f32) -> Self {
        let m = Measurement::new(Instant::from_micros(0), HectoPascal(value_hpa));
        Self { readings: core::array::from_fn(|_| Ok(m)) }
    }

    /// Fixe la lecture (ou l'erreur) d'un capteur individuel.
    pub fn set(&mut self, index: usize, reading: Result<Measurement<HectoPascal>, MockSensorError>) {
        self.readings[index] = reading;
    }
}

impl BatchSensor<HectoPascal, NUMBER_OF_PRESSURE_SENSOR> for MockPressureSensor {
    type Error = MockSensorError;

    fn read(&mut self) -> [Result<Measurement<HectoPascal>, Self::Error>; NUMBER_OF_PRESSURE_SENSOR] {
        self.readings
    }
}

/// Actionneur mock — mémorise le dernier état demandé, ne peut pas échouer.
pub struct MockActuator {
    pub is_on: bool,
}

impl MockActuator {
    pub fn new() -> Self {
        Self { is_on: false }
    }
}

impl BinaryActuator for MockActuator {
    type Error = core::convert::Infallible;

    fn turn_on(&mut self) -> Result<(), Self::Error> {
        self.is_on = true;
        Ok(())
    }

    fn turn_off(&mut self) -> Result<(), Self::Error> {
        self.is_on = false;
        Ok(())
    }
}

/// Seuil simple (pas d'hystérésis) : la régulation par hystérésis a sa
/// propre couverture de tests dédiée (`drivers::regulated`) — les tests
/// d'intégration de `control_loop.rs` n'ont pas besoin de re-simuler cette
/// logique, seulement de vérifier que `apply()` route les bonnes cibles
/// vers les bons actionneurs.
impl<Unit: Copy + PartialOrd, const N: usize> TargetActuator<Unit, N> for MockActuator {
    type Error = core::convert::Infallible;

    fn regulate(&mut self, hist: &RingBuffer<Measurement<Unit>, N>, target: Option<Unit>) -> Result<(), Self::Error> {
        let on = target.is_some_and(|t| hist.get(0).map(|m| m.value > t).unwrap_or(false));
        if on { self.turn_on() } else { self.turn_off() }
    }
}

/// Horloge mock pilotable par le test, avancée explicitement via
/// [`MockClock::advance`]. `&MockClock` implémente `MonotonicTimer`
/// directement (`Cell`, pas de `Rc` nécessaire).
pub struct MockClock(core::cell::Cell<Instant>);

impl MockClock {
    /// Démarre à `start`. Pour les tests qui font passer une lecture capteur
    /// par `MeasurementHistory` (via `push_if_newer`), préférer un départ
    /// après [`Instant::ZERO`] : `MeasurementHistory::new()` initialise ses
    /// buffers à `Instant::ZERO`, et `push_if_newer` n'enregistre une
    /// nouvelle lecture que si elle est strictement plus récente — une
    /// lecture posée à l'origine serait silencieusement ignorée.
    pub fn new(start: Instant) -> Self {
        Self(core::cell::Cell::new(start))
    }

    /// Avance l'horloge de `elapsed`. Prend une [`Duration`] et non des
    /// millisecondes nues, comme tout le reste de la chaîne temporelle.
    pub fn advance(&self, elapsed: Duration) {
        self.0.set(self.0.get() + elapsed);
    }
}

impl MonotonicTimer for &MockClock {
    fn now(&self) -> Instant {
        self.0.get()
    }
}
