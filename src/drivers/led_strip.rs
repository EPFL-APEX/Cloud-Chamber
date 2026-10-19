//! Bande LED du compartiment machine : relais GPIO tout-ou-rien.
//!
//! Le corps est celui de [`crate::drivers::relay::Relay`] — même
//! construction qui force la broche bas, même [`BinaryActuator`] — via
//! un rôle distinct, cf. la doc de `relay` pour le pourquoi du marqueur.
//!
//! # Pourquoi un rôle distinct de `drivers::lights`
//!
//! `drivers::lights::Lights` pilote l'éclairage **de la chambre** (les
//! deux ampoules au-dessus du volume de détection) : il appartient au
//! cycle automatique, via `ActuatorPlan` et `Actuators::apply` sur le
//! cœur 1.
//!
//! La bande LED d'ici est celle du **compartiment machine** — ajoutée
//! pour voir la mécanique fonctionner. Elle n'appartient à aucun cycle :
//! seule la commande opérateur l'allume, depuis l'écran dédié du menu.
//! Deux vies distinctes, deux rôles distincts — le marqueur de type
//! garantit qu'un futur plan d'actionneur ne peut pas « éclairer » le
//! compartiment en croyant piloter la chambre, ni l'inverse dans le
//! littéral `Actuators { … }` de `main.rs`.
//!
//! # Propriétaire
//!
//! Le cœur 0 (UI) en est seul propriétaire, comme de la console — la
//! boucle de contrôle (cœur 1) n'y touche pas et n'a pas à le connaître :
//! le registre GPIO est atomique par écriture, et la LED n'entre dans
//! aucune logique de sécurité.
//!
//! C'est aussi pourquoi GP`PIN_LED_STRIP` n'est pas dans
//! `drivers::safe_panic::POWER_OUTPUT_PINS` : le gestionnaire de panique
//! force les sorties **de puissance** au niveau bas ; une bande LED
//! alimentée par son MOSFET n'est pas une sortie dangereuse, et la
//! garder allumée pendant une panique aide au diagnostic plutôt qu'il ne
//! gêne.
//!
//! [`BinaryActuator`]: crate::cloud_chamber_hal::actuators::BinaryActuator

use crate::drivers::relay::Relay;

/// Marqueur de rôle. Sa seule fonction est de rendre la bande LED du
/// compartiment distincte de l'éclairage de chambre (`LightsRole`) aux
/// yeux du compilateur.
pub struct BayLightsRole;

/// Bande LED du compartiment machine, sur un GPIO actif à l'état haut.
///
/// Le point d'accroche matériel (MOSFET ou relais) est supposé actif
/// haut, comme les relais du cycle — cf. `drivers::relay` pour
/// l'inversion si un jour le câblage l'exige.
pub type LedStrip<P> = Relay<P, BayLightsRole>;

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::lights::Lights;

    /// Broche fictive, cf. `drivers::relay::tests` pour le même motif.
    struct MockPin {
        state: bool,
    }

    impl embedded_hal::digital::ErrorType for MockPin {
        type Error = core::convert::Infallible;
    }

    impl embedded_hal::digital::OutputPin for MockPin {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            self.state = false;
            Ok(())
        }
        fn set_high(&mut self) -> Result<(), Self::Error> {
            self.state = true;
            Ok(())
        }
    }

    /// La bande démarre éteinte, quel que soit l'état de la broche.
    /// (Le niveau physique de la broche est couvert par les tests de
    /// `drivers::relay` — ici on teste le rôle, pas le corps.)
    #[test]
    fn the_strip_starts_off() {
        let strip: LedStrip<MockPin> = LedStrip::new(MockPin { state: true });
        assert!(!strip.is_on());
    }

    /// `set` commande la bande dans les deux sens — la voie de la boucle
    /// du cœur 0, qui n'a pas de plan d'actionneur.
    #[test]
    fn set_drives_the_strip_both_ways() {
        let mut strip: LedStrip<MockPin> = LedStrip::new(MockPin { state: false });
        strip.set(true).unwrap();
        assert!(strip.is_on());
        strip.set(false).unwrap();
        assert!(!strip.is_on());
    }

    /// La bande du compartiment et l'éclairage de chambre restent des
    /// types distincts : les intervertir dans `Actuators { … }` ne passe
    /// pas la compilation, c'est le rôle du marqueur.
    #[test]
    fn the_bay_strip_is_not_the_chamber_lights() {
        use core::any::TypeId;
        assert_ne!(
            TypeId::of::<LedStrip<MockPin>>(),
            TypeId::of::<Lights<MockPin>>()
        );
    }
}
