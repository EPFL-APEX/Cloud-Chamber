//! Driver compresseur : relais GPIO régulé par hystérésis autour d'une
//! température cible.

use crate::cloud_chamber_hal::{
    actuators::{BinaryActuator, TargetActuator},
    measurement::Measurement,
    ring_buffer::RingBuffer,
    units::Celsius,
};
use crate::drivers::regulate_method::{hysteresis, RegulationDirection};

use embedded_hal::digital::OutputPin;

/// Compresseur de la boucle de refroidissement, piloté par un relais GPIO.
///
/// Implémente [`BinaryActuator`] pour le pilotage direct (on/off) et
/// [`TargetActuator<Celsius, N>`] pour la régulation par hystérésis autour
/// d'une température cible.
pub struct Compressor<P>
where
    P: OutputPin,
{
    relay_pin: P,
    hysteresis_band: Celsius,
    is_on: bool,
}

impl<P> Compressor<P>
where
    P: OutputPin,
{
    /// `hysteresis_band` est la demi-largeur (positive) de la bande morte
    /// autour de la cible : le compresseur s'active au-delà de
    /// `target + band` et se coupe en deçà de `target - band`.
    pub fn new(relay_pin: P, hysteresis_band: Celsius) -> Self {
        relay_pin.set_low();
        Self { relay_pin, hysteresis_band, is_on: false }
    }

    /// État courant — diagnostic uniquement, pas dans `BinaryActuator`
    /// (le trait générique ne porte pas de méthode de lecture d'état).
    pub fn is_on(&self) -> bool {
        self.is_on
    }
}

impl<P> BinaryActuator for Compressor<P>
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

impl<P, const N: usize> TargetActuator<Celsius, N> for Compressor<P>
where
    P: OutputPin,
{
    type Error = P::Error;

    /// `target: None` coupe le compresseur sans consulter l'historique
    /// (contrat de [`TargetActuator`]) ; idem si `hist` ne contient encore
    /// aucune mesure (`get(0)` en erreur) — pas de régulation possible sans
    /// lecture, mieux vaut couper que deviner.
    fn regulate(
        &mut self, hist: &RingBuffer<Measurement<Celsius>, N>, target: Option<Celsius>,
    ) -> Result<(), Self::Error> {
        let Some(target_value) = target else {
            return self.turn_off();
        };
        let Ok(current) = hist.get(0) else {
            return self.turn_off();
        };

        let should_turn_on = hysteresis(
            current.value, target_value, self.hysteresis_band, self.is_on, RegulationDirection::Upward,
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
        state: bool,
    }

    impl MockPin {
        fn new() -> Self { Self { state: false } }
    }

    impl embedded_hal::digital::ErrorType for MockPin {
        type Error = core::convert::Infallible;
    }

    impl OutputPin for MockPin {
        fn set_high(&mut self) -> Result<(), Self::Error> { self.state = true; Ok(()) }
        fn set_low(&mut self) -> Result<(), Self::Error> { self.state = false; Ok(()) }
    }

    /// Historique à une seule lecture récente valant `value_c`. Part de
    /// `RingBuffer::filled` (pas `::new()` — `Measurement` n'implémente pas
    /// `Default`) puis pousse une vraie valeur : `filled()` seul laisse le
    /// buffer marqué vide (`get(0)` en erreur), cf. doc de `RingBuffer`.
    fn history_of_one(value_c: f32) -> RingBuffer<Measurement<Celsius>, 4> {
        let mut hist = RingBuffer::filled(Measurement::new(Instant::from_micros(0), Celsius(0.0)));
        hist.push(Measurement::new(Instant::from_micros(1), Celsius(value_c)));
        hist
    }

    #[test]
    fn turn_on_drives_pin_high_and_updates_state() {
        let mut c = Compressor::new(MockPin::new(), Celsius(1.0));
        c.turn_on().unwrap();
        assert!(c.relay_pin.state);
        assert!(c.is_on());
    }

    #[test]
    fn turn_off_drives_pin_low_and_updates_state() {
        let mut c = Compressor::new(MockPin::new(), Celsius(1.0));
        c.turn_on().unwrap();
        c.turn_off().unwrap();
        assert!(!c.relay_pin.state);
        assert!(!c.is_on());
    }

    #[test]
    fn regulate_turns_on_when_above_target_plus_band() {
        let mut c = Compressor::new(MockPin::new(), Celsius(1.0));
        c.regulate(&history_of_one(10.0), Some(Celsius(5.0))).unwrap();
        assert!(c.is_on());
    }

    #[test]
    fn regulate_stays_off_inside_band_when_starting_off() {
        let mut c = Compressor::new(MockPin::new(), Celsius(1.0));
        c.regulate(&history_of_one(5.5), Some(Celsius(5.0))).unwrap();
        assert!(!c.is_on());
    }

    #[test]
    fn regulate_stays_on_inside_band_once_already_on() {
        let mut c = Compressor::new(MockPin::new(), Celsius(1.0));
        c.regulate(&history_of_one(10.0), Some(Celsius(5.0))).unwrap();
        assert!(c.is_on());

        // Toujours dans la bande [target-band, target+band] : l'hystérésis
        // maintient l'état précédent plutôt que de re-décider sur un simple seuil.
        c.regulate(&history_of_one(5.5), Some(Celsius(5.0))).unwrap();
        assert!(c.is_on());
    }

    #[test]
    fn regulate_turns_off_at_target_minus_band() {
        let mut c = Compressor::new(MockPin::new(), Celsius(1.0));
        c.regulate(&history_of_one(10.0), Some(Celsius(5.0))).unwrap();
        assert!(c.is_on());

        c.regulate(&history_of_one(4.0), Some(Celsius(5.0))).unwrap();
        assert!(!c.is_on());
    }

    #[test]
    fn regulate_turns_off_on_none_target() {
        let mut c = Compressor::new(MockPin::new(), Celsius(1.0));
        c.regulate(&history_of_one(10.0), Some(Celsius(5.0))).unwrap();
        assert!(c.is_on());

        c.regulate(&history_of_one(10.0), None).unwrap();
        assert!(!c.is_on());
    }

    #[test]
    fn regulate_turns_off_on_empty_history() {
        let mut c = Compressor::new(MockPin::new(), Celsius(1.0));
        let hist: RingBuffer<Measurement<Celsius>, 4> =
            RingBuffer::filled(Measurement::new(Instant::from_micros(0), Celsius(0.0)));
        c.regulate(&hist, Some(Celsius(5.0))).unwrap();
        assert!(!c.is_on());
    }
}
