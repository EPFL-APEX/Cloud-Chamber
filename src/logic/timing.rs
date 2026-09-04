//! Timing de la boucle de contrôle : timeouts d'abandon, délais de
//! séquence, taille de l'historique.
//!
//! Distinct de `crate::config::operating` (cibles physiques à calibrer sur
//! la chambre réelle) : ces valeurs-ci réglent le comportement de la
//! machine à états elle-même (`logic::phase_clock`, `logic::control_loop`),
//! pas ce que la chambre doit atteindre physiquement — d'où sa place ici,
//! dans `logic/`, au plus près de ce qui le consomme.

/// 90 échantillons : à ~1 échantillon/s (cadence DS18B20, conversion
/// ~800ms), couvre une fenêtre de stabilité de 60s
/// (`crate::config::operating::STABLE_WINDOW_MS`) avec de la marge. Une
/// valeur de 10 ici (comme précédemment) empêche `temp_stable` de jamais
/// atteindre la couverture de 80% requise sur une fenêtre de 60s — bug
/// trouvé en écrivant `MeasurementHistory::temp_stable` (cf.
/// `logic::probing`).
pub const CONTROL_LOOP_HISTORY_SIZE: usize = 90;

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

/// Délai après coupure HV avant de couper la circulation d'isopropanol.
pub const STOP_HV_SETTLE_MS: u64 = 2_000;
/// #TODO CALIBRAGE : délai après arrêt de la pompe IPA avant de couper le
/// compresseur. Aucune référence physique disponible — valeur de départ
/// choisie du côté long (le compresseur qui tourne un peu plus longtemps
/// pendant que la pompe s'arrête est le côté sûr : la plaque reste froide),
/// à mesurer sur le montage réel avant tout bring-up.
pub const STOP_ISOPROP_SETTLE_MS: u64 = 5_000;
/// Délai après coupure compresseur avant d'attendre l'équilibrage pression.
pub const STOP_COMPRESSOR_SETTLE_MS: u64 = 500;
/// Pas de capteur dédié au circuit réfrigérant : temporisation fixe
/// d'équilibrage avant de considérer l'arrêt terminé (anti court-cycle).
pub const STOP_EQUALIZE_FALLBACK_MS: u64 = 60_000;
