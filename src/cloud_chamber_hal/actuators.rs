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

/// Ce qu'on demande aux trois actionneurs pour un cycle.
/// `cooling`/`iso_heater` sont des objectifs (`Option<Celsius>`): `logic/` décide *quoi* atteindre (une température), pas
/// *comment* — la régulation (hystérésis, PID...) est un détail
/// d'implémentation du driver, appliquée par `TargetActuator::regulate`.
/// `None` = coupure forcée, indépendante de toute mesure. `high_voltage`
/// reste un simple `bool` : il n'y a pas de notion de "maintenir" une
/// haute tension, juste de l'appliquer ou non.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActuatorPlan {
    pub cooling: Option<Celsius>,
    pub iso_heater: Option<Celsius>,
    pub high_voltage: bool,
}

/// Regroupe les trois actionneurs de la chambre. Ne décide rien — exécute
/// seulement ce qu'on lui demande.
pub struct Actuators<Hv, Cool, Iso> {
    pub high_voltage: Hv,
    pub cooling: Cool,
    pub iso_heater: Iso,
}
