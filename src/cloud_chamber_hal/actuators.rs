//! Traits pour les actionneurs de sécurité.
//!
//! Un actionneur est un composant qui **agit** sur le monde physique,
//! par opposition aux capteurs qui lisent.

/// Contrôle d'un disjoncteur ou relai de coupure d'urgence.
///
/// # États du disjoncteur
///
/// ```text
/// Normal ──trip()──► Déclenché
/// Déclenché ──reset()──► Normal
/// ```
pub trait BreakerActuator {
    type Error;

    /// Déclenche le disjoncteur (coupe l'alimentation).
    fn trip(&mut self) -> Result<(), Self::Error>;

    /// Réarme le disjoncteur (rétablit l'alimentation).
    fn reset(&mut self) -> Result<(), Self::Error>;

    /// Indique si le disjoncteur est actuellement déclenché.
    fn is_tripped(&self) -> Result<bool, Self::Error>;
}


/// Contrôle d'une alimentation à tension variable (ex. : alimentation Peltier, HV bias).
///
/// Le contrôleur conserve en mémoire la consigne courante et l'applique au
/// matériel. Les valeurs admissibles (plage, résolution) dépendent de
/// l'implémentation concrète.
///
/// # États
///
/// ```text
/// (tension quelconque) ──set_voltage(v)──► consigne = v, sortie stabilisée
///                      ◄──et_setpoint()── retourne la consigne appliquée
/// ```
///
/// # Erreurs
///
/// Chaque méthode retourne `Err(Self::Error)` si la communication avec le
/// matériel échoue ou si la valeur demandée est hors plage.
pub trait VoltageController {
    type Error;

    /// Applique la tension `voltage` (en volts) en sortie.
    ///
    /// # Errors
    ///
    /// Retourne une erreur si la valeur est hors plage ou si l'écriture
    /// matérielle échoue.
    fn set_voltage(&mut self, voltage: f32) -> Result<(), Self::Error>;

    /// Retourne la consigne de tension actuellement appliquée, en volts.
    ///
    /// Il s'agit de la dernière valeur transmise au matériel, pas
    /// nécessairement la tension mesurée en sortie.
    fn get_setpoint(&self) -> Result<f32, Self::Error>;
}