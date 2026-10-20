//! Driver compresseur : relais GPIO régulé par hystérésis autour d'une
//! température cible.
//!
//! Le corps est celui de [`crate::drivers::regulated_relay::RegulatedRelay`]
//! — cf. sa doc pour le pourquoi du marqueur de rôle et du sens porté par
//! le type.

use crate::drivers::regulate_method::RegulationDirection;
use crate::drivers::regulated_relay::{RegulatedRelay, RegulationRole};

/// Marqueur de rôle : le compresseur **refroidit** — actif quand la
/// lecture est AU-DESSUS de la cible.
pub struct CompressorRole;

impl RegulationRole for CompressorRole {
    const DIRECTION: RegulationDirection = RegulationDirection::Upward;
}

/// Compresseur de la boucle de refroidissement, piloté par un relais GPIO.
///
/// Implémente [`BinaryActuator`](crate::cloud_chamber_hal::actuators::BinaryActuator)
/// pour le pilotage direct (on/off) et
/// [`TargetActuator`](crate::cloud_chamber_hal::actuators::TargetActuator)
/// pour la régulation par hystérésis autour d'une température cible.
pub type Compressor<P> = RegulatedRelay<P, CompressorRole>;

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::cloud_chamber_hal::{
        measurement::Measurement, ring_buffer::RingBuffer, timer::Instant, units::Celsius,
    };
    use crate::cloud_chamber_hal::actuators::TargetActuator;
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

    /// Le sens qui distingue ce rôle de `HeaterRole` : trop chaud, donc
    /// on refroidit. Avec `Downward` (celui du chauffage), ce test
    /// échouerait — c'est précisément le bug qu'il garde.
    #[test]
    fn the_compressor_cools_when_it_is_too_hot() {
        let mut c: Compressor<MockPin> = Compressor::new(MockPin, Celsius(1.0));
        c.regulate(&history_at(6.5), Some(Celsius(5.0))).unwrap();
        assert!(c.is_on(), "trop chaud : le compresseur doit s'allumer");
    }

    /// Et le marqueur porte bien le sens déclaré : deux rôles distincts
    /// devant la même lecture décident opposé.
    #[test]
    fn the_role_carries_the_upward_direction() {
        assert_eq!(<CompressorRole as RegulationRole>::DIRECTION, RegulationDirection::Upward);
    }
}
