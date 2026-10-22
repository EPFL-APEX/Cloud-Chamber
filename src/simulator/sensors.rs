//! Capteurs simulés : lisent la physique, implémentent les traits du HAL.

use crate::cloud_chamber_hal::config::{NUMBER_OF_PRESSURE_SENSOR, NUMBER_OF_TEMP_SENSOR};
use crate::cloud_chamber_hal::measurement::Measurement;
use crate::cloud_chamber_hal::sensors::{BatchSensor, DeferredBatchSensor, Sensor};
use crate::cloud_chamber_hal::timer::{Duration, MonotonicTimer};
use crate::cloud_chamber_hal::units::{Celsius, HectoPascal};

use super::clock::VirtualClock;
use super::physics::ChamberPhysics;

/// Erreur de lecture simulée : une sonde en panne (injectée par le
/// scénario), rien d'aléatoire — la panne est un outil de test, pas
/// une loterie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimSensorError;

/// Les 8 sondes de température : lisent la physique à chaque conversion,
/// avec le timing deux-phases du vrai bus 1-Wire (start + attente +
/// lecture) pour que la boucle de contrôle s'exerce comme en vrai.
pub struct SimTempSensors<'a> {
    physics: std::rc::Rc<std::cell::RefCell<ChamberPhysics>>,
    clock: &'a VirtualClock,
    /// Résultats de la dernière conversion — comme le vrai DS18B20 :
    /// `start_conversion` déclenche, `read_result` rend la valeur d'avant.
    pending: [Option<Measurement<crate::cloud_chamber_hal::units::Celsius>>; NUMBER_OF_TEMP_SENSOR],
}

impl<'a> SimTempSensors<'a> {
    pub fn new(
        physics: std::rc::Rc<std::cell::RefCell<ChamberPhysics>>,
        clock: &'a VirtualClock,
    ) -> Self {
        Self { physics, clock, pending: [None; NUMBER_OF_TEMP_SENSOR] }
    }
}

impl BatchSensor<Celsius, NUMBER_OF_TEMP_SENSOR> for SimTempSensors<'_> {
    type Error = SimSensorError;

    fn read(
        &mut self,
    ) -> [Result<Measurement<Celsius>, Self::Error>; NUMBER_OF_TEMP_SENSOR] {
        self.start_conversion().ok();
        // En simulation la conversion est immédiate : le pas de physique
        // est notre « attente ». On rend le résultat courant.
        self.read_result()
    }
}

impl DeferredBatchSensor<Celsius, NUMBER_OF_TEMP_SENSOR> for SimTempSensors<'_> {
    fn start_conversion(&mut self) -> Result<(), Self::Error> {
        let mut physics = self.physics.borrow_mut();
        for i in 0..NUMBER_OF_TEMP_SENSOR {
            self.pending[i] = physics
                .read_temp(i)
                .map(|t| Measurement::new(self.clock.now(), Celsius(t)));
        }
        Ok(())
    }

    fn conversion_time_ms(&self) -> Duration {
        // Le DS18B20 en 12 bits : 750 ms. La boucle de contrôle réelle
        // en tient compte dans son `ProbingPlan` — la simulation doit
        // présenter la même facture temporelle.
        Duration::from_millis(750)
    }

    fn read_result(
        &mut self,
    ) -> [Result<Measurement<Celsius>, Self::Error>; NUMBER_OF_TEMP_SENSOR] {
        core::array::from_fn(|i| self.pending[i].ok_or(SimSensorError))
    }
}

/// Le capteur de pression : atmosphérique (la chambre n'est pas
/// pressurisée — cf. la décision documentée dans `logic::security`),
/// avec une dérive lente pour le réalisme des relevés d'expérience.
pub struct SimPressureSensor<'a> {
    clock: &'a VirtualClock,
    rng: u32,
}

impl<'a> SimPressureSensor<'a> {
    pub fn new(clock: &'a VirtualClock) -> Self {
        Self { clock, rng: 0x1234_5678 }
    }

    /// Pression atmosphérique simulée ~1013 hPa avec dérive lente.
    fn hpa(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        1013.25 + ((x as f32) / (u32::MAX as f32) - 0.5) * 4.0
    }
}

impl BatchSensor<HectoPascal, NUMBER_OF_PRESSURE_SENSOR> for SimPressureSensor<'_> {
    type Error = SimSensorError;

    fn read(&mut self) -> [Result<Measurement<HectoPascal>, Self::Error>; NUMBER_OF_PRESSURE_SENSOR] {
        let m = Measurement::new(self.clock.now(), HectoPascal::new(self.hpa()));
        [Ok(m)]
    }
}

/// `IndependentSensors` enveloppe des `Sensor<T>` individuels : le
/// capteur de pression doit donc exister des deux façons — seul (ce
/// trait) et groupé (`BatchSensor` ci-dessus, dont l'implémentation par
/// défaut de `IndependentSensors` se servira).
impl Sensor<Measurement<HectoPascal>> for SimPressureSensor<'_> {
    type Error = SimSensorError;

    fn read(&mut self) -> Result<Measurement<HectoPascal>, Self::Error> {
        Ok(Measurement::new(self.clock.now(), HectoPascal::new(self.hpa())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_chamber_hal::units::Celsius;

    #[test]
    fn temp_sensors_read_the_physics() {
        let clock = VirtualClock::new();
        let physics = std::rc::Rc::new(std::cell::RefCell::new(ChamberPhysics::new(Default::default())));
        let mut sensors = SimTempSensors::new(std::rc::Rc::clone(&physics), &clock);

        let readings = sensors.read();
        for (i, r) in readings.iter().enumerate() {
            assert!(r.is_ok(), "sonde {i} doit lire");
            let m: Measurement<Celsius> = r.as_ref().unwrap().clone();
            assert!((m.value.0 - 20.0).abs() < 0.2, "a l'ambiant au depart");
        }
    }

    #[test]
    fn a_failed_probe_reads_err_while_others_stay_ok() {
        let clock = VirtualClock::new();
        let physics = std::rc::Rc::new(std::cell::RefCell::new(ChamberPhysics::new(Default::default())));
        physics.borrow_mut().apply_power(super::super::physics::PowerState {
            cooling: true,
            compressor_probe_failed: true,
            ..Default::default()
        });
        let mut sensors = SimTempSensors::new(std::rc::Rc::clone(&physics), &clock);

        let readings = sensors.read();
        assert!(readings[crate::cloud_chamber_hal::config::COMPRESSOR_IDX].is_err());
        for (i, r) in readings.iter().enumerate() {
            if i != crate::cloud_chamber_hal::config::COMPRESSOR_IDX {
                assert!(r.is_ok(), "sonde {i} doit rester lisible");
            }
        }
    }

    #[test]
    fn pressure_stays_around_atmospheric() {
        let clock = VirtualClock::new();
        let mut sensor = SimPressureSensor::new(&clock);
        for _ in 0..100 {
            use crate::cloud_chamber_hal::sensors::Sensor as _;
            let reading = Sensor::read(&mut sensor).unwrap().clone();
            assert!(
                (reading.value.0 - 1013.0).abs() < 5.0,
                "pression atmospherique plausible"
            );
        }
    }
}
