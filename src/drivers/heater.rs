//! Driver chauffage résistif : relais GPIO régulé par hystérésis autour
//! d'une température cible.
//!
//! Le corps est celui de [`crate::drivers::regulated_relay::RegulatedRelay`]
//! — cf. sa doc pour le pourquoi du marqueur de rôle et du sens porté par
//! le type. Jumeau de [`crate::drivers::compressor::Compressor`], au sens
//! de régulation près : un chauffage s'active quand il fait **trop
//! froid**, un compresseur quand il fait trop chaud. Le sens vit dans le
//! type de rôle, pas dans un booléen de configuration — se tromper ici,
//! c'est chauffer l'isopropanol quand il est déjà trop chaud.

use crate::drivers::regulate_method::RegulationDirection;
use crate::drivers::regulated_relay::{RegulatedRelay, RegulationRole};

/// Marqueur de rôle : le chauffage **chauffe** — actif quand la lecture
/// est EN DESSOUS de la cible.
pub struct HeaterRole;

impl RegulationRole for HeaterRole {
    const DIRECTION: RegulationDirection = RegulationDirection::Downward;
}

/// Chauffage résistif piloté par un relais GPIO (thermostat isopropanol).
///
/// Implémente [`BinaryActuator`](crate::cloud_chamber_hal::actuators::BinaryActuator)
/// pour le pilotage direct (on/off) et
/// [`TargetActuator`](crate::cloud_chamber_hal::actuators::TargetActuator)
/// pour la régulation par hystérésis autour d'une température cible.
pub type Heater<P> = RegulatedRelay<P, HeaterRole>;

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::cloud_chamber_hal::{
        actuators::TargetActuator, measurement::Measurement, ring_buffer::RingBuffer,
        timer::Instant, units::Celsius,
    };
    use crate::drivers::regulate_method::RegulationDirection;

    use super::*;

    /// Broche fictive, cf. `drivers::relay::tests` pour le même motif.
    struct MockPin;

    impl embedded_hal::digital::ErrorType for MockPin {
        type Error = core::convert::Infallible;
    }

    impl embedded_hal::digital::OutputPin for MockPin {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
        fn set_high(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    fn history_at(value: f32) -> RingBuffer<Measurement<Celsius>, 4> {
        let mut hist =
            RingBuffer::filled(Measurement::new(Instant::from_micros(0), Celsius(0.0)));
        hist.push(Measurement::new(Instant::from_micros(1), Celsius(value)));
        hist
    }

    /// Le sens qui distingue ce rôle de `CompressorRole` : trop froid,
    /// donc on chauffe. Avec `Upward` (celui du compresseur), ce test
    /// échouerait — c'est précisément le bug qu'il garde.
    #[test]
    fn the_heater_heats_when_it_is_too_cold() {
        let mut h: Heater<MockPin> = Heater::new(MockPin, Celsius(2.0));
        h.regulate(&history_at(36.5), Some(Celsius(40.0))).unwrap();
        assert!(h.is_on(), "trop froid : le chauffage doit s'allumer");
    }

    #[test]
    fn the_role_carries_the_downward_direction() {
        assert_eq!(<HeaterRole as RegulationRole>::DIRECTION, RegulationDirection::Downward);
    }
}
