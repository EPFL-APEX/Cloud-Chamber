#![no_std]
// Pendant le développement, beaucoup de symboles de la lib ne sont pas encore
// utilisés dans les binaires de test. On supprime ces warnings temporairement.
#![allow(dead_code)]

/// Configuration centrale (broches, adresses, seuils, timings).
pub mod config;

/// Structures de données partagées entre les cœurs.
pub mod data;

/// Drivers capteurs + traits (DS18B20, BME280, ABP2).
/// Même organisation que le projet partenaire (rust-init-refactor).
pub mod sensors;
