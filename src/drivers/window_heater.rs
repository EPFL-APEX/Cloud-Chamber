//! Driver chauffage de la vitre supérieure : sortie GPIO tout-ou-rien
//! (marche/arrêt) — anti-buée/anti-givre.

use crate::cloud_chamber_hal::actuators::BinaryActuator;
use embedded_hal::digital::OutputPin;

/// Chauffage de la vitre supérieure, piloté par une sortie GPIO.
pub struct WindowHeater<P>
where
    P: OutputPin,
{
    activation_pin: P,
    is_on: bool,
}

impl<P> WindowHeater<P>
where
    P: OutputPin,
{
    /// Force la sortie à l'état bas à la construction — même pattern que
    /// `Lights`/`Pump` (évite un état flottant/indéterminé de la broche
    /// avant le premier `turn_on`/`turn_off` explicite).
    pub fn new(mut activation_pin: P) -> Self {
        let _ = activation_pin.set_low();
        Self { activation_pin, is_on: false }
    }

    /// État courant — diagnostic uniquement, pas dans `BinaryActuator`
    /// (le trait générique ne porte pas de méthode de lecture d'état).
    pub fn is_on(&self) -> bool {
        self.is_on
    }
}

impl<P> BinaryActuator for WindowHeater<P>
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
        fn new() -> Self { Self { state: true } }
    }

    impl embedded_hal::digital::ErrorType for MockPin {
        type Error = core::convert::Infallible;
    }

    impl OutputPin for MockPin {
        fn set_high(&mut self) -> Result<(), Self::Error> { self.state = true; Ok(()) }
        fn set_low(&mut self) -> Result<(), Self::Error> { self.state = false; Ok(()) }
    }

    #[test]
    fn new_forces_pin_low() {
        let heater = WindowHeater::new(MockPin::new());
        assert!(!heater.activation_pin.state);
        assert!(!heater.is_on());
    }

    #[test]
    fn turn_on_drives_pin_high_and_updates_state() {
        let mut heater = WindowHeater::new(MockPin::new());
        heater.turn_on().unwrap();
        assert!(heater.activation_pin.state);
        assert!(heater.is_on());
    }

    #[test]
    fn turn_off_drives_pin_low_and_updates_state() {
        let mut heater = WindowHeater::new(MockPin::new());
        heater.turn_on().unwrap();
        heater.turn_off().unwrap();
        assert!(!heater.activation_pin.state);
        assert!(!heater.is_on());
    }
}
