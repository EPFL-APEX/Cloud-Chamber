//! Traits pour les actionneurs de sécurité.
//!
//! Un actionneur est un composant qui **agit** sur le monde physique,
//! par opposition aux capteurs qui lisent. Le disjoncteur est l'unique
//! actionneur de sécurité dans ce projet.

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
