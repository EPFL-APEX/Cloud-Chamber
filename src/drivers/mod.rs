//! Drivers matériels concrets pour la chambre à nuages.
//!
//! # Relation avec `cloud_chamber_hal`
//!
//! Le module [`crate::cloud_chamber_hal`] définit les **traits** (interfaces).
//! Ce module (`drivers`) fournit les **implémentations concrètes** de ces traits
//! pour le matériel réel.
//!
//! Cette séparation suit le principe de l'**inversion de dépendance** :
//! la logique métier (`security_loop`) dépend des traits abstraits,
//! pas des drivers concrets. On peut donc remplacer un driver sans toucher
//! à la logique de sécurité.

/// Drivers ADC : capteurs de tension et de courant via l'ADC embarqué.
pub mod adc;

/// Driver disjoncteur : contrôle d'un relai ou contacteur via GPIO.
pub mod breaker;

/// Driver d'affichage : wrapper autour du contrôleur ILI9341.
pub mod display;

/// Driver encodeur rotatif : lecture des impulsions et du bouton.
pub mod encoder;

/// Driver capteur de fermeture : détection d'un contact sec via GPIO.
pub mod closure;

/// Driver DS18B20 : capteur de température 1-Wire (authentique ou clone SKIP ROM).
pub mod ds18b20;

/// Driver BME280 : capteur de température, humidité et pression atmosphérique via I²C.
pub mod bme280;

/// Driver ABP2 : capteur de pression Honeywell via I²C.
pub mod abp2;
