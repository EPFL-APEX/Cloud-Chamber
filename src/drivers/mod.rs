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

/// Fonctions de régulation (hystérésis, PID)
pub mod regulate_method;

pub mod zero_cross_pwm;

/// Driver compresseur : relais GPIO régulé par hystérésis autour d'une
/// température cible.
pub mod compressor;

/// Driver pompe : sortie GPIO tout-ou-rien (marche/arrêt).
pub mod pump;

/// Driver éclairage : sortie GPIO tout-ou-rien (marche/arrêt).
pub mod lights;

/// Capteurs mock (température/pression/tension) pour les tests — pas de
/// matériel, valeurs configurables. Compilé uniquement sous `cargo test`.
#[cfg(test)]
pub mod mock;
