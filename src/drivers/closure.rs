//! Driver capteur de fermeture via GPIO.

use crate::cloud_chamber_hal::sensors::ClosureSensor;
use embedded_hal::digital::InputPin;

/// Capteur de fermeture basé sur une entrée GPIO avec pull-up interne.
///
/// La logique est configurable : `active_low = true` signifie que la broche
/// passe à LOW quand la chambre est fermée (contact normalement ouvert + pull-up).
pub struct GpioClosureSensor<Pin: InputPin> {
    pin: Pin,
    /// `true` → chambre fermée = broche LOW (contact NO + pull-up).
    /// `false` → chambre fermée = broche HIGH (contact NF + pull-down).
    active_low: bool,
}

impl<Pin: InputPin> GpioClosureSensor<Pin> {
    pub fn new(pin: Pin, active_low: bool) -> Self {
        Self { pin, active_low }
    }
}

impl<Pin: InputPin> ClosureSensor for GpioClosureSensor<Pin> {
    type Error = Pin::Error;

    fn is_closed(&mut self) -> Result<bool, Self::Error> {
        let high = self.pin.is_high()?;
        Ok(if self.active_low { !high } else { high })
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    struct MockPin {
        state: bool,
    }

    impl embedded_hal::digital::ErrorType for MockPin {
        type Error = core::convert::Infallible;
    }

    impl InputPin for MockPin {
        fn is_high(&mut self) -> Result<bool, Self::Error> { Ok(self.state) }
        fn is_low(&mut self) -> Result<bool, Self::Error> { Ok(!self.state) }
    }

    #[test]
    fn active_low_closed_when_pin_low() {
        let mut sensor = GpioClosureSensor::new(MockPin { state: false }, true);
        assert!(sensor.is_closed().unwrap());
    }

    #[test]
    fn active_low_open_when_pin_high() {
        let mut sensor = GpioClosureSensor::new(MockPin { state: true }, true);
        assert!(!sensor.is_closed().unwrap());
    }

    #[test]
    fn active_high_closed_when_pin_high() {
        let mut sensor = GpioClosureSensor::new(MockPin { state: true }, false);
        assert!(sensor.is_closed().unwrap());
    }
}
