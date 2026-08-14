//! Driver pompe : sortie GPIO tout-ou-rien (marche/arrêt).

use crate::cloud_chamber_hal::actuators::BinaryActuator;
use embedded_hal::digital::OutputPin;

/// Pompe (ex. circulation isopropanol) pilotée par une sortie GPIO.
pub struct Pump<P>
where
    P: OutputPin,
{
    activation_pin: P,
    is_on: bool,
}

impl<P> Pump<P>
where
    P: OutputPin,
{
    pub fn new(activation_pin: P) -> Self {
        activation_pin.set_low();
        Self { activation_pin, is_on: false }
    }

    /// État courant — diagnostic uniquement, pas dans `BinaryActuator`
    /// (le trait générique ne porte pas de méthode de lecture d'état).
    pub fn is_on(&self) -> bool {
        self.is_on
    }
}

impl<P> BinaryActuator for Pump<P>
where
    P: OutputPin,
{
    type Error = P::Error;

    fn turn_on(&mut self) -> Result<(), Self::Error> {
        self.activation_pin.set_high()?;
        self.is_on = true;
        Ok(())
    }

    fn turn_off(&mut self) -> Result<(), Self::Error> {
        self.activation_pin.set_low()?;
        self.is_on = false;
        Ok(())
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn turn_on_drives_pin_high_and_updates_state() {
        let mut p = Pump::new(MockPin::new());
        p.turn_on().unwrap();
        assert!(p.activation_pin.state);
        assert!(p.is_on());
    }

    #[test]
    fn turn_off_drives_pin_low_and_updates_state() {
        let mut p = Pump::new(MockPin::new());
        p.turn_on().unwrap();
        p.turn_off().unwrap();
        assert!(!p.activation_pin.state);
        assert!(!p.is_on());
    }

    #[test]
    fn initial_state_is_off() {
        let p = Pump::new(MockPin::new());
        assert!(!p.is_on());
    }
}
