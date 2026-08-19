//! Construction physique de la machine : sur quelle broche/adresse chaque
//! capteur/actionneur est branché sur cette installation précise.
//!
//! Les compteurs (`NUMBER_OF_TEMP_SENSOR`...) et les index de rôle
//! (`CHAMBER_TEMP_IDX`...) restent dans
//! [`crate::cloud_chamber_hal::config`] plutôt qu'ici : le code générique du
//! HAL (`Sensors<Tmp, Prs>`, `RingBuffer<T, N>`...) en a besoin comme
//! paramètres de type, et le HAL ne doit pas dépendre de `crate::config`
//! (sens de dépendance inverse). Ce fichier assume cette forme et y branche
//! le câblage concret.

use crate::cloud_chamber_hal::config::NUMBER_OF_TEMP_SENSOR;

// ─── Broches GPIO (numéros GP du RP2040/RP2350) ───────────────────────────────

/// Bus 1-Wire pour les DS18B20 (avec pull-up 4.7 kΩ externe)
pub const PIN_ONEWIRE: u8 = 23;
/// I²C SDA pour les capteurs
pub const PIN_I2C_SDA: u8 = 21;
/// I²C SCL pour les capteurs
pub const PIN_I2C_SCL: u8 = 20;
/// GPIO de sortie pour le relais de sécurité compresseur
pub const PIN_COMPRESSOR_RELAY: u8 = 5;
/// TODO CÂBLAGE : broches provisoires (GP17/GP18, suite de
/// `PIN_COMPRESSOR_RELAY`), jamais vérifiées sur le montage réel — à
/// confirmer avant tout bring-up matériel.
pub const PIN_HV_RELAY: u8 = 14;
pub const PIN_ISO_HEATER_RELAY: u8 = 9;
/// TODO CÂBLAGE : broches provisoires (suite de `PIN_ISO_HEATER_RELAY`),
/// jamais assignées sur le montage réel. Les drivers `drivers::pump::Pump`,
/// `drivers::lights::Lights` et `drivers::window_heater::WindowHeater`
/// existent déjà mais n'étaient pas encore référencés ici — à confirmer
/// avant tout bring-up matériel, comme les broches ci-dessus.
pub const PIN_PUMP_RELAY: u8 = 7;
pub const PIN_LIGHTS_RELAY: u8 = 8;
pub const PIN_WINDOW_HEATER_RELAY: u8 = 21;

// ─── Adresse I²C du capteur de pression ABP2 (chambre) ─────────────────────────

/// TODO CÂBLAGE : reprend l'ancienne adresse basse-pression (0x28) en
/// attendant vérification — l'unique capteur restant est câblé dans la
/// chambre, pas sur le circuit réfrigérant, l'adresse réelle n'est pas
/// confirmée sur le montage.
pub const ABP2_ADDR: u8 = 0x28;

// ─── Plage de pression ABP2 ─────────────────────────────────────────────────────

/// TODO CALIBRAGE : reprend la plage 0–1 bar (variante basse-pression) en
/// attendant confirmation de la plage réelle du capteur chambre.
pub const CHAMBER_PRESSURE_MIN: f32 = 0.0;
pub const CHAMBER_PRESSURE_MAX: f32 = 1.0;

// ─── Labels des capteurs de température ──────────────────────────────────────

pub const TEMP_LABELS: [&str; NUMBER_OF_TEMP_SENSOR] = [
    "sortie_compresseur",
    "sortie_condenseur",
    "entree_evaporateur",
    "sortie_evaporateur",
    "base_chambre",
    "",
    "",
    "",
];
