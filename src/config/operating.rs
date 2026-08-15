//! Réglages physiques ajustables : cibles de température, tolérances,
//! seuils de sécurité. Plusieurs sont déjà exposés comme valeurs par défaut
//! modifiables dans `ui::screens::settings` — ce fichier reste la source de
//! vérité pour ces valeurs par défaut, pas pour l'état courant modifié en
//! session (qui vit dans l'écran lui-même).

// ─── Seuils de sécurité ───────────────────────────────────────────────────────

pub const SAFETY_TEMP_COMPRESSOR_MAX: f32 = 120.0;
pub const TARGET_CHAMBER_TEMP: f32 = -40.0;

// ─── Séquence de refroidissement (logic::cooling) ──────────────────────────────
// Valeurs initiales, à calibrer sur la chambre réelle.

/// PreCoolingThePlate → StartingIpaCirculation quand ds4 ≤ ce seuil.
pub const PRECOOL_TARGET_C: f32 = -20.0;
/// SaturatingAirWithIpa → HighVoltage quand ds4 ≤ ce seuil.
pub const SATURATION_TARGET_C: f32 = -35.0;
/// Fenêtre de stabilité pour valider la phase HighVoltage — durée
/// d'observation à calibrer avec `STABLE_TOLERANCE_C`, pas un timeout de
/// boucle de contrôle : reste ici plutôt que dans `logic::timing`.
pub const STABLE_WINDOW_MS: u64 = 60_000;
/// Tolérance de variation de ds4 sur la fenêtre de stabilité.
pub const STABLE_TOLERANCE_C: f32 = 1.0;

/// Demi-largeur de la bande d'hystérésis des actionneurs régulés (froid,
/// chauffage IPA). Distincte de `STABLE_TOLERANCE_C` : même valeur de
/// départ, mais concept différent (l'une valide qu'une phase peut avancer,
/// l'autre évite l'oscillation rapide d'un relais) — pas de raison qu'elles
/// restent égales si l'une est recalibrée plus tard. TODO : pas encore
/// consommée — aucun bring-up ne construit encore les actionneurs régulés
/// avec cette valeur.
pub const REGULATION_BAND_C: f32 = 1.0;

/// TODO CALIBRAGE : cible du thermostat chauffage isopropanol (ds3, cf.
/// `cloud_chamber_hal::config::ISO_TEMP_IDX`) — aucune valeur physique de
/// référence disponible pour l'instant. Valeur volontairement haute plutôt
/// qu'un chiffre qui aurait l'air raisonnable sans l'être — à vérifier sur
/// le montage réel avant tout bring-up.
pub const IPA_HEATER_TARGET_C: f32 = 40.0;
