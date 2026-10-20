//! Relais tout-ou-rien régulé par hystérésis autour d'une cible — corps
//! commun des actionneurs thermostatés.
//!
//! [`crate::drivers::compressor::Compressor`] (active quand il fait trop
//! chaud) et [`crate::drivers::heater::Heater`] (active quand il fait trop
//! froid) étaient deux fichiers identiques octet pour octet, au sens de
//! régulation près : même construction, même [`BinaryActuator`], même
//! [`TargetActuator`], à un `RegulationDirection` près. Deux copies à
//! corriger en deux endroits.
//!
//! # Le sens vit dans le rôle, pas dans un booléen
//!
//! Le sens de régulation n'est pas un paramètre d'exécution qu'on
//! pourrait brancher à l'envers : c'est une constante portée par le type
//! de rôle ([`RegulationRole::DIRECTION`]) — la même mécanique de
//! marqueur que [`crate::drivers::relay::Relay`] utilise pour garder ses
//! rôles distincts. Se tromper de sens, c'est chauffer l'isopropanol
//! quand il est déjà trop chaud : l'erreur doit être de compilation,
//! pas de mise en service.

use embedded_hal::digital::OutputPin;

use crate::cloud_chamber_hal::{
    actuators::{BinaryActuator, TargetActuator},
    measurement::Measurement,
    ring_buffer::RingBuffer,
    units::Celsius,
};
use crate::drivers::regulate_method::{hysteresis, RegulationDirection};
use crate::drivers::relay::Relay;

/// Ce que doit déclarer le rôle d'un actionneur régulé : dans quel sens
/// l'erreur compte.
pub trait RegulationRole {
    /// Le sens dans lequel la sortie s'active — `Upward` : ON quand la
    /// lecture dépasse la cible (refroidir ce qui est trop chaud) ;
    /// `Downward` : ON quand elle passe dessous (chauffer ce qui est trop
    /// froid).
    const DIRECTION: RegulationDirection;
}

/// Relais à hystérésis, spécialisé par un marqueur `Role` portant le sens
/// de régulation.
///
/// Voir les alias de rôle : [`crate::drivers::compressor::Compressor`],
/// [`crate::drivers::heater::Heater`].
pub struct RegulatedRelay<P, Role>
where
    P: OutputPin,
    Role: RegulationRole,
{
    relay: Relay<P, Role>,
    hysteresis_band: Celsius,
}

impl<P, Role> RegulatedRelay<P, Role>
where
    P: OutputPin,
    Role: RegulationRole,
{
    /// `hysteresis_band` est la demi-largeur (positive) de la bande morte
    /// autour de la cible : la sortie s'active d'un côté, se coupe de
    /// l'autre, maintient son état entre les deux.
    pub fn new(relay_pin: P, hysteresis_band: Celsius) -> Self {
        // `Relay::new` force la broche bas : la sortie démarre toujours
        // éteinte, quel que soit l'état laissé par un reset.
        Self { relay: Relay::new(relay_pin), hysteresis_band }
    }

    /// État courant — diagnostic uniquement, pas dans `BinaryActuator`
    /// (le trait générique ne porte pas de méthode de lecture d'état).
    pub fn is_on(&self) -> bool {
        self.relay.is_on()
    }
}

impl<P, Role> BinaryActuator for RegulatedRelay<P, Role>
where
    P: OutputPin,
    Role: RegulationRole,
{
    type Error = P::Error;

    fn turn_on(&mut self) -> Result<(), Self::Error> {
        self.relay.turn_on()
    }

    fn turn_off(&mut self) -> Result<(), Self::Error> {
        self.relay.turn_off()
    }
}

