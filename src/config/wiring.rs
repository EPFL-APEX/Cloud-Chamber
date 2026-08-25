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
pub const PIN_I2C_SDA: u8 = 20;
/// I²C SCL pour les capteurs
pub const PIN_I2C_SCL: u8 = 21;
/// GPIO de sortie pour le relais de sécurité compresseur
pub const PIN_COMPRESSOR_RELAY: u8 = 5;
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
pub const PIN_WINDOW_HEATER_RELAY: u8 = 10;

/// Toutes les broches attribuées ci-dessus, avec le rôle qui les réclame.
///
/// Sert uniquement au contrôle d'unicité juste en dessous — deux rôles sur
/// la même broche ne peuvent pas fonctionner, et rien dans le typage ne
/// l'empêche : `gpio::new_pin` prend un numéro, pas un jeton unique.
/// Ce tableau doit rester en phase avec les constantes ; une broche ajoutée
/// et oubliée ici n'est simplement pas vérifiée.
const ASSIGNED_PINS: [(&str, u8); 17] = [
    ("PIN_ONEWIRE", PIN_ONEWIRE),
    ("PIN_I2C_SDA", PIN_I2C_SDA),
    ("PIN_I2C_SCL", PIN_I2C_SCL),
    ("PIN_COMPRESSOR_RELAY", PIN_COMPRESSOR_RELAY),
    ("PIN_HV_RELAY", PIN_HV_RELAY),
    ("PIN_ISO_HEATER_RELAY", PIN_ISO_HEATER_RELAY),
    ("PIN_PUMP_RELAY", PIN_PUMP_RELAY),
    ("PIN_LIGHTS_RELAY", PIN_LIGHTS_RELAY),
    ("PIN_WINDOW_HEATER_RELAY", PIN_WINDOW_HEATER_RELAY),
    ("PIN_ENCODER_A", PIN_ENCODER_A),
    ("PIN_ENCODER_B", PIN_ENCODER_B),
    ("PIN_ENCODER_SW", PIN_ENCODER_SW),
    ("PIN_SCREEN_SCK", PIN_SCREEN_SCK),
    ("PIN_SCREEN_MOSI", PIN_SCREEN_MOSI),
    ("PIN_SCREEN_CS", PIN_SCREEN_CS),
    ("PIN_SCREEN_DC", PIN_SCREEN_DC),
    ("PIN_SCREEN_RESET", PIN_SCREEN_RESET),
];

/// Échoue **à la compilation** si deux rôles se partagent une broche.
///
/// A rattrapé un vrai conflit : `PIN_WINDOW_HEATER_RELAY` valait 21, comme
/// `PIN_I2C_SCL`. Configurer la broche en sortie relais aurait coupé le bus
/// I²C — donc le capteur de pression, donc la surveillance de sécurité HP —
/// et ça ne se serait vu qu'au bring-up, sous forme d'un I²C muet.
const _: () = {
    let mut i = 0;
    while i < ASSIGNED_PINS.len() {
        let mut j = i + 1;
        while j < ASSIGNED_PINS.len() {
            if ASSIGNED_PINS[i].1 == ASSIGNED_PINS[j].1 {
                panic!("deux roles sont cables sur la meme broche GPIO");
            }
            j += 1;
        }
        i += 1;
    }
};

/// Encodeur rotatif de l'UI (quadrature) — cf. `drivers::encoder`.
pub const PIN_ENCODER_A: u8 = 26;
pub const PIN_ENCODER_B: u8 = 27;
/// Bouton-poussoir intégré à l'encodeur.
pub const PIN_ENCODER_SW: u8 = 28;

/// Écran SPI (ILI9341, 320x240). SCK/MOSI passent par le périphérique SPI0
/// matériel (broches valides pour ce rôle d'après la table RP2040) ; CS/DC/RESET
/// sont de simples GPIO pilotés en logiciel.
pub const PIN_SCREEN_SCK: u8 = 18;
pub const PIN_SCREEN_MOSI: u8 = 19;
pub const PIN_SCREEN_CS: u8 = 22;
pub const PIN_SCREEN_DC: u8 = 16;
pub const PIN_SCREEN_RESET: u8 = 17;

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
