#![no_std]

/// Configuration centrale (broches, adresses, seuils, timings).
pub mod config;

/// Structures de données partagées entre les cœurs.
pub mod data;

/// Drivers capteurs + traits (DS18B20, BME280, ABP2).
/// Même organisation que le projet partenaire (rust-init-refactor).
pub mod sensors;
