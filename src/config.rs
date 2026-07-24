/// Configuration centrale du système.
/// Fusion des deux branches : constantes de dimensionnement de la branche
/// équipe (NUMBER_OF_*) + broches/seuils/phases de la branche Capteurs.

// ============================================================
// Dimensionnement capteurs (branche équipe — utilisés par shared/data.rs
// et les futurs traits cloud_chamber_hal)
// ============================================================
pub const NUMBER_OF_TEMP_SENSOR: usize     = 5;
pub const NUMBER_OF_PRESSURE_SENSOR: usize = 1;
pub const NUMBER_OF_VOLTMETER: usize       = 3;
pub const NUMBER_OF_AMPMETER: usize        = 1;
/// Taille des ring buffers de la boucle de contrôle (branche équipe).
pub const CONTROL_LOOP_HISTORY_SIZE: usize = 10;

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
pub const SAFETY_HP_MAX: f32              = 14.0;
pub const SAFETY_TEMP_COMPRESSOR_MAX: f32 = 120.0;
pub const SAFETY_BP_MIN: f32              = 0.15;
pub const TARGET_CHAMBER_TEMP: f32        = -40.0;

// ============================================================
// Timing (ms)
// ============================================================
pub const CRITICAL_READ_INTERVAL_MS:     u64 = 500;
pub const NON_CRITICAL_READ_INTERVAL_MS: u64 = 2000;
pub const DATA_PUBLISH_INTERVAL_MS:      u64 = 1000;
/// Délai avant de retenter la lecture d'un capteur après un échec.
pub const SENSOR_FAILURE_RETRY_MS:       u64 = 1_000;

// ============================================================
// Seuils de variation BME280
// ============================================================
/// Seuil de variation thermique ambiante (°C).
pub const BME280_FAST_TEMP_C:       f32 = 1.0;
/// Seuil de variation de pression atmosphérique (hPa).
pub const BME280_FAST_PRESSURE_HPA: f32 = 0.3;
/// Seuil de variation d'humidité (%).
pub const BME280_FAST_HUMIDITY_PCT: f32 = 1.0;

// ============================================================
// Labels des capteurs de température
// ============================================================
pub const TEMP_LABELS: [&str; NUMBER_OF_TEMP_SENSOR] = [
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

// ============================================================
// Rôles des capteurs (index dans TEMP_LABELS)
// ATTENTION : les slots ds0..ds4 suivent l'ordre de découverte SEARCH ROM.
// Vérifier la correspondance physique via les lignes INFO ds{i} au boot.
// ============================================================
pub const COMPRESSOR_OUT_IDX: usize = 0; // "sortie_compresseur" — sécurité
pub const ISO_TEMP_IDX:       usize = 3; // "sortie_evaporateur" — thermostat isopropanol
pub const CHAMBER_TEMP_IDX:   usize = 4; // "base_chambre"       — cible refroidissement

// ============================================================
// Machine à états — seuils et timeouts des phases
// (valeurs initiales, à calibrer sur la chambre réelle)
// ============================================================
/// PreCoolingThePlate → StartingIpaCirculation quand ds4 ≤ ce seuil.
pub const PRECOOL_TARGET_C: f32 = -20.0;
/// SaturatingAirWithIpa → HighVoltage quand ds4 ≤ ce seuil.
pub const SATURATION_TARGET_C: f32 = -35.0;
/// Fenêtre de stabilité pour valider la phase HighVoltage.
pub const STABLE_WINDOW_MS: u64 = 60_000;
/// Tolérance de variation de ds4 sur la fenêtre de stabilité.
pub const STABLE_TOLERANCE_C: f32 = 1.0;
/// Durée de circulation IPA (pas de capteur dédié — temporisation).
pub const IPA_CIRCULATION_MS: u64 = 120_000;
/// Durée minimale de la vérification finale.
pub const FINAL_CHECK_MS: u64 = 5_000;

// Timeouts d'abandon (phase trop longue → retour Idle)
pub const SENSOR_CHECK_TIMEOUT_MS: u64 = 30_000;
pub const PRECOOL_TIMEOUT_MS:      u64 = 45 * 60_000;
pub const SATURATION_TIMEOUT_MS:   u64 = 30 * 60_000;
pub const HV_STABILISE_TIMEOUT_MS: u64 = 15 * 60_000;
pub const FINAL_CHECK_TIMEOUT_MS:  u64 = 30_000;

// Arrêt (Stopping)
/// Délai après coupure HV avant de couper le compresseur.
pub const STOP_HV_SETTLE_MS: u64 = 2_000;
/// HP considérée équilibrée sous ce seuil (bar) — si capteur présent.
pub const STOP_EQUALIZE_HP_MAX: f32 = 2.0;
/// Sans capteur HP : temporisation d'équilibrage (anti court-cycle).
pub const STOP_EQUALIZE_FALLBACK_MS: u64 = 60_000;
