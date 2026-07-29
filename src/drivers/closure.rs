//! Driver capteur de fermeture via GPIO.

use crate::cloud_chamber_hal::sensors::Sensor;
use crate::cloud_chamber_hal::measurement::Measurement;
use crate::cloud_chamber_hal::timer::MonotonicTimer;
use embedded_hal::digital::InputPin;

/// Capteur de fermeture basé sur une entrée GPIO avec pull-up interne.
///
/// La logique est configurable : `active_low = true` signifie que la broche
/// passe à LOW quand la chambre est fermée (contact normalement ouvert + pull-up).
pub struct GpioClosureSensor<Pin: InputPin, Clk> {
    pin: Pin,
    /// `true` → chambre fermée = broche LOW (contact NO + pull-up).
    /// `false` → chambre fermée = broche HIGH (contact NF + pull-down).
    active_low: bool,
    clock: Clk,
}

impl<Pin: InputPin, Clk: MonotonicTimer> GpioClosureSensor<Pin, Clk> {
    pub fn new(pin: Pin, active_low: bool, clock: Clk) -> Self {
        Self { pin, active_low, clock }
    }
}

impl<Pin: InputPin, Clk: MonotonicTimer> Sensor<Measurement<bool>> for GpioClosureSensor<Pin, Clk> {
    type Error = Pin::Error;

    fn read(&mut self) -> Result<Measurement<bool>, Self::Error> {
        let high = self.pin.is_high()?;
        let closed = if self.active_low { !high } else { high };
        Ok(Measurement::new(self.clock.get_counter_us(), closed))
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

    /// Horloge mock retournant toujours l'instant zéro.
    struct MockClock;

    impl MonotonicTimer for MockClock {
        fn get_counter_us(&self) -> crate::cloud_chamber_hal::timer::Instant {
            crate::cloud_chamber_hal::timer::Instant::new(0)
        }
    }

    #[test]
    fn active_low_closed_when_pin_low() {
        let mut sensor = GpioClosureSensor::new(MockPin { state: false }, true, MockClock);
        assert!(sensor.read().unwrap().value);
    }

    #[test]
    fn active_low_open_when_pin_high() {
        let mut sensor = GpioClosureSensor::new(MockPin { state: true }, true, MockClock);
        assert!(!sensor.read().unwrap().value);
    }

    #[test]
    fn active_high_closed_when_pin_high() {
        let mut sensor = GpioClosureSensor::new(MockPin { state: true }, false, MockClock);
        assert!(sensor.read().unwrap().value);
    }
}
