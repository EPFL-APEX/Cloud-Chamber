//! Couche d'abstraction matérielle (HAL) spécifique à la chambre à nuages.
//!
//! # Pourquoi une HAL personnalisée ?
//!
//! La crate `embedded-hal` définit des traits génériques pour le matériel
//! standard (SPI, I2C, GPIO…). Ce module définit des traits de plus haut
//! niveau adaptés aux besoins spécifiques de ce projet : types de capteurs,
//! actionneurs, et timer monotonique.
//!
//! # Polymorphisme par traits en Rust
//!
//! En Rust, le polymorphisme se fait principalement via les **traits** plutôt
//! que l'héritage (il n'y a pas d'héritage de classes). Un trait définit un
//! ensemble de méthodes que n'importe quel type peut implémenter.
//!
//! Les fonctions et structures génériques (comme `SecurityLoop<T>`) acceptent
//! n'importe quel type `T` qui implémente le trait requis. Cela permet de
//! changer le matériel (ex: type de capteur) sans modifier la logique métier.

/// Mesure horodatée — forme commune à toutes les lectures de capteur.
pub mod measurement;

/// Traits pour les capteurs de mesure (température, tension, courant, fermeture).
pub mod sensors;

/// Traits pour les actionneurs (tout-ou-rien, sortie continue) et leur
/// regroupement générique.
pub mod actuators;

/// Traits pour le timer monotonique et le watchdog.
pub mod timer;

pub mod units;

/// Indices des capteurs dans les tableaux `SensorSnapshot`/`MeasurementHistory`.
pub mod config;