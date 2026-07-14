/// Configuration centrale du système.
/// Modifier ces constantes pour adapter le firmware à votre installation.

// ─── Capteurs ──────────────────────────────────────

pub const NUMBER_OF_TEMP_SENSOR: usize = 5;
pub const NUMBER_OF_PRESSURE_SENSOR: usize = 1;
pub const NUMBER_OF_VOLTMETER: usize = 3;
pub const NUMBER_OF_AMPMETER: usize = 1;

// ─── Broches GPIO (numéros GP du RP2040/RP2350) ───────────────────────────────

/// Bus 1-Wire pour les DS18B20 (avec pull-up 4.7 kΩ externe)
pub const PIN_ONEWIRE: u8 = 15;
/// I²C SDA pour les capteurs
pub const PIN_I2C_SDA: u8 = 4;
/// I²C SCL pour les capteurs
pub const PIN_I2C_SCL: u8 = 5;
/// GPIO de sortie pour le relais de sécurité compresseur
pub const PIN_COMPRESSOR_RELAY: u8 = 16;

// ─── Adresses I²C des capteurs de pression ABP2 ───────────────────────────────

pub const ABP2_BP_ADDR: u8 = 0x28; // Basse pression (0–1 bar abs)
pub const ABP2_HP_ADDR: u8 = 0x38; // Haute pression (0–12 bar gauge)

// ─── Plages de pression ABP2 ──────────────────────────────────────────────────

pub const BP_PRESSURE_MIN: f32 = 0.0;
pub const BP_PRESSURE_MAX: f32 = 1.0;
pub const HP_PRESSURE_MIN: f32 = 0.0;
pub const HP_PRESSURE_MAX: f32 = 12.0;


// ─── Labels des capteurs de température ──────────────────────────────────────

pub const TEMP_LABELS: [&str; NUMBER_OF_TEMP_SENSOR] = [
    "sortie_compresseur",
    "sortie_condenseur",
    "entree_evaporateur",
    "sortie_evaporateur",
    "base_chambre",
];



// ─── Seuils de sécurité ───────────────────────────────────────────────────────

pub const SAFETY_HP_MAX: f32              = 14.0;
pub const SAFETY_TEMP_COMPRESSOR_MAX: f32 = 120.0;
pub const SAFETY_BP_MIN: f32              = 0.15;
pub const TARGET_CHAMBER_TEMP: f32        = -40.0;


// ─── Contol loop options ──────────────────────────────────────────────────────────────
pub const CONTROL_LOOP_HISTORY_SIZE:usize = 10;