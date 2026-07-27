//! Indices des capteurs dans les tableaux `SensorSnapshot`/`MeasurementHistory`.
//!
//! Ces constantes vivent ici plutôt que dans `config.rs` : elles décrivent la
//! forme des tableaux que `cloud_chamber_hal::sensors` définit (quel index
//! correspond à quel rôle physique), pas des réglages d'installation.
//!
//! ATTENTION : les slots `ds0..ds4` suivent l'ordre de découverte SEARCH ROM
//! du bus 1-Wire, pas un ordre physique fixe — à vérifier au boot (lignes
//! INFO ds{i}) avant de faire confiance à ces valeurs sur un nouveau montage.

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