impl<P, Role, const N: usize> TargetActuator<Celsius, N> for RegulatedRelay<P, Role>
where
    P: OutputPin,
    Role: RegulationRole,
{
    type Error = P::Error;

    /// `target: None` coupe la sortie sans consulter l'historique
    /// (contrat de [`TargetActuator`]) ; idem si `hist` ne contient encore
    /// aucune mesure (`get(0)` en erreur) — pas de régulation possible
    /// sans lecture, mieux vaut couper que deviner.
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
            self.relay.is_on(),
            Role::DIRECTION,
        );

        if should_turn_on { self.turn_on() } else { self.turn_off() }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_chamber_hal::timer::Instant;

    /// Broche fictive, cf. `drivers::relay::tests` pour le même motif.
    struct MockPin;

    impl embedded_hal::digital::ErrorType for MockPin {
        type Error = core::convert::Infallible;
    }

    impl OutputPin for MockPin {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
        fn set_high(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    struct CoolsWhenHot;
    struct HeatsWhenCold;

    impl RegulationRole for CoolsWhenHot {
        const DIRECTION: RegulationDirection = RegulationDirection::Upward;
    }

    impl RegulationRole for HeatsWhenCold {
        const DIRECTION: RegulationDirection = RegulationDirection::Downward;
    }

    const BAND: Celsius = Celsius(1.0);
    const TARGET: Celsius = Celsius(5.0);

    /// `filled()` d'abord (`Measurement` n'implémente pas `Default`), puis
    /// une vraie valeur : `filled()` seul laisse le buffer marqué vide,
    /// `get(0)` rend une erreur — cf. doc de `RingBuffer`.
    fn history_at(value: f32) -> RingBuffer<Measurement<Celsius>, 4> {
        let mut hist =
            RingBuffer::filled(Measurement::new(Instant::from_micros(0), Celsius(0.0)));
        hist.push(Measurement::new(Instant::from_micros(1), Celsius(value)));
        hist
    }

    #[test]
    fn an_upward_role_turns_on_above_target_plus_band() {
        let mut r: RegulatedRelay<_, CoolsWhenHot> =
            RegulatedRelay::new(MockPin, BAND);
        r.regulate(&history_at(TARGET.0 + BAND.0 + 0.5), Some(TARGET)).unwrap();
        assert!(r.is_on(), "trop chaud : la sortie refroidissante s'allume");
    }

    #[test]
    fn a_downward_role_turns_on_below_target_minus_band() {
        let mut r: RegulatedRelay<_, HeatsWhenCold> =
            RegulatedRelay::new(MockPin, BAND);
        r.regulate(&history_at(TARGET.0 - BAND.0 - 0.5), Some(TARGET)).unwrap();
        assert!(r.is_on(), "trop froid : la sortie chauffante s'allume");
    }

    /// Le sens du rôle décide, pas l'ordre des arguments : les deux rôles
    /// devant la même lecture tiennent des états opposés.
    #[test]
    fn the_same_reading_drives_opposite_states_through_the_role() {
        // En dehors de la bande, à froid (sous la cible).
        let cold = history_at(TARGET.0 - BAND.0 - 5.0);

        let mut up: RegulatedRelay<_, CoolsWhenHot> = RegulatedRelay::new(MockPin, BAND);
        up.regulate(&cold, Some(TARGET)).unwrap();
        assert!(!up.is_on());

        let mut down: RegulatedRelay<_, HeatsWhenCold> = RegulatedRelay::new(MockPin, BAND);
        down.regulate(&cold, Some(TARGET)).unwrap();
        assert!(down.is_on());
    }

    #[test]
    fn hysteresis_holds_the_state_inside_the_band() {
        let mut r: RegulatedRelay<_, CoolsWhenHot> = RegulatedRelay::new(MockPin, BAND);
        r.regulate(&history_at(TARGET.0 + BAND.0 + 0.5), Some(TARGET)).unwrap();
        assert!(r.is_on());

        // Rentrée dans la bande : l'hystérésis maintient l'état précédent
        // plutôt que de re-décider sur un simple seuil.
        r.regulate(&history_at(TARGET.0), Some(TARGET)).unwrap();
        assert!(r.is_on());

        // Franchie de l'autre côté : coupe.
        r.regulate(&history_at(TARGET.0 - BAND.0 - 0.5), Some(TARGET)).unwrap();
        assert!(!r.is_on());
    }

    #[test]
    fn no_target_cuts_the_output() {
        let mut r: RegulatedRelay<_, CoolsWhenHot> = RegulatedRelay::new(MockPin, BAND);
        r.regulate(&history_at(TARGET.0 + BAND.0 + 0.5), Some(TARGET)).unwrap();
        assert!(r.is_on());

        r.regulate(&history_at(TARGET.0 + BAND.0 + 0.5), None).unwrap();
        assert!(!r.is_on());
    }

    #[test]
    fn an_empty_history_cuts_the_output() {
        let mut r: RegulatedRelay<_, CoolsWhenHot> = RegulatedRelay::new(MockPin, BAND);
        let hist: RingBuffer<Measurement<Celsius>, 4> =
            RingBuffer::filled(Measurement::new(Instant::from_micros(0), Celsius(0.0)));
        r.regulate(&hist, Some(TARGET)).unwrap();
        assert!(!r.is_on());
    }

    #[test]
    fn binary_commands_bypass_the_regulation() {
        let mut r: RegulatedRelay<_, CoolsWhenHot> = RegulatedRelay::new(MockPin, BAND);
        r.turn_on().unwrap();
        assert!(r.is_on());
        r.turn_off().unwrap();
        assert!(!r.is_on());
    }
}
