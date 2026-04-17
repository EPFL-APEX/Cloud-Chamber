/// Configuration centrale du système.
/// Modifier ces constantes pour adapter le firmware à votre installation.

// ============================================================
// WiFi
// ============================================================
pub const WIFI_SSID: &str = "YOUR_WIFI_SSID";
pub const WIFI_PASSWORD: &str = "YOUR_WIFI_PASSWORD";

// ============================================================
// Broches GPIO (numéros GP du Pico W)
// ============================================================
/// Bus 1-Wire pour les DS18B20 (avec pull-up 4.7kΩ externe)
pub const PIN_ONEWIRE: u8 = 15;
/// I²C SDA pour les capteurs ABP2
pub const PIN_I2C_SDA: u8 = 4;
/// I²C SCL pour les capteurs ABP2
pub const PIN_I2C_SCL: u8 = 5;
/// GPIO de sortie pour le relais de sécurité compresseur
pub const PIN_COMPRESSOR_RELAY: u8 = 16;

// ============================================================
// Adresses I²C des capteurs de pression ABP2
// ============================================================
pub const ABP2_BP_ADDR: u8 = 0x28; // Basse pression (0–1 bar abs)
pub const ABP2_HP_ADDR: u8 = 0x38; // Haute pression (0–12 bar gauge)

// ============================================================
// Plages de pression ABP2 (pour calcul de la valeur)
// ============================================================
pub const BP_PRESSURE_MIN: f32 = 0.0;  // bar absolu
pub const BP_PRESSURE_MAX: f32 = 1.0;  // bar absolu
pub const HP_PRESSURE_MIN: f32 = 0.0;  // bar gauge
pub const HP_PRESSURE_MAX: f32 = 12.0; // bar gauge

// ============================================================
// Seuils de sécurité (réaction immédiate sur Core 0)
// ============================================================
/// Pression HP maximale avant coupure compresseur (bar gauge)
pub const SAFETY_HP_MAX: f32 = 14.0;
/// Température maximale sortie compresseur avant alarme (°C)
pub const SAFETY_TEMP_COMPRESSOR_MAX: f32 = 120.0;
/// Pression BP minimale avant alarme (bar absolu)
/// En dessous = vide trop profond, risque d'entrée d'air
pub const SAFETY_BP_MIN: f32 = 0.15;
/// Température base chambre cible (°C) — pour information
pub const TARGET_CHAMBER_TEMP: f32 = -40.0;

// ============================================================
// Timing (intervalles en millisecondes)
// ============================================================
/// Intervalle de lecture des capteurs critiques (ms)
pub const CRITICAL_READ_INTERVAL_MS: u64 = 500;
/// Intervalle de lecture des capteurs non-critiques (ms)
pub const NON_CRITICAL_READ_INTERVAL_MS: u64 = 2000;
/// Intervalle de publication des données vers Core 1 (ms)
pub const DATA_PUBLISH_INTERVAL_MS: u64 = 1000;
/// Port du serveur HTTP
pub const HTTP_PORT: u16 = 80;

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

/// Index des capteurs critiques dans le tableau de températures
pub const CRITICAL_TEMP_INDICES: [usize; 1] = [
    0, // T1: sortie compresseur (surchauffe)
];

/// Index des capteurs non-critiques
pub const NON_CRITICAL_TEMP_INDICES: [usize; 4] = [
    1, // T2: sortie condenseur
    2, // T3: entrée évaporateur
    3, // T4: sortie évaporateur
    4, // T5: base chambre
];
