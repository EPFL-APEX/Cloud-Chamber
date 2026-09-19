//! Relais tout-ou-rien piloté par un GPIO, actif à l'état haut.
//!
//! # Une implémentation, plusieurs rôles
//!
//! `Pump`, `Lights` et `WindowHeater` étaient trois fichiers dont le code de
//! production était identique octet pour octet, au nom du type près : même
//! champ, même `new` qui force la broche à l'état bas, même `is_on`, même
//! implémentation de [`BinaryActuator`]. Trois copies à corriger en trois
//! endroits.
//!
//! Le paramètre `Role` est un marqueur de taille nulle : il ne coûte rien à
//! l'exécution, mais garde `Pump<P>` et `Lights<P>` **distincts pour le
//! compilateur**. Un simple alias les aurait rendus interchangeables, et
//! intervertir la pompe et l'éclairage dans le `Actuators { … }` de
//! `main.rs` — un littéral de struct où les deux champs se suivent — serait
//! passé inaperçu. Sur une machine qui pilote de la haute tension, ce
//! contrôle vaut les trois lignes de marqueur.
//!
//! # Actif à l'état haut
//!
//! `turn_on` met la broche à 1. Les modules de relais actifs à l'état bas
//! existent : si la chambre en reçoit un jour, c'est ici que l'inversion se
//! fait, une fois pour les trois rôles. `drivers::breaker::GpioBreaker`, lui,
//! porte déjà son propre `active_high` parce qu'il doit pouvoir être câblé
//! dans les deux sens sans recompiler la logique de sécurité.

use core::marker::PhantomData;

use embedded_hal::digital::OutputPin;

use crate::cloud_chamber_hal::actuators::BinaryActuator;

/// Relais GPIO actif à l'état haut, spécialisé par un marqueur `Role`.
///
/// Voir les alias de rôle : [`crate::drivers::pump::Pump`],
/// [`crate::drivers::lights::Lights`],
/// [`crate::drivers::window_heater::WindowHeater`].
pub struct Relay<P, Role>
where
    P: OutputPin,
{
    activation_pin: P,
    is_on: bool,
    _role: PhantomData<Role>,
}

impl<P, Role> Relay<P, Role>
where
    P: OutputPin,
{
    /// Force la broche à l'état bas — le relais démarre toujours ouvert,
    /// quel que soit l'état dans lequel un reset l'a laissée.
    ///
    /// L'erreur d'écriture est ignorée ici : `new` ne peut pas rendre de
    /// `Result` sans contaminer toute la construction des actionneurs dans
    /// `main.rs`, et une broche qui refuse déjà d'être écrite au démarrage
    /// se signalera au premier `turn_on`.
    pub fn new(mut activation_pin: P) -> Self {
        let _ = activation_pin.set_low();
        Self { activation_pin, is_on: false, _role: PhantomData }
    }

    /// Dernier état commandé. C'est une mémoire logicielle, pas une
    /// relecture du matériel : si `turn_on` a échoué, `is_on` reste faux.
    pub fn is_on(&self) -> bool {
        self.is_on
    }
}

impl<P, Role> BinaryActuator for Relay<P, Role>
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
    use crate::drivers::{lights::Lights, pump::Pump, window_heater::WindowHeater};

    /// Démarre à l'état haut, pour que `new` ait quelque chose à corriger.
    struct MockPin {
        state: bool,
    }

    impl MockPin {
        fn new() -> Self {
            Self { state: true }
        }
    }

    impl embedded_hal::digital::ErrorType for MockPin {
        type Error = core::convert::Infallible;
    }

    impl OutputPin for MockPin {
        fn set_high(&mut self) -> Result<(), Self::Error> {
            self.state = true;
            Ok(())
        }
        fn set_low(&mut self) -> Result<(), Self::Error> {
            self.state = false;
            Ok(())
        }
    }

    #[test]
    fn new_forces_the_pin_low() {
        let relay: Relay<_, ()> = Relay::new(MockPin::new());
        assert!(!relay.activation_pin.state);
        assert!(!relay.is_on());
    }

    #[test]
    fn turn_on_drives_the_pin_high_and_updates_state() {
        let mut relay: Relay<_, ()> = Relay::new(MockPin::new());
        relay.turn_on().unwrap();
        assert!(relay.activation_pin.state);
        assert!(relay.is_on());
    }

    #[test]
    fn turn_off_drives_the_pin_low_and_updates_state() {
        let mut relay: Relay<_, ()> = Relay::new(MockPin::new());
        relay.turn_on().unwrap();
        relay.turn_off().unwrap();
        assert!(!relay.activation_pin.state);
        assert!(!relay.is_on());
    }

    /// Le marqueur de rôle ne doit rien coûter : `Relay<P, Role>` fait la
    /// taille de son seul contenu réel, la broche et le booléen.
    #[test]
    fn the_role_marker_is_free_at_runtime() {
        assert_eq!(
            core::mem::size_of::<Pump<MockPin>>(),
            core::mem::size_of::<MockPin>() + core::mem::size_of::<bool>(),
        );
    }

    /// Ce que l'alias nu aurait perdu : les trois rôles restent des types
    /// différents, donc intervertibles seulement au prix d'une erreur de
    /// compilation. Ce test documente l'intention ; c'est le typage qui la
    /// fait respecter.
    #[test]
    fn the_three_roles_are_distinct_types() {
        use core::any::TypeId;
        assert_ne!(TypeId::of::<Pump<MockPin>>(), TypeId::of::<Lights<MockPin>>());
        assert_ne!(TypeId::of::<Lights<MockPin>>(), TypeId::of::<WindowHeater<MockPin>>());
        assert_ne!(TypeId::of::<Pump<MockPin>>(), TypeId::of::<WindowHeater<MockPin>>());
    }
}
