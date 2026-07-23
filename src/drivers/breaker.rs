//! Driver disjoncteur via GPIO.
//!
//! # Logique active-haute / active-basse
//!
//! Selon le câblage du relai, une sortie GPIO à `HIGH` peut soit fermer
//! (active-haute) soit ouvrir (active-basse) le circuit. Le paramètre
//! `active_high` permet de gérer les deux cas sans dupliquer la logique.

use crate::cloud_chamber_hal::actuators::BreakerActuator;
use embedded_hal::digital::OutputPin;

/// Disjoncteur contrôlé par une sortie GPIO.
///
/// # Paramètre générique `Pin`
///
/// `Pin` doit implémenter `embedded_hal::digital::OutputPin`.
/// Cela permet d'utiliser n'importe quelle broche GPIO compatible,
/// qu'elle vienne du HAL RP2040, RP2350 ou d'un mock de test.
pub struct GpioBreaker<Pin: OutputPin> {
    pin: Pin,
    /// Si `true`, `HIGH` = déclenché. Si `false`, `LOW` = déclenché.
    active_high: bool,
    tripped: bool,
}

impl<Pin: OutputPin> GpioBreaker<Pin> {
    pub fn new(pin: Pin, active_high: bool) -> Self {
        Self { pin, active_high, tripped: false }
    }

    fn set_output(&mut self, tripped: bool) -> Result<(), Pin::Error> {
        let level = tripped == self.active_high;
        if level {
            self.pin.set_high()
        } else {
            self.pin.set_low()
        }
    }
}

impl<Pin: OutputPin> BreakerActuator for GpioBreaker<Pin> {
    type Error = Pin::Error;

    fn trip(&mut self) -> Result<(), Self::Error> {
        self.set_output(true)?;
        self.tripped = true;
        Ok(())
    }

    fn reset(&mut self) -> Result<(), Self::Error> {
        self.set_output(false)?;
        self.tripped = false;
        Ok(())
    }

    fn is_tripped(&self) -> Result<bool, Self::Error> {
        Ok(self.tripped)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock de broche GPIO pour les tests.
    struct MockPin {
        pub state: bool,
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
    fn trip_sets_tripped_state() {
        let mut breaker = GpioBreaker::new(MockPin::new(), true);
        breaker.trip().unwrap();
        assert!(breaker.is_tripped().unwrap());
    }

    #[test]
    fn reset_clears_tripped_state() {
        let mut breaker = GpioBreaker::new(MockPin::new(), true);
        breaker.trip().unwrap();
        breaker.reset().unwrap();
        assert!(!breaker.is_tripped().unwrap());
    }

    #[test]
    fn active_high_trip_drives_pin_high() {
        let mut breaker = GpioBreaker::new(MockPin::new(), true);
        breaker.trip().unwrap();
        assert!(breaker.pin.state);
    }

    #[test]
    fn active_low_trip_drives_pin_low() {
        let mut breaker = GpioBreaker::new(MockPin::new(), false);
        breaker.trip().unwrap();
        assert!(!breaker.pin.state);
    }

    #[test]
    fn initial_state_is_not_tripped() {
        let breaker = GpioBreaker::new(MockPin::new(), true);
        assert!(!breaker.is_tripped().unwrap());
    }
}
