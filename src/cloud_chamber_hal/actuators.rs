//! Traits pour les actionneurs, et regroupement générique de trois
//! actionneurs (haute tension, compresseur, chauffage isopropanol).
//!
//! # `BinaryActuator` / `AnalogActuator<Unit>`
//!
//! Même principe que `Sensor<T>` côté lecture : un seul trait générique par
//! forme d'E/S plutôt qu'un trait par rôle physique. `BinaryActuator`
//! remplace l'ancien `BreakerActuator` (trip/reset) — le matériel réel de
//! ce projet est un relais GPIO simple, pas un disjoncteur à verrouillage
//! matériel propre ; le verrouillage (rester coupé jusqu'à réarmement
//! opérateur) est déjà géré côté logiciel par `logic::security::SafetyMonitor`,
//! dupliquer cette sémantique dans le HAL aurait fait doublon.
//! `AnalogActuator<Unit>` généralise l'ancien `VoltageController` : réutilisable
//! pour toute sortie continue (tension, ou un chauffage qui passerait un jour
//! en PWM/duty cycle), pas seulement une tension.

use core::fmt::Debug;

use crate::cloud_chamber_hal::measurement::Measurement;
use crate::cloud_chamber_hal::ring_buffer::RingBuffer;
use crate::cloud_chamber_hal::units::Celsius;

/// Actionneur tout-ou-rien (relais, GPIO simple).
pub trait BinaryActuator {
    type Error: Debug;

    /// Active la sortie.
    fn turn_on(&mut self) -> Result<(), Self::Error>;

    /// Désactive la sortie.
    fn turn_off(&mut self) -> Result<(), Self::Error>;
}


/// Actionneur qui régule lui-même son état par rapport à une cible et un
/// historique de mesures — la politique (hystérésis, PID...) est un détail
/// d'implémentation du driver. `target: None` = coupure forcée, indépendamment de toute mesure.
pub trait TargetActuator<Unit: Copy, const N: usize> {
    type Error: Debug;

    fn regulate(&mut self, hist: &RingBuffer<Measurement<Unit>, N>, target: Option<Unit>) -> Result<(), Self::Error>;
}

/// Actionneur à sortie continue dans l'unité physique `Unit` (ex. tension
/// d'une alimentation variable, duty cycle d'un chauffage PWM).
///
/// Le contrôleur conserve en mémoire la consigne courante et l'applique au
/// matériel. Les valeurs admissibles (plage, résolution) dépendent de
/// l'implémentation concrète.
pub trait AnalogActuator<Unit> {
    type Error: Debug;

    /// Applique `value` en sortie.
    ///
    /// # Errors
    ///
    /// Retourne une erreur si la valeur est hors plage ou si l'écriture
    /// matérielle échoue.
    fn set_output(&mut self, value: Unit) -> Result<(), Self::Error>;

    /// Retourne la consigne actuellement appliquée.
    ///
    /// Il s'agit de la dernière valeur transmise au matériel, pas
    /// nécessairement la valeur mesurée en sortie.
    fn get_setpoint(&self) -> Result<Unit, Self::Error>;
}

/// Ce qu'on demande aux six actionneurs pour un cycle.
///
/// `cooling`/`iso_heater` sont des objectifs (`Option<Celsius>`) : `logic/`
/// décide *quoi* atteindre (une température), pas *comment* — la régulation
/// (hystérésis, PID...) est un détail d'implémentation du driver, appliquée
/// par [`TargetActuator::regulate`]. `None` = coupure forcée, indépendante
/// de toute mesure.
///
/// Les autres actionneurs sont de simples booléens : il n'y a pas de notion
/// de « maintenir » une haute tension, une pompe ou un chauffage vitre,
/// juste de l'appliquer ou non. `lights` est un `Option<bool>` à part :
/// `None` veut dire « cette phase n'a pas d'avis », et l'éclairage garde
/// alors l'état où l'opérateur l'a laissé.
///
/// # Construction
///
/// Se construit depuis [`ActuatorPlan::all_off`], en n'activant que ce que
/// la phase veut :
///
/// ```
/// # use cloud_chamber_firmware::cloud_chamber_hal::actuators::ActuatorPlan;
/// # use cloud_chamber_firmware::cloud_chamber_hal::units::Celsius;
/// let plan = ActuatorPlan::all_off()
///     .with_cooling(Celsius::new(-35.0))
///     .with_iso_pump();
/// assert!(plan.iso_pump);
/// assert!(!plan.high_voltage);
/// ```
///
/// Les quatorze sites de construction énuméraient auparavant les six
/// champs, y compris ceux qui ne changeaient pas d'une phase à l'autre —
/// ce qui noyait la seule chose qui compte à la lecture : ce que *cette*
/// phase allume.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActuatorPlan {
    pub cooling: Option<Celsius>,
    pub iso_heater: Option<Celsius>,
    pub high_voltage: bool,
    pub iso_pump: bool,
    pub lights: Option<bool>,
    pub glass_heater: bool,
}

