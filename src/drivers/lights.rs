//! Driver éclairage : sortie GPIO tout-ou-rien (marche/arrêt).

use crate::cloud_chamber_hal::actuators::BinaryActuator;
use embedded_hal::digital::OutputPin;

/// Éclairage de la chambre, piloté par une sortie GPIO.
pub struct Lights<P>
where
    P: OutputPin,
{
    activation_pin: P,
    is_on: bool,
}

impl<P> Lights<P>
where
    P: OutputPin,
{
    /// Force la sortie à l'état bas à la construction — évite un état
    /// flottant/indéterminé de la broche avant le premier `turn_on`/`turn_off`
    /// explicite. Best-effort : `new` reste infaillible comme les autres
    /// constructeurs de `drivers/`, une éventuelle erreur matérielle ici
    /// sera de toute façon revue au premier appel réel de `turn_on`/`turn_off`.
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

impl<P> BinaryActuator for Lights<P>
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
        /// Démarre à `true` (haut) : `new_forces_pin_low` ne serait pas
        /// probant si le mock démarrait déjà bas comme dans les autres
        /// drivers — ici on veut vérifier que `Lights::new` agit vraiment.
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
        let lights = Lights::new(MockPin::new());
        assert!(!lights.activation_pin.state);
        assert!(!lights.is_on());
    }

    #[test]
    fn turn_on_drives_pin_high_and_updates_state() {
        let mut lights = Lights::new(MockPin::new());
        lights.turn_on().unwrap();
        assert!(lights.activation_pin.state);
        assert!(lights.is_on());
    }

    #[test]
    fn turn_off_drives_pin_low_and_updates_state() {
        let mut lights = Lights::new(MockPin::new());
        lights.turn_on().unwrap();
        lights.turn_off().unwrap();
        assert!(!lights.activation_pin.state);
        assert!(!lights.is_on());
    }
}
