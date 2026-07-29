//! Capteurs mock — aucun matériel, valeurs fixées par le test.
//!
//! Compilé uniquement en `#[cfg(test)]` (cf. la déclaration du module dans
//! `drivers::mod`) : ne fait jamais partie d'un build embarqué ou release.

use crate::cloud_chamber_hal::config::{
    NUMBER_OF_PRESSURE_SENSOR, NUMBER_OF_TEMP_SENSOR, NUMBER_OF_VOLTMETER,
};
use crate::cloud_chamber_hal::measurement::Measurement;
use crate::cloud_chamber_hal::sensors::{BatchSensor, DeferredBatchSensor};
use crate::cloud_chamber_hal::timer::{Duration, Instant};
use crate::cloud_chamber_hal::units::{Celsius, HectoPascal, Volt};

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
    /// Toutes les cases valent `value_c`, horodatées à `Instant::new(0)`.
    pub fn new(value_c: f32) -> Self {
        let m = Measurement::new(Instant::new(0), Celsius(value_c));
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
        Duration::new(0)
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
    /// Toutes les cases valent `value_hpa`, horodatées à `Instant::new(0)`.
    pub fn new(value_hpa: f32) -> Self {
        let m = Measurement::new(Instant::new(0), HectoPascal(value_hpa));
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

/// Capteur de tension mock — implémente
/// `BatchSensor<Volt, NUMBER_OF_VOLTMETER>`.
pub struct MockVoltSensor {
    readings: [Result<Measurement<Volt>, MockSensorError>; NUMBER_OF_VOLTMETER],
}

impl MockVoltSensor {
    /// Toutes les cases valent `value_v`, horodatées à `Instant::new(0)`.
    pub fn new(value_v: f32) -> Self {
        let m = Measurement::new(Instant::new(0), Volt(value_v));
        Self { readings: core::array::from_fn(|_| Ok(m)) }
    }

    /// Fixe la lecture (ou l'erreur) d'un capteur individuel.
    pub fn set(&mut self, index: usize, reading: Result<Measurement<Volt>, MockSensorError>) {
        self.readings[index] = reading;
    }
}

impl BatchSensor<Volt, NUMBER_OF_VOLTMETER> for MockVoltSensor {
    type Error = MockSensorError;

    fn read(&mut self) -> [Result<Measurement<Volt>, Self::Error>; NUMBER_OF_VOLTMETER] {
        self.readings
    }
}
