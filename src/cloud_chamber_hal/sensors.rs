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
    cloud_chamber_hal::{timer::Instant, units::{Celsius, HectoPascal, Volt}},
    config::{NUMBER_OF_PRESSURE_SENSOR, NUMBER_OF_TEMP_SENSOR, NUMBER_OF_VOLTMETER}
};

pub trait Sensor<T> {
    type Error: Debug;
    fn read(&mut self) -> Result<T, Self::Error>;
}

pub struct Measurement<Unit> {
    pub time: Instant,
    pub value: Unit,
}

pub struct Sensors<T, P, V>
where
    T: Sensor<Measurement<Celsius>>,
    P: Sensor<Measurement<HectoPascal>>,
    V: Sensor<Measurement<Volt>>,
{
    pub temperature_sensors: [T; NUMBER_OF_TEMP_SENSOR],
    pub pressure_sensors: [P; NUMBER_OF_PRESSURE_SENSOR],
    pub voltage_sensors: [V; NUMBER_OF_VOLTMETER],
}

impl<T, P, V> Sensors<T, P, V>
where
    T: Sensor<Measurement<Celsius>>,
    P: Sensor<Measurement<HectoPascal>>,
    V: Sensor<Measurement<Volt>>,
{
    pub fn new(
        temperature_sensors: [T; NUMBER_OF_TEMP_SENSOR],
        pressure_sensors: [P; NUMBER_OF_PRESSURE_SENSOR],
        voltage_sensors: [V; NUMBER_OF_VOLTMETER],
    ) -> Self {
        Self { temperature_sensors, pressure_sensors, voltage_sensors }
    }
}
