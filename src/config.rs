/// Configuration centrale du système.
/// Modifier ces constantes pour adapter le firmware à votre installation.

// ============================================================
// Broches GPIO (numéros GP du Pico W)
// ============================================================
/// Bus 1-Wire pour les DS18B20 (avec pull-up 4.7kΩ externe)
pub const PIN_ONEWIRE: u8 = 15;
/// I²C SDA pour les capteurs
pub const PIN_I2C_SDA: u8 = 4;
/// I²C SCL pour les capteurs
pub const PIN_I2C_SCL: u8 = 5;
/// GPIO de sortie pour le relais de sécurité compresseur
pub const PIN_COMPRESSOR_RELAY: u8 = 16;
/// GPIO de sortie pour l'activation du haut voltage
pub const PIN_HV_ENABLE: u8 = 17;
/// GPIO de sortie pour le chauffage isopropanol (PWM à venir, digital pour l'instant)
pub const PIN_ISO_HEATER: u8 = 18;

// ============================================================
// Adresses I²C des capteurs de pression ABP2
// ============================================================
pub const ABP2_BP_ADDR: u8 = 0x28; // Basse pression (0–1 bar abs)
pub const ABP2_HP_ADDR: u8 = 0x38; // Haute pression (0–12 bar gauge)

// ============================================================
// Plages de pression ABP2
// ============================================================
pub const BP_PRESSURE_MIN: f32 = 0.0;
pub const BP_PRESSURE_MAX: f32 = 1.0;
pub const HP_PRESSURE_MIN: f32 = 0.0;
pub const HP_PRESSURE_MAX: f32 = 12.0;

// ============================================================
// Seuils de sécurité
// ============================================================
pub const SAFETY_HP_MAX: f32             = 14.0;
pub const SAFETY_TEMP_COMPRESSOR_MAX: f32 = 120.0;
pub const SAFETY_BP_MIN: f32             = 0.15;
pub const TARGET_CHAMBER_TEMP: f32       = -40.0;

// ============================================================
// Timing (ms)
// ============================================================
pub const CRITICAL_READ_INTERVAL_MS:     u64 = 500;
pub const NON_CRITICAL_READ_INTERVAL_MS: u64 = 2000;
pub const DATA_PUBLISH_INTERVAL_MS:      u64 = 1000;
/// Délai avant de retenter la lecture d'un capteur après un échec.
pub const SENSOR_FAILURE_RETRY_MS:       u64 = 1_000;

// ============================================================
// Seuils de variation BME280 (même logique que FAST_CHANGE_THRESHOLD_C des DS18B20)
// ============================================================
/// Seuil de variation thermique ambiante (°C).
pub const BME280_FAST_TEMP_C:       f32 = 1.0;
/// Seuil de variation de pression atmosphérique (hPa) — plus bas car variations faibles.
pub const BME280_FAST_PRESSURE_HPA: f32 = 0.3;
/// Seuil de variation d'humidité (%).
pub const BME280_FAST_HUMIDITY_PCT: f32 = 1.0;

// ============================================================
// Labels des capteurs de température
// ============================================================
pub const TEMP_LABELS: [&str; 5] = [
    "sortie_compresseur",
    "sortie_condenseur",
    "entree_evaporateur",
    "sortie_evaporateur",
    "base_chambre",
];

/// Index des capteurs critiques
pub const CRITICAL_TEMP_INDICES: [usize; 1] = [0];

/// Index des capteurs non-critiques
pub const NON_CRITICAL_TEMP_INDICES: [usize; 4] = [1, 2, 3, 4];
