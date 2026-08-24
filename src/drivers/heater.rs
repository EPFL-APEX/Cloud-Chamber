//! Driver chauffage résistif : relais GPIO régulé par hystérésis autour
//! d'une température cible.
//!
//! Jumeau de [`crate::drivers::compressor::Compressor`], au sens de
//! régulation près : un chauffage s'active quand il fait **trop froid**
//! ([`RegulationDirection::Downward`]), un compresseur quand il fait trop
//! chaud. C'est la seule différence, mais elle n'est pas paramétrable sur
//! `Compressor` — et il vaut mieux deux types explicites qu'un booléen de
//! configuration qu'on peut brancher à l'envers : se tromper de sens ici,
//! c'est chauffer l'isopropanol quand il est déjà trop chaud.

use crate::cloud_chamber_hal::{
    actuators::{BinaryActuator, TargetActuator},
    measurement::Measurement,
    ring_buffer::RingBuffer,
    units::Celsius,
};
use crate::drivers::regulate_method::{RegulationDirection, hysteresis};

use embedded_hal::digital::OutputPin;

/// Chauffage résistif piloté par un relais GPIO (thermostat isopropanol).
///
/// Implémente [`BinaryActuator`] pour le pilotage direct (on/off) et
/// [`TargetActuator<Celsius, N>`] pour la régulation par hystérésis.
pub struct Heater<P>
where
    P: OutputPin,
{
    relay_pin: P,
    hysteresis_band: Celsius,
    is_on: bool,
}

impl<P> Heater<P>
where
    P: OutputPin,
{
    /// `hysteresis_band` est la demi-largeur (positive) de la bande morte
    /// autour de la cible : le chauffage s'active en deçà de
    /// `target - band` et se coupe au-delà de `target + band`.
    pub fn new(mut relay_pin: P, hysteresis_band: Celsius) -> Self {
        let _ = relay_pin.set_low();
        Self { relay_pin, hysteresis_band, is_on: false }
    }

    /// État courant — diagnostic uniquement, pas dans `BinaryActuator`.
    pub fn is_on(&self) -> bool {
        self.is_on
    }
}

impl<P> BinaryActuator for Heater<P>
where
    P: OutputPin,
{
    type Error = P::Error;

    fn turn_on(&mut self) -> Result<(), Self::Error> {
        self.relay_pin.set_high()?;
        self.is_on = true;
        Ok(())
    }

    fn turn_off(&mut self) -> Result<(), Self::Error> {
        self.relay_pin.set_low()?;
        self.is_on = false;
        Ok(())
    }
}

impl<P, const N: usize> TargetActuator<Celsius, N> for Heater<P>
where
    P: OutputPin,
{
    type Error = P::Error;

    /// `target: None` coupe le chauffage sans consulter l'historique
    /// (contrat de [`TargetActuator`]) ; idem si `hist` ne contient encore
    /// aucune mesure — pas de régulation possible sans lecture, et un
    /// chauffage laissé en marche à l'aveugle est le mauvais défaut.
    fn regulate(
        &mut self,
        hist: &RingBuffer<Measurement<Celsius>, N>,
        target: Option<Celsius>,
    ) -> Result<(), Self::Error> {
        let Some(target_value) = target else {
            return self.turn_off();
        };
        let Ok(current) = hist.get(0) else {
            return self.turn_off();
        };

        let should_turn_on = hysteresis(
            current.value,
            target_value,
            self.hysteresis_band,
            self.is_on,
            RegulationDirection::Downward,
        );

        if should_turn_on { self.turn_on() } else { self.turn_off() }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_chamber_hal::timer::Instant;

    struct MockPin {
        is_high: bool,
    }

    impl embedded_hal::digital::ErrorType for MockPin {
        type Error = core::convert::Infallible;
    }

    impl OutputPin for MockPin {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            self.is_high = false;
            Ok(())
        }
        fn set_high(&mut self) -> Result<(), Self::Error> {
            self.is_high = true;
            Ok(())
        }
    }

    const BAND: Celsius = Celsius(2.0);
    const TARGET: Celsius = Celsius(40.0);

    fn heater() -> Heater<MockPin> {
        Heater::new(MockPin { is_high: false }, BAND)
    }

    /// `filled()` d'abord (`Measurement` n'implémente pas `Default`, donc
    /// pas de `RingBuffer::new()`), puis une vraie valeur : `filled()` seul
    /// laisse le buffer marqué vide — cf. doc de `RingBuffer` et le même
    /// motif dans les tests de `compressor`.
    fn history_at(value: f32) -> RingBuffer<Measurement<Celsius>, 4> {
        let mut hist =
            RingBuffer::filled(Measurement::new(Instant::from_micros(0), Celsius(0.0)));
        hist.push(Measurement::new(Instant::from_micros(1), Celsius(value)));
        hist
    }

    #[test]
    fn turn_on_drives_pin_high_and_updates_state() {
        let mut h = heater();
        h.turn_on().unwrap();
        assert!(h.relay_pin.is_high);
        assert!(h.is_on());
    }

    #[test]
    fn turn_off_drives_pin_low_and_updates_state() {
        let mut h = heater();
        h.turn_on().unwrap();
        h.turn_off().unwrap();
        assert!(!h.relay_pin.is_high);
        assert!(!h.is_on());
    }

    /// Le sens qui distingue ce driver du compresseur : trop froid, donc on
    /// chauffe. Avec `RegulationDirection::Upward` (celle du compresseur),
    /// ce test échouerait — c'est précisément le bug qu'il garde.
    #[test]
    fn regulate_turns_on_when_below_target_minus_band() {
        let mut h = heater();
        h.regulate(&history_at(TARGET.0 - BAND.0 - 1.0), Some(TARGET)).unwrap();
        assert!(h.is_on(), "trop froid : le chauffage doit s'allumer");
    }

    #[test]
    fn regulate_turns_off_at_target_plus_band() {
        let mut h = heater();
        h.turn_on().unwrap();
        h.regulate(&history_at(TARGET.0 + BAND.0 + 1.0), Some(TARGET)).unwrap();
        assert!(!h.is_on(), "trop chaud : le chauffage doit se couper");
    }

    #[test]
    fn regulate_stays_on_inside_band_once_already_on() {
        let mut h = heater();
        h.turn_on().unwrap();
        h.regulate(&history_at(TARGET.0), Some(TARGET)).unwrap();
        assert!(h.is_on(), "dans la bande morte : pas de changement d'etat");
    }

    #[test]
    fn regulate_stays_off_inside_band_when_starting_off() {
        let mut h = heater();
        h.regulate(&history_at(TARGET.0), Some(TARGET)).unwrap();
        assert!(!h.is_on(), "dans la bande morte : pas de changement d'etat");
    }

    #[test]
    fn regulate_turns_off_on_none_target() {
        let mut h = heater();
        h.turn_on().unwrap();
        h.regulate(&history_at(TARGET.0 - 10.0), None).unwrap();
        assert!(!h.is_on(), "cible absente : on coupe");
    }

    #[test]
    fn regulate_turns_off_on_empty_history() {
        let mut h = heater();
        h.turn_on().unwrap();
        // `filled()` sans `push()` : le buffer reste marqué vide.
        let empty: RingBuffer<Measurement<Celsius>, 4> =
            RingBuffer::filled(Measurement::new(Instant::from_micros(0), Celsius(0.0)));
        h.regulate(&empty, Some(TARGET)).unwrap();
        assert!(!h.is_on(), "aucune mesure : on coupe plutot que deviner");
    }
}
