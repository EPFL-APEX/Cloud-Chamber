//! Drivers matériels concrets pour la chambre à nuages.
//!
//! # Relation avec `cloud_chamber_hal`
//!
//! [`crate::cloud_chamber_hal`] définit les **traits** (interfaces),
//! ce module fournit les **implémentations concrètes** pour le matériel
//! réel. La logique (`logic/`) dépend des traits abstraits, pas des
//! drivers concrets : on peut remplacer un driver sans toucher à la
//! logique de contrôle.

/// Gestionnaire de panique qui coupe les sorties de puissance avant tout.
pub mod safe_panic;
/// Driver disjoncteur : contrôle d'un relai ou contacteur via GPIO.
pub mod breaker;

/// Driver encodeur rotatif : lecture des impulsions et du bouton.
pub mod encoder;

/// Driver DS18B20 : capteur de température 1-Wire (authentique ou clone SKIP ROM).
pub mod ds18b20;

/// Driver BME280 : capteur de température, humidité et pression atmosphérique via I²C.
pub mod bme280;

/// Driver écran ILI9341 (SPI) avec framebuffer RAM bandé pour un rendu
/// rapide — cf. doc de module pour le détail.
pub mod display;

/// Fonctions de régulation (hystérésis, PID)
pub mod regulate_method;

/// Driver compresseur : relais GPIO régulé par hystérésis autour d'une
/// température cible — refroidit quand il fait trop chaud.
pub mod compressor;

/// Driver chauffage résistif : relais GPIO régulé par hystérésis — jumeau
/// de `compressor`, chauffe quand il fait trop froid.
pub mod heater;

/// Relais GPIO tout-ou-rien, spécialisé par un marqueur de rôle — le
/// corps commun de toutes les sorties binaires (pompe, éclairage,
/// ventilateurs, vitre, bande LED), rôles définis dans le module.
pub mod relay;

/// Relais GPIO à hystérésis, le sens de régulation porté par le type de
/// rôle — corps commun à `compressor` et `heater`.
pub mod regulated_relay;

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
