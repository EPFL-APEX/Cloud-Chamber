//! Historique glissant des mesures capteurs (buffers circulaires).
//!
//! # Rôle de ce module
//!
//! La boucle de sécurité ne travaille pas sur une seule mesure instantanée,
//! mais sur un **historique** des N dernières valeurs. Cela permet de détecter
//! des tendances (dérive progressive) et de filtrer les faux positifs (pic ponctuel).
//!
//! # Structure `SensorHistory`
//!
//! Contient un `RingBuffer` par capteur et par type de grandeur.
//! Les constantes `NUMBER_OF_TEMP_SENSOR`, `NUMBER_OF_VOLTMETER`, `NUMBER_OF_AMPMETER` sont
//! importées de `shared::data` pour garantir la cohérence avec `SensorSnapshot`.

use crate::security_loop::error::{Error, Result};
use crate::shared::{
    ring_buffer::RingBuffer,
};
use crate::config::{
    NUMBER_OF_TEMP_SENSOR,
    NUMBER_OF_PRESSURE_SENSOR,
    NUMBER_OF_VOLTMETER,
    NUMBER_OF_AMPMETER,
};

/// Nombre de mesures conservées par capteur.
const HISTORY_LENGTH: usize = 10;

/// Historique glissant pour tous les capteurs.
pub struct SensorHistory {
    temps: [RingBuffer<f32, HISTORY_LENGTH>; NUMBER_OF_TEMP_SENSOR],
    volts: [RingBuffer<f32, HISTORY_LENGTH>; NUMBER_OF_VOLTMETER],
    amps: [RingBuffer<f32, HISTORY_LENGTH>; NUMBER_OF_AMPMETER],
    closeness: RingBuffer<bool, HISTORY_LENGTH>,
}

impl SensorHistory {
    pub fn new() -> Self {
        Self {
            temps: core::array::from_fn(|_| RingBuffer::new()),
            volts: core::array::from_fn(|_| RingBuffer::new()),
            amps: core::array::from_fn(|_| RingBuffer::new()),
            closeness: RingBuffer::new(),
        }
    }
}

// ─── Push ─────────────────────────────────────────────────────────────────────

impl SensorHistory {
    pub fn push_temp(&mut self, sensor: usize, value: f32) -> Result<()> {
        if sensor >= NUMBER_OF_TEMP_SENSOR {
            return Err(Error::SensorIndexOutOfBounds { index: sensor });
        }
        self.temps[sensor].push(value);
        Ok(())
    }

    pub fn push_voltage(&mut self, sensor: usize, value: f32) -> Result<()> {
        if sensor >= NUMBER_OF_VOLTMETER {
            return Err(Error::SensorIndexOutOfBounds { index: sensor });
        }
        self.volts[sensor].push(value);
        Ok(())
    }

    pub fn push_amperage(&mut self, sensor: usize, value: f32) -> Result<()> {
        if sensor >= NUMBER_OF_AMPMETER {
            return Err(Error::SensorIndexOutOfBounds { index: sensor });
        }
        self.amps[sensor].push(value);
        Ok(())
    }

    pub fn push_closeness(&mut self, value: bool) {
        self.closeness.push(value);
    }
}

// ─── Get ──────────────────────────────────────────────────────────────────────

impl SensorHistory {
    pub fn get_temp(&self, sensor: usize, index: usize) -> Result<f32> {
        if sensor >= NUMBER_OF_TEMP_SENSOR {
            return Err(Error::SensorIndexOutOfBounds { index: sensor });
        }
        self.temps[sensor].get(index).map_err(|_| Error::HistoryIndexOutOfBounds { index })
    }

    pub fn get_voltage(&self, sensor: usize, index: usize) -> Result<f32> {
        if sensor >= NUMBER_OF_VOLTMETER {
            return Err(Error::SensorIndexOutOfBounds { index: sensor });
        }
        self.volts[sensor].get(index).map_err(|_| Error::HistoryIndexOutOfBounds { index })
    }

    pub fn get_amperage(&self, sensor: usize, index: usize) -> Result<f32> {
        if sensor >= NUMBER_OF_AMPMETER {
            return Err(Error::SensorIndexOutOfBounds { index: sensor });
        }
        self.amps[sensor].get(index).map_err(|_| Error::HistoryIndexOutOfBounds { index })
    }

    pub fn get_closeness(&self, index: usize) -> Result<bool> {
        self.closeness.get(index).map_err(|_| Error::HistoryIndexOutOfBounds { index })
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_get_temp() {
        let mut h = SensorHistory::new();
        h.push_temp(0, 25.0).unwrap();
        assert_eq!(h.get_temp(0, 0).unwrap(), 25.0);
    }

    #[test]
    fn push_temp_invalid_sensor_returns_err() {
        let mut h = SensorHistory::new();
        assert!(h.push_temp(NUMBER_OF_TEMP_SENSOR, 0.0).is_err());
    }

    #[test]
    fn push_and_get_voltage() {
        let mut h = SensorHistory::new();
        h.push_voltage(0, 12.0).unwrap();
        assert_eq!(h.get_voltage(0, 0).unwrap(), 12.0);
    }

    #[test]
    fn push_and_get_amperage() {
        let mut h = SensorHistory::new();
        h.push_amperage(0, 1.5).unwrap();
        assert_eq!(h.get_amperage(0, 0).unwrap(), 1.5);
    }

    #[test]
    fn push_amperage_invalid_sensor_returns_err() {
        let mut h = SensorHistory::new();
        assert!(h.push_amperage(NUMBER_OF_AMPMETER, 0.0).is_err());
    }

    #[test]
    fn push_and_get_closeness() {
        let mut h = SensorHistory::new();
        h.push_closeness(true);
        assert!(h.get_closeness(0).unwrap());
    }

    #[test]
    fn history_respects_ring_order() {
        let mut h = SensorHistory::new();
        h.push_temp(0, 20.0).unwrap();
        h.push_temp(0, 30.0).unwrap();
        assert_eq!(h.get_temp(0, 0).unwrap(), 30.0); // plus récente
        assert_eq!(h.get_temp(0, 1).unwrap(), 20.0); // précédente
    }

    #[test]
    fn get_before_push_returns_err() {
        let h = SensorHistory::new();
        assert!(h.get_temp(0, 0).is_err());
    }
}