impl ActuatorPlan {
    /// Tout coupé : l'état de repli, et la base de toute construction.
    ///
    /// C'est aussi le plan des états qui ne pilotent rien (`Idle`,
    /// `Tripped`) — pour ceux-là, « tout coupé » n'est pas un point de
    /// départ mais la réponse complète.
    pub const fn all_off() -> Self {
        Self {
            cooling: None,
            iso_heater: None,
            high_voltage: false,
            iso_pump: false,
            lights: None,
            glass_heater: false,
        }
    }

    /// Demande au froid de tenir `target` dans la chambre.
    #[must_use]
    pub const fn with_cooling(mut self, target: Celsius) -> Self {
        self.cooling = Some(target);
        self
    }

    /// Demande au chauffage isopropanol de tenir `target`.
    #[must_use]
    pub const fn with_iso_heater(mut self, target: Celsius) -> Self {
        self.iso_heater = Some(target);
        self
    }

    /// Met la haute tension sous tension.
    #[must_use]
    pub const fn with_high_voltage(mut self) -> Self {
        self.high_voltage = true;
        self
    }

    /// Fait circuler l'isopropanol.
    #[must_use]
    pub const fn with_iso_pump(mut self) -> Self {
        self.iso_pump = true;
        self
    }

    /// Impose l'état de l'éclairage. Sans cet appel, la phase n'a pas
    /// d'avis et l'éclairage reste tel quel.
    #[must_use]
    pub const fn with_lights(mut self, on: bool) -> Self {
        self.lights = Some(on);
        self
    }

    /// Allume le chauffage anti-buée de la vitre.
    #[must_use]
    pub const fn with_glass_heater(mut self) -> Self {
        self.glass_heater = true;
        self
    }
}

/// Regroupe les six actionneurs de la chambre. Ne décide rien — exécute
/// seulement ce qu'on lui demande.
pub struct Actuators<Hv, Cool, Iso, Pump, Lights, Glass> {
    pub high_voltage: Hv,
    pub cooling: Cool,
    pub iso_heater: Iso,
    /// Pompe de circulation de l'isopropanol.
    pub iso_pump: Pump,
    /// Éclairage de la chambre (deux ampoules sur le même circuit, pilotées
    /// comme un seul actionneur).
    pub lights: Lights,
    /// Chauffage anti-buée de la vitre supérieure.
    pub glass_heater: Glass,
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_off_leaves_every_actuator_alone() {
        let plan = ActuatorPlan::all_off();
        assert_eq!(plan.cooling, None);
        assert_eq!(plan.iso_heater, None);
        assert!(!plan.high_voltage);
        assert!(!plan.iso_pump);
        assert_eq!(plan.lights, None);
        assert!(!plan.glass_heater);
    }

    /// Chaque `with_*` n'allume que son propre actionneur : c'est ce qui
    /// permet de lire un site de construction comme la liste exhaustive de
    /// ce que la phase demande.
    #[test]
    fn each_builder_touches_only_its_own_field() {
        let base = ActuatorPlan::all_off();

        assert_eq!(base.with_cooling(Celsius::new(-40.0)), ActuatorPlan {
            cooling: Some(Celsius::new(-40.0)),
            ..ActuatorPlan::all_off()
        });
        assert_eq!(base.with_iso_heater(Celsius::new(40.0)), ActuatorPlan {
            iso_heater: Some(Celsius::new(40.0)),
            ..ActuatorPlan::all_off()
        });
        assert_eq!(base.with_high_voltage(), ActuatorPlan {
            high_voltage: true,
            ..ActuatorPlan::all_off()
        });
        assert_eq!(base.with_iso_pump(), ActuatorPlan {
            iso_pump: true,
            ..ActuatorPlan::all_off()
        });
        assert_eq!(base.with_lights(true), ActuatorPlan {
            lights: Some(true),
            ..ActuatorPlan::all_off()
        });
        assert_eq!(base.with_glass_heater(), ActuatorPlan {
            glass_heater: true,
            ..ActuatorPlan::all_off()
        });
    }

    /// `with_lights(false)` n'est pas `all_off()` : « éteins » et « je n'ai
    /// pas d'avis » sont deux demandes différentes, et `Actuators::apply`
    /// les traite différemment.
    #[test]
    fn lights_off_differs_from_no_opinion() {
        assert_eq!(ActuatorPlan::all_off().lights, None);
        assert_eq!(ActuatorPlan::all_off().with_lights(false).lights, Some(false));
        assert_ne!(ActuatorPlan::all_off().with_lights(false), ActuatorPlan::all_off());
    }

    /// Utilisable en contexte `const` — rien dans la construction d'un plan
    /// n'a besoin de tourner à l'exécution.
    #[test]
    fn plans_can_be_built_at_compile_time() {
        const PLAN: ActuatorPlan = ActuatorPlan::all_off()
            .with_cooling(Celsius::new(-35.0))
            .with_iso_pump()
            .with_glass_heater();
        assert_eq!(PLAN.cooling, Some(Celsius::new(-35.0)));
        assert!(PLAN.iso_pump);
        assert!(PLAN.glass_heater);
        assert!(!PLAN.high_voltage);
    }
}
