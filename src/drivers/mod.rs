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

/// Driver écran ILI9341 (SPI) avec framebuffer RAM bandé pour un rendu
/// rapide — cf. doc de module pour le détail.
pub mod display;

/// Fonctions de régulation (hystérésis, PID)
pub mod regulate_method;

pub mod zero_cross_pwm;

/// Driver compresseur : relais GPIO régulé par hystérésis autour d'une
/// température cible.
pub mod compressor;

/// Driver chauffage résistif : relais GPIO régulé par hystérésis — jumeau
/// de `compressor`, sens de régulation inversé.
pub mod heater;

/// Relais GPIO tout-ou-rien, spécialisé par un marqueur de rôle — corps
/// commun à `pump`, `lights` et `window_heater`.
pub mod relay;

/// Driver pompe : relais GPIO tout-ou-rien (marche/arrêt).
pub mod pump;

/// Driver éclairage : relais GPIO tout-ou-rien (marche/arrêt).
pub mod lights;

/// Driver chauffage de la vitre supérieure : relais GPIO tout-ou-rien
/// (marche/arrêt).
pub mod window_heater;

/// Stockage persistant des réglages (`config::settings::Settings`) dans la
/// flash interne, sans système de fichiers.
pub mod flash_store;

/// Implémentation de `flash_store::FlashOps` sur la flash QSPI du RP2040,
/// avec mise en attente du second cœur. RP2040 uniquement : la séquence
/// dépend des routines ROM de cette puce.
#[cfg(all(rp2040, target_arch = "arm"))]
pub mod flash_rp2040;

/// Capteurs mock (température/pression/tension) pour les tests — pas de
/// matériel, valeurs configurables. Compilé uniquement sous `cargo test`.
#[cfg(test)]
pub mod mock;
