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

/// Actionneur tout-ou-rien (relais, GPIO simple).
pub trait BinaryActuator {
    type Error: Debug;

    /// Active la sortie.
    fn turn_on(&mut self) -> Result<(), Self::Error>;

    /// Désactive la sortie.
    fn turn_off(&mut self) -> Result<(), Self::Error>;
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

/// Regroupe les trois actionneurs de la chambre. Ne décide rien — exécute
/// seulement ce qu'on lui demande via `apply()`.
///
/// Prend trois booléens plutôt qu'un `logic::ActuatorPlan` : le HAL ne
/// dépend jamais de la logique métier (principe d'inversion de dépendance
/// du projet — cf. `security_loop` historiquement, `logic/` aujourd'hui,
/// qui dépendent tous deux de `cloud_chamber_hal`, jamais l'inverse).
/// L'appelant décompose son `ActuatorPlan` au point d'appel.
pub struct Actuators<Hv, Comp, Iso> {
    pub high_voltage: Hv,
    pub compressor: Comp,
    pub iso_heater: Iso,
}

impl<Hv, Comp, Iso> Actuators<Hv, Comp, Iso>
where
    Hv: BinaryActuator,
    Comp: BinaryActuator,
    Iso: BinaryActuator,
{
    pub fn apply(&mut self, high_voltage: bool, compressor: bool, iso_heater: bool) {
        let _ = Self::set(&mut self.high_voltage, high_voltage);
        let _ = Self::set(&mut self.compressor, compressor);
        let _ = Self::set(&mut self.iso_heater, iso_heater);
    }

    fn set<A: BinaryActuator>(actuator: &mut A, on: bool) -> Result<(), A::Error> {
        if on { actuator.turn_on() } else { actuator.turn_off() }
    }
}
