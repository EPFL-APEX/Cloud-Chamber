//! Relais tout-ou-rien piloté par un GPIO, actif à l'état haut.
//!
//! # Une implémentation, plusieurs rôles
//!
//! Toutes les sorties binaires de la chambre partagent le même
//! comportement — `new` qui force la broche à l'état bas, on/off — donc
//! un seul type : [`Relay<P, Role>`](Relay). Le paramètre `Role` est un
//! marqueur de taille nulle : il ne coûte rien à l'exécution, mais garde
//! `Pump<P>` et `Lights<P>` **distincts pour le compilateur**. Un simple
//! alias les aurait rendus interchangeables, et intervertir la pompe et
//! l'éclairage dans le `Actuators { … }` de `main.rs` — un littéral de
//! struct où les deux champs se suivent — serait passé inaperçu. Sur une
//! machine qui pilote de la haute tension, ce contrôle vaut les trois
//! lignes de marqueur. Les rôles sont définis en bas de ce fichier.
//!
//! # Actif à l'état haut
//!
//! `turn_on` met la broche à 1. Les modules de relais actifs à l'état bas
//! existent : si la chambre en reçoit un jour, c'est ici que l'inversion se
//! fait, une fois pour tous les rôles. `drivers::breaker::GpioBreaker`, lui,
//! porte déjà son propre `active_high` parce qu'il doit pouvoir être câblé
//! dans les deux sens sans recompiler la logique de sécurité.

use core::marker::PhantomData;

use embedded_hal::digital::OutputPin;

use crate::cloud_chamber_hal::actuators::BinaryActuator;

/// Relais GPIO actif à l'état haut, spécialisé par un marqueur `Role`.
///
/// Les rôles (et leurs alias) sont définis en bas de ce fichier.
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

    /// Applique l'état demandé en un seul geste — pour les propriétaires
    /// hors boucle de contrôle (la bande LED du compartiment, pilotée par
    /// le cœur 0), qui n'ont pas de raison de passer par le trait.
    ///
    /// Le trait reste la voie de la boucle de contrôle : `turn_on`/
    /// `turn_off` sont ce que `ActuatorPlan`applique via `Actuators`.
    pub fn set(&mut self, on: bool) -> Result<(), P::Error> {
        if on {
            self.turn_on()
        } else {
            self.turn_off()
        }
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

// ─── Rôles des sorties binaires ───────────────────────────────────────────────

/// Pompe de circulation de l'isopropanol.
pub struct PumpRole;
pub type Pump<P> = Relay<P, PumpRole>;

/// Éclairage de la chambre — deux ampoules sur le même circuit, pilotées
/// comme un seul actionneur.
pub struct LightsRole;
pub type Lights<P> = Relay<P, LightsRole>;

/// Ventilateurs du radiateur.
pub struct FansRole;
pub type Fans<P> = Relay<P, FansRole>;

/// Chauffage anti-buée de la vitre supérieure.
pub struct WindowHeaterRole;
pub type WindowHeater<P> = Relay<P, WindowHeaterRole>;

/// Bande LED du compartiment machine — un rôle distinct de [`Lights`] :
/// elle n'appartient à aucun cycle, seule la commande opérateur
/// l'allume (écran dédié, cœur 0 seul propriétaire), et sa broche
/// n'est pas dans `safe_panic::POWER_OUTPUT_PINS` — une LED allumée
/// pendant une panique aide au diagnostic.
pub struct BayLightsRole;
pub type LedStrip<P> = Relay<P, BayLightsRole>;

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{Lights, Pump, WindowHeater, *};

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
