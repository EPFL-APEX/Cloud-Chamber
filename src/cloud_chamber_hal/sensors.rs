//! Traits pour les capteurs de mesure.
//!
//! # Pourquoi des traits séparés par type de capteur ?
//!
//! Chaque trait représente une **capacité** précise. Une structure peut
//! implémenter plusieurs traits (ex: un module I2C multi-capteurs).
//! La logique métier dépend uniquement du trait, pas du type concret :
//! on peut substituer n'importe quelle implémentation sans modifier
//! `SecurityLoop`.

use core::fmt::Debug;

use crate::{
    config::{NUMBER_OF_PRESSURE_SENSOR, NUMBER_OF_TEMP_SENSOR, NUMBER_OF_VOLTMETER},
    shared::data::{PressureReading, TemperatureReading, VoltsReading}
};

pub trait Sensor<T> {
    type Error: Debug;
    fn read(&mut self) -> Result<T, Self::Error>;
}

/// Capteur de température retournant des degrés Celsius.
pub trait TemperatureSensor : Sensor<TemperatureReading> {
    /// Déclenche une conversion (peut être asynchrone sur certains capteurs).
    fn start_measurement(&mut self) -> Result<(), Self::Error>;
    /// Lit la dernière température convertie, en °C.
    fn read_celsius(&mut self) -> Result<f32, Self::Error>;
}

/// Capteur de tension retournant des Volts.
pub trait VoltageSensor : Sensor<VoltsReading> {
    fn read_voltage(&mut self) -> Result<f32, Self::Error>;
}

/// Capteur de courant retournant des Ampères.
pub trait CurrentSensor : Sensor<T> {
    fn read_amperes(&mut self) -> Result<f32, Self::Error>;
}

/// Capteur de pression retournant des pascal
pub trait PressureSensor : Sensor<PressureReading> {
    fn read_pascal(&mut self) -> Result<f32, Self::Error>;
}

/// Capteur de fermeture (contact sec) retournant un booléen.
pub trait ClosureSensor : Sensor<T> {
    /// Retourne `true` si la chambre est physiquement fermée.
    fn is_closed(&mut self) -> Result<bool, Self::Error>;
}

pub struct Sensors<T: TemperatureSensor, P: PressureSensor, V: VoltageSensor> {
    pub temperature_sensors: [T; NUMBER_OF_TEMP_SENSOR],
    pub pressure_sensors: [P; NUMBER_OF_PRESSURE_SENSOR],
    pub voltage_sensors: [V; NUMBER_OF_VOLTMETER],
}

impl<T: TemperatureSensor, P: PressureSensor, V: VoltageSensor> Sensors<T, P, V> {
    pub fn new() -> Self {
        todo!()
    }
}