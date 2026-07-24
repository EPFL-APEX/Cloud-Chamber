//! Driver encodeur rotatif quadrature avec bouton-poussoir.
//!
//! # Encodeur quadrature
//!
//! Un encodeur rotatif génère deux signaux en quadrature (A et B) déphasés
//! de 90°. En lisant l'état de A quand B change (ou vice-versa), on détermine
//! le sens de rotation. Ce driver utilise un polling simple (pas d'interruption)
//! adapté à une boucle de 10 ms.

use embedded_hal::digital::InputPin;

/// Événement produit par l'encodeur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderEvent {
    /// L'encodeur a tourné d'un cran dans le sens horaire.
    RotateClockwise,
    /// L'encodeur a tourné d'un cran dans le sens anti-horaire.
    RotateCounterClockwise,
    /// Le bouton central a été pressé (front descendant).
    ButtonPressed,
    /// Aucun événement détecté.
    None,
}

/// Encodeur rotatif avec trois broches : A, B et bouton.
pub struct RotaryEncoder<PinA, PinB, PinSw>
where
    PinA: InputPin,
    PinB: InputPin,
    PinSw: InputPin,
{
    pin_a: PinA,
    pin_b: PinB,
    pin_sw: PinSw,
    last_a: bool,
    last_sw: bool,
}

impl<PinA, PinB, PinSw> RotaryEncoder<PinA, PinB, PinSw>
where
    PinA: InputPin,
    PinB: InputPin,
    PinSw: InputPin,
{
    pub fn new(pin_a: PinA, pin_b: PinB, pin_sw: PinSw) -> Self {
        Self {
            pin_a,
            pin_b,
            pin_sw,
            last_a: false,
            last_sw: true, // pull-up : bouton relâché = HIGH
        }
    }

    /// Lit l'état des broches et retourne l'événement détecté.
    ///
    /// Doit être appelé à chaque itération de la boucle UI (~10–100 ms).
    pub fn poll(&mut self) -> EncoderEvent {
        let a = self.pin_a.is_high().unwrap_or(false);
        let b = self.pin_b.is_high().unwrap_or(false);
        let sw = self.pin_sw.is_high().unwrap_or(true);

        // Front descendant sur le bouton (pull-up : HIGH → LOW = pressé)
        let button_event = self.last_sw && !sw;
        self.last_sw = sw;

        // Front montant sur A → sens déterminé par B
        let rotation_event = !self.last_a && a;
        self.last_a = a;

        if button_event {
            EncoderEvent::ButtonPressed
        } else if rotation_event {
            if b { EncoderEvent::RotateCounterClockwise } else { EncoderEvent::RotateClockwise }
        } else {
            EncoderEvent::None
        }
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
        fn high() -> Self { Self { state: true } }
        fn low() -> Self { Self { state: false } }
        fn set(&mut self, v: bool) { self.state = v; }
    }

    impl embedded_hal::digital::ErrorType for MockPin {
        type Error = core::convert::Infallible;
    }

    impl InputPin for MockPin {
        fn is_high(&mut self) -> Result<bool, Self::Error> { Ok(self.state) }
        fn is_low(&mut self) -> Result<bool, Self::Error> { Ok(!self.state) }
    }

    #[test]
    fn no_event_when_stable() {
        let mut enc = RotaryEncoder::new(MockPin::low(), MockPin::low(), MockPin::high());
        assert_eq!(enc.poll(), EncoderEvent::None);
    }

    #[test]
    fn button_press_detected() {
        let mut enc = RotaryEncoder::new(MockPin::low(), MockPin::low(), MockPin::high());
        enc.pin_sw.set(false); // bouton pressé (pull-up LOW)
        assert_eq!(enc.poll(), EncoderEvent::ButtonPressed);
    }

    #[test]
    fn clockwise_rotation_detected() {
        let mut enc = RotaryEncoder::new(MockPin::low(), MockPin::low(), MockPin::high());
        enc.pin_a.set(true); // front montant A, B=LOW → horaire
        assert_eq!(enc.poll(), EncoderEvent::RotateClockwise);
    }

    #[test]
    fn counter_clockwise_rotation_detected() {
        let mut enc = RotaryEncoder::new(MockPin::low(), MockPin::low(), MockPin::high());
        enc.pin_a.set(true);
        enc.pin_b.set(true); // front montant A, B=HIGH → anti-horaire
        assert_eq!(enc.poll(), EncoderEvent::RotateCounterClockwise);
    }
}
