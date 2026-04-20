//! Drivers ADC pour les capteurs de tension et de courant.
//!
//! # Conversion ADC → valeur physique
//!
//! L'ADC du RP2040/RP2350 est 12 bits (0–4095). La conversion en grandeur
//! physique dépend du gain du circuit de conditionnement :
//!
//! ```text
//! tension = (raw / 4095.0) * V_REF * GAIN_FACTOR
//! ```
//!
//! Les valeurs de `VOLTAGE_SCALE` et `CURRENT_SCALE` doivent être ajustées
//! selon le schéma électrique du projet.

use crate::cloud_chamber_hal::sensors::{CurrentSensor, VoltageSensor};

/// Facteur de conversion ADC → Volts (à calibrer selon le diviseur de tension).
const VOLTAGE_SCALE: f32 = 3.3 / 4095.0 * 11.0; // ex: diviseur 1/11 pour 33V max

/// Facteur de conversion ADC → Ampères (à calibrer selon le shunt/amplificateur).
const CURRENT_SCALE: f32 = 3.3 / 4095.0 / 0.1; // ex: shunt 0.1Ω, gain 1

/// Capteur de tension basé sur l'ADC embarqué.
///
/// # Paramètre générique `Channel`
///
/// En production, `Channel` sera le type de canal ADC du HAL embarqué.
/// Pour les tests, on peut substituer un type mock.
pub struct AdcVoltageSensor<Channel> {
    channel: Channel,
}

impl<Channel> AdcVoltageSensor<Channel> {
    pub fn new(channel: Channel) -> Self {
        Self { channel }
    }
}

impl<Channel> VoltageSensor for AdcVoltageSensor<Channel> {
    type Error = core::convert::Infallible;

    fn read_voltage(&mut self) -> Result<f32, Self::Error> {
        let raw = read_raw_stub();
        Ok(raw as f32 * VOLTAGE_SCALE)
    }
}

/// Capteur de courant basé sur l'ADC embarqué.
pub struct AdcCurrentSensor<Channel> {
    channel: Channel,
}

impl<Channel> AdcCurrentSensor<Channel> {
    pub fn new(channel: Channel) -> Self {
        Self { channel }
    }
}

impl<Channel> CurrentSensor for AdcCurrentSensor<Channel> {
    type Error = core::convert::Infallible;

    fn read_amperes(&mut self) -> Result<f32, Self::Error> {
        let raw = read_raw_stub();
        Ok(raw as f32 * CURRENT_SCALE)
    }
}

/// Lecture ADC brute — stub retournant mi-échelle.
///
/// À remplacer par un vrai appel HAL en production :
/// `adc.read(&mut channel).unwrap_or(0)`
#[inline]
fn read_raw_stub() -> u16 {
    2048
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    struct MockChannel;

    #[test]
    fn voltage_sensor_returns_positive_value() {
        let mut sensor = AdcVoltageSensor::new(MockChannel);
        let v = sensor.read_voltage().unwrap();
        assert!(v > 0.0, "tension doit être positive");
    }

    #[test]
    fn current_sensor_returns_positive_value() {
        let mut sensor = AdcCurrentSensor::new(MockChannel);
        let a = sensor.read_amperes().unwrap();
        assert!(a > 0.0, "courant doit être positif");
    }

    #[test]
    fn voltage_midscale_is_reasonable() {
        let mut sensor = AdcVoltageSensor::new(MockChannel);
        let v = sensor.read_voltage().unwrap();
        // 2048/4095 * 3.3 * 11 ≈ 18.17 V
        assert!(v > 10.0 && v < 25.0);
    }
}
