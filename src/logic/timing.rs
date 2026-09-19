//! Timing de la boucle de contrôle : timeouts d'abandon, délais de
//! séquence, taille de l'historique.
//!
//! Toutes les valeurs temporelles sont des
//! [`Duration`](crate::cloud_chamber_hal::timer::Duration) — plus de `u64`
//! de millisecondes nues, ni de suffixe `_MS` : l'unité est portée par le
//! type, plus par le nom. Seul `CONTROL_LOOP_HISTORY_SIZE` reste un
//! scalaire, c'est un nombre d'échantillons.
//!
//! Distinct de `crate::config::operating` (cibles physiques à calibrer sur
//! la chambre réelle) : ces valeurs-ci réglent le comportement de la
//! machine à états elle-même (`logic::phase_clock`, `logic::control_loop`),
//! pas ce que la chambre doit atteindre physiquement — d'où sa place ici,
//! dans `logic/`, au plus près de ce qui le consomme.

use crate::cloud_chamber_hal::timer::Duration;

/// 90 échantillons : à ~1 échantillon/s (cadence DS18B20, conversion
/// ~800 ms), couvre la fenêtre de stabilité de 60 s
/// (`crate::config::operating::STABLE_WINDOW`) avec de la marge.
///
/// `MeasurementHistory::is_temp_stable` ne suppose plus de cadence, mais
/// cette taille reste un plafond dur : le balayage s'arrête au bout du
/// buffer, donc un historique trop court ne peut jamais prouver qu'il
/// enjambe la fenêtre. Une valeur de 10 (comme dans une version
/// précédente) rendait la stabilité inatteignable sur 60 s.
pub const CONTROL_LOOP_HISTORY_SIZE: usize = 90;

/// Durée de circulation IPA (pas de capteur dédié — temporisation).
pub const IPA_CIRCULATION: Duration = Duration::from_millis(120_000);

// Timeouts d'abandon (phase trop longue → retour Idle), gérés par l'appelant.
pub const SENSOR_CHECK_TIMEOUT: Duration = Duration::from_millis(30_000);
pub const PRECOOL_TIMEOUT: Duration = Duration::from_millis(45 * 60_000);
pub const SATURATION_TIMEOUT: Duration = Duration::from_millis(30 * 60_000);
pub const HV_STABILISE_TIMEOUT: Duration = Duration::from_millis(15 * 60_000);
pub const FINAL_CHECK_TIMEOUT: Duration = Duration::from_millis(30_000);

/// Perte de capteur pendant un cycle : au-delà de ce délai sans lecture
/// valide de la base chambre, la phase est abandonnée (plutôt que d'attendre
/// le timeout long de la phase, aveugle).
pub const SENSOR_LOSS: Duration = Duration::from_millis(10_000);

// ─── Séquence d'arrêt (logic::stopping) ────────────────────────────────────────

/// Délai après coupure HV avant de couper la circulation d'isopropanol.
pub const STOP_HV_SETTLE: Duration = Duration::from_millis(2_000);
/// #TODO CALIBRAGE : délai après arrêt de la pompe IPA avant de couper le
/// compresseur. Aucune référence physique disponible — valeur de départ
/// choisie du côté long (le compresseur qui tourne un peu plus longtemps
/// pendant que la pompe s'arrête est le côté sûr : la plaque reste froide),
/// à mesurer sur le montage réel avant tout bring-up.
pub const STOP_ISOPROP_SETTLE: Duration = Duration::from_millis(5_000);
/// Délai après coupure compresseur avant d'attendre l'équilibrage pression.
pub const STOP_COMPRESSOR_SETTLE: Duration = Duration::from_millis(500);
/// Pas de capteur dédié au circuit réfrigérant : temporisation fixe
/// d'équilibrage avant de considérer l'arrêt terminé (anti court-cycle).
pub const STOP_EQUALIZE_FALLBACK: Duration = Duration::from_millis(60_000);
