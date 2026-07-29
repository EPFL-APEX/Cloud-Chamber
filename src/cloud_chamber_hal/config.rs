//! Forme des tableaux `SensorSnapshot`/`MeasurementHistory`/`Sensors` :
//! combien de capteurs de chaque catégorie, et quel index correspond à quel
//! rôle physique.
//!
//! Ces constantes vivent ici plutôt que dans `crate::config` : elles
//! décrivent la forme de l'abstraction générique que `cloud_chamber_hal`
//! définit, pas le câblage concret (broches GPIO, adresses I²C — ça reste
//! dans `crate::config`, question différente de bring-up matériel) ni les
//! réglages de contrôle (seuils de sécurité, timing de phase — `crate::config`
//! aussi pour l'instant, chantier `logic/` séparé).
//!
//! ATTENTION : les slots `ds0..ds4` suivent l'ordre de découverte SEARCH ROM
//! du bus 1-Wire, pas un ordre physique fixe — à vérifier au boot (lignes
//! INFO ds{i}) avant de faire confiance à ces valeurs sur un nouveau montage.

/// Nombre de sondes de température (DS18B20 sur le bus 1-Wire).
pub const NUMBER_OF_TEMP_SENSOR: usize = 5;
/// Nombre de capteurs de pression (2 ABP2 distincts : basse + haute pression).
pub const NUMBER_OF_PRESSURE_SENSOR: usize = 2;
/// Nombre de voltmètres.
pub const NUMBER_OF_VOLTMETER: usize = 3;
/// Nombre d'ampèremètres.
pub const NUMBER_OF_AMPMETER: usize = 1;

/// Index de la sonde base-chambre (ds4) — cible du refroidissement.
pub const CHAMBER_TEMP_IDX: usize = 4;
/// Index de la sonde sortie-compresseur (ds0) — surveillance surchauffe.
pub const COMPRESSOR_OUT_IDX: usize = 0;
/// Index de la sonde utilisée par le thermostat chauffage isopropanol.
pub const ISO_TEMP_IDX: usize = 3;

/// Index du capteur basse pression (ABP2, 0–1 bar) dans `press`.
pub const BP_PRESSURE_IDX: usize = 0;
/// Index du capteur haute pression (ABP2, 0–12 bar) dans `press`.
pub const HP_PRESSURE_IDX: usize = 1;
