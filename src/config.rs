/// Configuration centrale du système.
/// Modifier ces constantes pour adapter le firmware à votre installation.
// Nombre de capteurs par catégorie (NUMBER_OF_TEMP_SENSOR, etc.) : déplacé
// vers `cloud_chamber_hal::config` — décrit la forme de l'abstraction
// générique `Sensors`/`SensorSnapshot`, pas un réglage d'installation.
use crate::cloud_chamber_hal::config::NUMBER_OF_TEMP_SENSOR;

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

/// TODO CALIBRAGE : 14.0 bar est au-dessus de la plage du capteur ABP2 HP
/// (0-12 bar, cf. `HP_PRESSURE_MAX`) — ce seuil ne peut physiquement jamais
/// être atteint tel quel, l'alarme HP ne se déclenchera donc jamais en
/// pratique. Bug trouvé pendant l'audit de logic/security.rs, présent sur
/// les deux lignées d'origine. Valeur volontairement pas corrigée ici :
/// il faut la vraie limite mécanique du circuit frigorifique, pas une
/// valeur devinée sur un seuil de sécurité.
pub const SAFETY_HP_MAX: f32 = 14.0;
pub const SAFETY_TEMP_COMPRESSOR_MAX: f32 = 120.0;
pub const SAFETY_BP_MIN: f32 = 0.15;
pub const TARGET_CHAMBER_TEMP: f32 = -40.0;

// ─── Contol loop options ──────────────────────────────────────────────────────────────
// 90 échantillons : à ~1 échantillon/s (cadence DS18B20, conversion ~800ms),
// couvre une fenêtre de stabilité de 60s (STABLE_WINDOW_MS) avec de la marge.
// Une valeur de 10 ici (comme précédemment) empêche `temp_stable` de jamais
// atteindre la couverture de 80% requise sur une fenêtre de 60s — bug trouvé
// en écrivant `MeasurementHistory::temp_stable` (cf. logic/probing.rs).
pub const CONTROL_LOOP_HISTORY_SIZE: usize = 90;

// ─── Séquence de refroidissement (logic::cooling) ──────────────────────────────
// Valeurs initiales, à calibrer sur la chambre réelle.

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

// Timeouts d'abandon (phase trop longue → retour Idle), gérés par l'appelant.
pub const SENSOR_CHECK_TIMEOUT_MS: u64 = 30_000;
pub const PRECOOL_TIMEOUT_MS: u64 = 45 * 60_000;
pub const SATURATION_TIMEOUT_MS: u64 = 30 * 60_000;
pub const HV_STABILISE_TIMEOUT_MS: u64 = 15 * 60_000;
pub const FINAL_CHECK_TIMEOUT_MS: u64 = 30_000;

/// Perte de capteur pendant un cycle : au-delà de ce délai sans lecture
/// valide de la base chambre, la phase est abandonnée (plutôt que d'attendre
/// le timeout long de la phase, aveugle).
pub const SENSOR_LOSS_MS: u64 = 10_000;

// ─── Séquence d'arrêt (logic::stopping) ────────────────────────────────────────

/// Délai après coupure HV avant de couper le compresseur.
pub const STOP_HV_SETTLE_MS: u64 = 2_000;
/// Délai après coupure compresseur avant d'attendre l'équilibrage pression.
pub const STOP_COMPRESSOR_SETTLE_MS: u64 = 500;
/// HP considérée équilibrée sous ce seuil (bar) — si capteur présent.
pub const STOP_EQUALIZE_HP_MAX: f32 = 2.0;
/// Sans capteur HP : temporisation d'équilibrage (anti court-cycle).
pub const STOP_EQUALIZE_FALLBACK_MS: u64 = 60_000;
