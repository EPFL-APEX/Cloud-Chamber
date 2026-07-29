//! Ce que la phase courante demande aux actionneurs — décision, retournée
//! par `react_to`, séparée de l'application au matériel
//! (`cloud_chamber_hal::actuators::Actuators::apply`). Voir
//! `logic/cooling.rs` et `logic/stopping.rs` pour la justification : une
//! table séparée risquait de diverger du match de transition (cf.
//! régression déjà vue sur ce repo avec l'interlock HT), et un effet de
//! bord direct dans `react_to` aurait empêché de tester la logique de
//! séquencement avec `cargo test-host` sans simuler des GPIO.
//!
//! Reste dans `logic/` (pas dans `cloud_chamber_hal`) : c'est une décision
//! liée aux phases de la machine à états, pas une abstraction matérielle.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActuatorPlan {
    pub compressor: bool,
    pub iso_heater: bool,
    pub high_voltage: bool,
}
