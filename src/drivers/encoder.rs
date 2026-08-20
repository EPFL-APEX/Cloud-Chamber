//! Driver encodeur rotatif quadrature avec bouton-poussoir.
//!
//! # Encodeur quadrature
//!
//! Un encodeur rotatif génère deux signaux en quadrature (A et B) déphasés
//! de 90°. En lisant l'état de A quand B change (ou vice-versa), on détermine
//! le sens de rotation. Ce driver utilise un polling simple (pas d'interruption),
//! un seul front (montant sur A) par cycle de quadrature — pas de machine à
//! états sur les 4 transitions.
//!
//! Constaté sur matériel réel : à 10 ms de cadence de poll, un cycle de
//! quadrature complet (10-20 ms à vitesse de rotation normale) peut être
//! manqué ou lu à moitié fait (A et B pas encore synchrones), donnant le
//! mauvais sens par intermittence. **Poller à ~1 ms** plutôt que 10 ms
//! élimine ce sous-échantillonnage — voir aussi [`SW_DEBOUNCE_POLLS`],
//! dont la durée réelle dépend directement de cette cadence.

use embedded_hal::digital::InputPin;

/// Nombre de scrutations consécutives à l'état bas requis avant de
/// considérer un appui bouton comme réel plutôt qu'un parasite électrique
/// transitoire — constaté sur matériel réel : sans debounce, un appui
/// fantôme pouvait se déclencher pendant une simple rotation (couplage
/// avec les contacts A/B qui commutent, ou léger jeu mécanique du bouton
/// entraîné par l'axe).
///
/// Exprimé en nombre de scrutations plutôt qu'en durée : la durée réelle
/// dépend de la cadence à laquelle [`RotaryEncoder::poll`] est appelée. En
/// pratique à ~1 ms de cadence (cf. doc de module), 4 correspond à ~4 ms —
/// largement en dessous de la durée d'un appui volontaire (dizaines de
/// ms), largement au-dessus d'un parasite ponctuel. Si la cadence de poll
/// change significativement, revoir cette constante en conséquence.
const SW_DEBOUNCE_POLLS: u8 = 4;

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
    /// État "débruité" courant du bouton : `true` = relâché.
    sw_released: bool,
    /// Scrutations consécutives à l'état bas depuis le dernier relâchement
    /// stable — cf. [`SW_DEBOUNCE_POLLS`].
    sw_low_streak: u8,
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
            sw_released: true, // pull-up : bouton relâché = HIGH
            sw_low_streak: 0,
        }
    }

    /// Lit l'état des broches et retourne l'événement détecté.
    ///
    /// À appeler à chaque itération de la boucle UI, à ~1 ms — cf. doc de
    /// module pour l'impact d'une cadence différente sur la précision de
    /// décodage et sur la durée réelle du debounce bouton.
    pub fn poll(&mut self) -> EncoderEvent {
        let a = self.pin_a.is_high().unwrap_or(false);
        let b = self.pin_b.is_high().unwrap_or(false);
        let sw_low_now = self.pin_sw.is_low().unwrap_or(false);

        // Debounce par comptage : un appui n'est retenu qu'après
        // SW_DEBOUNCE_POLLS scrutations consécutives à l'état bas, pour
        // ignorer un parasite électrique ou mécanique bref pendant une
        // rotation — cf. doc de [`SW_DEBOUNCE_POLLS`].
        let mut button_event = false;
        if sw_low_now {
            if self.sw_low_streak < SW_DEBOUNCE_POLLS {
                self.sw_low_streak += 1;
            }
            if self.sw_low_streak == SW_DEBOUNCE_POLLS && self.sw_released {
                button_event = true;
                self.sw_released = false;
            }
        } else {
            self.sw_low_streak = 0;
            self.sw_released = true;
        }

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
    fn button_press_detected_after_debounce() {
        let mut enc = RotaryEncoder::new(MockPin::low(), MockPin::low(), MockPin::high());
        enc.pin_sw.set(false); // bouton pressé (pull-up LOW)
        // Les SW_DEBOUNCE_POLLS - 1 premières scrutations à l'état bas ne
        // déclenchent encore rien — c'est seulement la Nième qui confirme.
        for _ in 0..SW_DEBOUNCE_POLLS - 1 {
            assert_eq!(enc.poll(), EncoderEvent::None);
        }
        assert_eq!(enc.poll(), EncoderEvent::ButtonPressed);
        // Rester appuyé ne redéclenche pas l'événement à chaque scrutation.
        assert_eq!(enc.poll(), EncoderEvent::None);
    }

    #[test]
    fn brief_glitch_on_sw_does_not_trigger_button_press() {
        // Parasite électrique ou mécanique bref (ex. pendant une rotation) :
        // l'état bas ne dure pas assez de scrutations consécutives pour
        // passer le seuil de debounce — aucun appui ne doit être détecté.
        let mut enc = RotaryEncoder::new(MockPin::low(), MockPin::low(), MockPin::high());
        enc.pin_sw.set(false);
        for _ in 0..SW_DEBOUNCE_POLLS - 1 {
            assert_eq!(enc.poll(), EncoderEvent::None);
        }
        enc.pin_sw.set(true); // relâché avant d'atteindre le seuil
        assert_eq!(enc.poll(), EncoderEvent::None);

        // Un nouvel appui, ensuite, doit toujours pouvoir se déclencher
        // normalement — le glitch précédent ne doit pas laisser l'état
        // bloqué.
        enc.pin_sw.set(false);
        for _ in 0..SW_DEBOUNCE_POLLS - 1 {
            assert_eq!(enc.poll(), EncoderEvent::None);
        }
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
