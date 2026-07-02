use crate::config::{SAFETY_HP_MAX, SAFETY_TEMP_COMPRESSOR_MAX, SAFETY_BP_MIN};
use crate::data::SystemState;
use super::output::ControlOutput;
use super::pid::PidController;
use super::target::TargetState;

// ─────────────────────────────────────────────────────────────────────────────
// Index des capteurs dans TEMP_LABELS (cf. config.rs)
// ─────────────────────────────────────────────────────────────────────────────

const COMPRESSOR_OUT_IDX: usize = 0; // "sortie_compresseur"  — sécurité
const ISO_TEMP_IDX:       usize = 3; // "sortie_evaporateur"  — TODO: confirmer avec l'équipe
const CHAMBER_TEMP_IDX:   usize = 4; // "base_chambre"        — cible de refroidissement

// ─────────────────────────────────────────────────────────────────────────────
// Constantes de contrôle — TODO: calibrer expérimentalement
// ─────────────────────────────────────────────────────────────────────────────

/// Hystérésis du compresseur (°C de part et d'autre de chamber_temp_c).
/// Ex : cible −40 °C → ON si T > −38, OFF si T < −42.
const COMPRESSOR_HYST_BAND_C: f32 = 2.0; // TODO: affiner

/// Le HV n'est activé que si la chambre est à moins de N °C au-dessus de la cible.
const HV_READY_WINDOW_C: f32 = 5.0; // TODO: affiner

// Gains PID chauffage isopropanol — TODO: à calibrer expérimentalement
const ISO_KP: f32 = 1.0;
const ISO_KI: f32 = 0.1;
const ISO_KD: f32 = 0.05;

// ─────────────────────────────────────────────────────────────────────────────

/// Contrôleur principal de la chambre à brouillard.
///
/// `step()` reçoit l'état courant et la consigne, et retourne les commandes
/// actuateurs pour le cycle en cours. Tout le hardware reste hors de cette struct.
pub struct Controller {
    /// Mémoire d'état du compresseur pour l'hystérésis (évite les courts-cycles).
    compressor_on: bool,
    /// PID du chauffage isopropanol.
    iso_pid: PidController,
}

impl Controller {
    pub fn new() -> Self {
        Self {
            compressor_on: false,
            iso_pid: PidController::new(ISO_KP, ISO_KI, ISO_KD, 0.0, 1.0),
        }
    }

    /// Calcule les commandes pour ce cycle.
    ///
    /// `dt_s` : durée écoulée depuis le dernier appel, en secondes (pour le PID).
    pub fn step(
        &mut self,
        state:  &SystemState,
        target: &TargetState,
        dt_s:   f32,
    ) -> ControlOutput {
        // ── 1. Sécurité — priorité absolue ───────────────────────────────────
        if self.safety_triggered(state) {
            self.compressor_on = false;
            self.iso_pid.reset();
            return ControlOutput::emergency_stop();
        }

        // ── 2. Compresseur (hystérésis sur la base de la chambre) ────────────
        let chamber = &state.temperatures[CHAMBER_TEMP_IDX];
        if chamber.valid {
            let t   = chamber.value;
            let tgt = target.chamber_temp_c;
            // La chambre est trop chaude → démarrer le compresseur.
            if t > tgt + COMPRESSOR_HYST_BAND_C {
                self.compressor_on = true;
            }
            // La chambre a trop dépassé la cible → arrêter.
            else if t < tgt - COMPRESSOR_HYST_BAND_C {
                self.compressor_on = false;
            }
            // Dans la bande d'hystérésis → on conserve l'état courant.
        }
        // Si lecture invalide : on conserve l'état précédent (fail-safe neutre).

        // ── 3. Chauffage isopropanol (PID) ───────────────────────────────────
        let iso = &state.temperatures[ISO_TEMP_IDX];
        let iso_duty = if iso.valid {
            self.iso_pid.update(target.isopropanol_temp_c, iso.value, dt_s)
        } else {
            // Pas de mesure → sécurité : on coupe et on purge l'état PID.
            self.iso_pid.reset();
            0.0
        };

        // ── 4. Haut voltage ──────────────────────────────────────────────────
        let high_voltage = target.high_voltage_enabled && self.chamber_ready(state, target);

        ControlOutput {
            // L'interlock externe (autre cœur, IHM) peut bloquer le compresseur
            // sans déclencher le safety_override. L'état interne (hysteresis) est
            // préservé : quand l'interlock se lève, le compresseur reprend normalement.
            compressor: self.compressor_on && state.compressor_allowed,
            isopropanol_heater_duty: iso_duty,
            high_voltage,
            safety_override: false,
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Helpers privés
    // ─────────────────────────────────────────────────────────────────────────

    /// Conditions qui déclenchent un arrêt d'urgence immédiat.
    fn safety_triggered(&self, state: &SystemState) -> bool {
        // Pression HP trop haute → risque mécanique sur le compresseur.
        if state.pressure_hp.valid && state.pressure_hp.pressure > SAFETY_HP_MAX {
            return true;
        }
        // Température sortie compresseur trop haute → surchauffe.
        let t_comp = &state.temperatures[COMPRESSOR_OUT_IDX];
        if t_comp.valid && t_comp.value > SAFETY_TEMP_COMPRESSOR_MAX {
            return true;
        }
        // Pression BP trop faible → perte de réfrigérant / cavitation.
        if state.pressure_bp.valid && state.pressure_bp.pressure < SAFETY_BP_MIN {
            return true;
        }
        false
    }

    /// La chambre est prête pour le HV si elle est suffisamment froide.
    fn chamber_ready(&self, state: &SystemState, target: &TargetState) -> bool {
        let chamber = &state.temperatures[CHAMBER_TEMP_IDX];
        if !chamber.valid { return false; }
        // La chambre doit être au plus HV_READY_WINDOW_C au-dessus de la cible.
        chamber.value <= target.chamber_temp_c + HV_READY_WINDOW_C
    }
}
