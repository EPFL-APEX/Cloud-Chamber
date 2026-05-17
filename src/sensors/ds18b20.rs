/// Driver DS18B20 via crate `onewire` (protocole 1-Wire).
///
/// Code bloquant, Rust stable, pas d'Embassy.
///
/// # Broche open-drain
///
/// Le protocole 1-Wire utilise le trait `OpenDrainOutput` de la crate `onewire`.
/// Tout type implémentant `InputPin + OutputPin` (même type d'erreur) satisfait
/// automatiquement ce trait via l'implémentation blanket du crate.
///
/// # Pattern delay
///
/// Le délai est passé explicitement à chaque méthode (pas stocké dans la struct).
/// Cela permet au binaire d'utiliser le même timer pour autre chose (ex. USB polling)
/// pendant la conversion de 800ms.

use onewire::{DeviceSearch, OneWire, OpenDrainOutput, DS18B20};
use embedded_hal::delay::DelayNs;
use heapless::Vec;

use super::TemperatureSensor;
use crate::config::CRITICAL_TEMP_INDICES;
use crate::data::TemperatureReading;

/// Code de famille du DS18B20 (fixe selon la datasheet)
const DS18B20_FAMILY: u8 = 0x28;

pub const MAX_SENSORS: usize = 5;

// ════════════════════════════════════════════════════════════════════════════
// Erreur
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug)]
pub enum Ds18b20Error {
    Bus,
    NoSensor,
}

// ════════════════════════════════════════════════════════════════════════════
// Bus multi-capteurs
// ════════════════════════════════════════════════════════════════════════════

/// Gère un bus 1-Wire avec plusieurs DS18B20.
pub struct Ds18b20Bus<P: OpenDrainOutput> {
    ow:      OneWire<P>,
    sensors: Vec<DS18B20, MAX_SENSORS>,
}

impl<P: OpenDrainOutput> Ds18b20Bus<P> {
    pub fn new(pin: P) -> Self {
        Self { ow: OneWire::new(pin, false), sensors: Vec::new() }
    }

    /// Recherche tous les DS18B20 sur le bus. Appeler une fois au démarrage.
    pub fn discover<D: DelayNs>(&mut self, delay: &mut D) -> usize {
        self.sensors.clear();
        let mut search = DeviceSearch::new_for_family(DS18B20_FAMILY);
        loop {
            match self.ow.search_next(&mut search, delay) {
                Ok(Some(device)) => {
                    if let Ok(sensor) = DS18B20::new(device) {
                        let _ = self.sensors.push(sensor);
                    }
                }
                Ok(None) | Err(_) => break,
            }
        }
        self.sensors.len()
    }

    /// Envoie la commande Convert T au capteur `index` (sans attendre).
    /// Appeler `read_celsius(index, delay)` après ~800ms.
    pub fn start_conversion<D: DelayNs>(
        &mut self, index: usize, delay: &mut D,
    ) -> Result<(), Ds18b20Error> {
        // Destructuration pour éviter les conflits d'emprunt entre `sensors` et `ow`
        let Ds18b20Bus { sensors, ow, .. } = self;
        let sensor = sensors.get(index).ok_or(Ds18b20Error::NoSensor)?;
        sensor.measure_temperature(ow, delay)
            .map(|_| ())   // measure_temperature renvoie MeasureResolution, pas ()
            .map_err(|_| Ds18b20Error::Bus)
    }

    /// Lit le scratchpad du capteur `index`. À appeler après le délai de conversion.
    pub fn read_celsius<D: DelayNs>(
        &mut self, index: usize, delay: &mut D,
    ) -> Result<f32, Ds18b20Error> {
        // Destructuration pour éviter les conflits d'emprunt entre `sensors` et `ow`
        let Ds18b20Bus { sensors, ow, .. } = self;
        let sensor = sensors.get(index).ok_or(Ds18b20Error::NoSensor)?;
        let raw = sensor.read_temperature(ow, delay)
            .map_err(|_| Ds18b20Error::Bus)?;
        // raw est u16 ; cast en i16 pour obtenir les températures négatives
        Ok(raw as i16 as f32 / 16.0)
    }

    pub fn sensor_count(&self) -> usize { self.sensors.len() }

    /// Lecture complète de tous les capteurs (bloquant 800 ms par capteur).
    ///
    /// A utiliser uniquement dans le firmware principal ou les taches
    /// qui peuvent bloquer librement. Pour un usage avec USB polling
    /// simultane, utiliser `start_conversion` + `wait_ms_usb` + `read_celsius`.
    pub fn read_all<D: DelayNs>(&mut self, delay: &mut D) -> [TemperatureReading; MAX_SENSORS] {
        let mut readings = [TemperatureReading::default(); MAX_SENSORS];
        for idx in 0..self.sensors.len() {
            let is_critical = CRITICAL_TEMP_INDICES.contains(&idx);
            if self.start_conversion(idx, delay).is_ok() {
                delay.delay_ms(800); // attente conversion 12 bits (750 ms max)
                readings[idx] = match self.read_celsius(idx, delay) {
                    Ok(t)  => TemperatureReading { value: t,        valid: true,  critical: is_critical },
                    Err(_) => TemperatureReading { value: f32::NAN, valid: false, critical: is_critical },
                };
            }
        }
        readings
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Wrapper single-capteur implémentant TemperatureSensor
// (pour le firmware principal où on peut bloquer librement)
// ════════════════════════════════════════════════════════════════════════════

pub struct Ds18b20Sensor<P: OpenDrainOutput, D> {
    bus:   Ds18b20Bus<P>,
    delay: D,
    index: usize,
}

impl<P: OpenDrainOutput, D: DelayNs> Ds18b20Sensor<P, D> {
    pub fn new(bus: Ds18b20Bus<P>, delay: D, index: usize) -> Self {
        Self { bus, delay, index }
    }
}

impl<P: OpenDrainOutput, D: DelayNs> TemperatureSensor for Ds18b20Sensor<P, D> {
    type Error = Ds18b20Error;

    /// Envoie Convert T — ne bloque PAS pendant la conversion.
    /// L'appelant doit attendre ~800ms avant read_celsius().
    fn start_measurement(&mut self) -> Result<(), Self::Error> {
        self.bus.start_conversion(self.index, &mut self.delay)
    }

    fn read_celsius(&mut self) -> Result<f32, Self::Error> {
        self.bus.read_celsius(self.index, &mut self.delay)
    }
}
