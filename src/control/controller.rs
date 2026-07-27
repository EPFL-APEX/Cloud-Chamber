use crate::config::{
    CHAMBER_TEMP_IDX, ISO_TEMP_IDX, COMPRESSOR_OUT_IDX,
    SAFETY_HP_MAX, SAFETY_TEMP_COMPRESSOR_MAX, SAFETY_BP_MIN,
};
use crate::data::SystemState;
use crate::logic::{cooling::CoolingPhase, history::MeasurementHistory,
                   stopping::StoppingPhase, PhaseContext, SystemTask};
use crate::security::{monitor::SecurityMonitor, safety::{SafetyCause, SafetyConfig}};
use super::output::ControlOutput;
use super::target::TargetState;

// ─────────────────────────────────────────────────────────────────────────────
// Constantes de contrôle — TODO: calibrer expérimentalement
// ─────────────────────────────────────────────────────────────────────────────

/// Le HV n'est activé que si la chambre est à moins de N °C au-dessus de la cible.
const HV_READY_WINDOW_C: f32 = 5.0; // TODO: affiner

/// Hystérésis du thermostat chauffage isopropanol. La sortie GP18 est
/// tout-ou-rien : un thermostat à hystérésis remplace l'ancien PID dont la
/// sortie proportionnelle était de toute façon écrasée en binaire (duty > 0).
/// Reprendre control/pid.rs si la sortie passe un jour en PWM.
const ISO_HYSTERESIS_C: f32 = 0.5;

// ─────────────────────────────────────────────────────────────────────────────

/// Contrôleur principal de la chambre à brouillard.
///
/// Deux points d'entrée :
/// - `step()`  : comportement historique (mode manuel + sécurité non
///   verrouillante). Conservé tel quel pour test_control.rs.
/// - `tick()`  : machine à états (SystemTask) + SecurityMonitor verrouillant.
///   C'est ce que main.rs utilise. En Idle, tick() == step().
pub struct Controller {
    /// Mémoire d'état du compresseur (mode manuel).
    compressor_on: bool,
    /// État du thermostat chauffage isopropanol (hystérésis).
    iso_heating: bool,
    /// Anti-rebond de l'ancien chemin de sécurité (step()).
    safety_cycles: u8,
    /// Machine à états — Idle = mode manuel.
    task: SystemTask,
    /// Instant d'entrée dans la phase courante (ms).
    phase_entered_ms: u64,
    /// Disjoncteur logiciel (chemin tick()).
    monitor: SecurityMonitor,
    /// Message d'alerte pour le TFT (bannière clignotante). Persiste jusqu'à
    /// acquittement (CYCLE 0 / réarmement) ou nouveau départ de cycle.
    alert: Option<&'static str>,
}

impl Controller {
    pub fn new() -> Self {
        Self {
            compressor_on: false,
            iso_heating: false,
            safety_cycles: 0,
            task: SystemTask::Idle,
            phase_entered_ms: 0,
            monitor: SecurityMonitor::new(SafetyConfig::default()),
            alert: None,
        }
    }

    // ─── Machine à états ─────────────────────────────────────────────────────

    pub fn task(&self) -> SystemTask { self.task }
    pub fn phase_code(&self) -> u8 { self.task.code() }
    pub fn phase_label(&self) -> Option<&'static str> { self.task.label() }
    pub fn is_tripped(&self) -> bool { self.monitor.is_tripped() }
    pub fn alert(&self) -> Option<&'static str> { self.alert }

    /// CYCLE 1 — lance la séquence de démarrage. Refusé hors Idle ou si le
    /// disjoncteur est verrouillé.
    pub fn request_start(&mut self, now_ms: u64) -> bool {
        if self.task == SystemTask::Idle && !self.monitor.is_tripped() {
            self.alert = None; // nouveau cycle = acquittement
            self.enter(SystemTask::Cooling(CoolingPhase::SensorCheck), now_ms);
            true
        } else {
            false
        }
    }

    /// CYCLE 0 — arrêt propre depuis n'importe quel état actif.
    /// Réarme aussi le disjoncteur (reconnaissance opérateur).
    pub fn request_stop(&mut self, now_ms: u64) -> bool {
        self.monitor.reset();
        self.alert = None; // acquittement opérateur
        match self.task {
            SystemTask::Idle | SystemTask::Stopping(_) => false,
            _ => {
                self.enter(SystemTask::Stopping(StoppingPhase::CutHighVoltage), now_ms);
                true
            }
        }
    }

    fn enter(&mut self, task: SystemTask, now_ms: u64) {
        self.task = task;
        self.phase_entered_ms = now_ms;
    }

    /// Point d'entrée principal (main.rs) : sécurité verrouillante, transitions
    /// de phases, puis sorties selon la tâche courante.
    ///
    /// `state` est mutable : tout retour en Idle depuis un cycle (abandon,
    /// fin d'arrêt, disjoncteur) révoque l'autorisation compresseur — le mode
    /// manuel reprend toujours moteur coupé, jamais par surprise.
    pub fn tick(
        &mut self,
        state:   &mut SystemState,
        history: &MeasurementHistory,
        target:  &TargetState,
        now_ms: u64,
        _dt_s:  f32, // conservé pour compat d'appel (plus de PID à intégrer)
    ) -> ControlOutput {
        // ── 1. Sécurité — disjoncteur prioritaire ────────────────────────────
        if !self.monitor.check(state) {
            if self.task != SystemTask::Idle {
                self.enter(SystemTask::Idle, now_ms);
            }
            self.alert = Some(match self.monitor.trip_cause {
                Some(SafetyCause::CompressorOverheat) => "SURCHAUFFE COMPRESS.",
                Some(SafetyCause::PressureHigh)
                | Some(SafetyCause::PressureLow)      => "PRESSION HORS LIMITE",
                None                                  => "SECURITE DECLENCHEE",
            });
            state.compressor_allowed = false; // réarmement + MARCHE explicites requis
            self.iso_heating = false;
            return ControlOutput::emergency_stop();
        }

        // ── 2. Bouton ARRÊT pendant un cycle → arrêt propre ──────────────────
        if !state.compressor_allowed
            && matches!(self.task, SystemTask::Cooling(_) | SystemTask::Stabilising)
        {
            self.enter(SystemTask::Stopping(StoppingPhase::CutHighVoltage), now_ms);
        }

        // ── 3. Transitions de phase ──────────────────────────────────────────
        let ctx = PhaseContext {
            state: &*state,
            history,
            now_ms,
            elapsed_ms: now_ms.saturating_sub(self.phase_entered_ms),
        };
        let next = self.task.react_to(&ctx);
        if next != self.task {
            let back_to_idle    = next == SystemTask::Idle;
            let aborted_cooling = back_to_idle
                && matches!(self.task, SystemTask::Cooling(_));
            self.enter(next, now_ms);
            if back_to_idle {
                // Abandon de phase (timeout, capteurs) ou fin d'arrêt propre :
                // on revient en manuel avec le compresseur BLOQUÉ.
                state.compressor_allowed = false;
                if aborted_cooling {
                    // Cooling → Idle direct = abandon (l'arrêt normal passe
                    // par Stopping). Cause : capteurs morts ou phase trop longue.
                    self.alert = Some(
                        if !state.temperatures[CHAMBER_TEMP_IDX].valid
                            || !state.bme280.valid {
                            "CAPTEURS ABSENTS"
                        } else {
                            "TIMEOUT PHASE"
                        });
                }
            }
        }

        // ── 4. Sorties selon la tâche ────────────────────────────────────────
        self.outputs(state, target)
    }

    /// Thermostat à hystérésis du chauffage isopropanol (sortie tout-ou-rien).
    /// `active` = la phase courante autorise le chauffage.
    fn iso_duty(&mut self, state: &SystemState, target: &TargetState, active: bool) -> f32 {
        let iso = &state.temperatures[ISO_TEMP_IDX];
        if !active || !iso.valid {
            self.iso_heating = false; // pas de mesure → sécurité : coupé
            return 0.0;
        }
        if iso.value < target.isopropanol_temp_c - ISO_HYSTERESIS_C {
            self.iso_heating = true;
        } else if iso.value > target.isopropanol_temp_c + ISO_HYSTERESIS_C {
            self.iso_heating = false;
        }
        if self.iso_heating { 1.0 } else { 0.0 }
    }

    /// Sorties actuateurs pour la tâche courante.
    fn outputs(&mut self, state: &SystemState, target: &TargetState) -> ControlOutput {
        use CoolingPhase::*;
        use StoppingPhase::*;

        // (compresseur, chauffage iso actif, HV) selon la phase.
        let (comp, iso_on, hv) = match self.task {
            // Mode manuel — comportement historique.
            SystemTask::Idle => (state.compressor_allowed, true, target.high_voltage_enabled),

            SystemTask::Cooling(SensorCheck)                 => (false, false, false),
            SystemTask::Cooling(PreCoolingThePlate)          => (true,  false, false),
            SystemTask::Cooling(StartingIpaCirculation)      => (true,  true,  false),
            SystemTask::Cooling(SaturatingAirWithIpa)        => (true,  true,  false),
            SystemTask::Cooling(HighVoltage)                 => (true,  true,  true),
            SystemTask::Cooling(FinalCheckBeforeStabilising) => (true,  true,  true),
            SystemTask::Stabilising                          => (true,  true,  true),

            SystemTask::Stopping(CutHighVoltage)          => (true,  false, false),
            SystemTask::Stopping(CutCompressor)           => (false, false, false),
            SystemTask::Stopping(WaitPressureEquilibrium) => (false, false, false),
        };

        let iso_duty = self.iso_duty(state, target, iso_on);

        ControlOutput {
            // L'interlock opérateur (bouton ARRÊT / COMP 0) bloque toujours.
            compressor: comp && state.compressor_allowed,
            isopropanol_heater_duty: iso_duty,
            // Interlock thermique : la haute tension ne s'active jamais sur une
            // chambre trop chaude, quelle que soit la phase. Un seul point de
            // contrôle plutôt qu'une vérification répartie par phase.
            //
            // Rétabli après la review PR #20 : le verrou existait (commit
            // 29d1733, « haut voltage : active seulement si chambre <= target
            // + 5C ») mais `chamber_ready` n'était plus appelé nulle part
            // depuis le merge — le retrait du `allow(dead_code)` global l'a
            // révélé.
            //
            // Fail-safe : `chamber_ready` renvoie `false` si la sonde base
            // chambre est absente ou invalide, donc pas de HV à l'aveugle.
            high_voltage: hv && self.chamber_ready(state, target),
            safety_override: false,
        }
    }

    // ─── Chemin historique (conservé pour test_control.rs) ──────────────────

    /// Calcule les commandes pour ce cycle — mode manuel uniquement,
    /// sécurité non verrouillante (comportement d'origine).
    pub fn step(
        &mut self,
        state:  &SystemState,
        target: &TargetState,
        _dt_s:  f32, // conservé pour compat d'appel (plus de PID à intégrer)
    ) -> ControlOutput {
        // ── 1. Sécurité — priorité absolue ───────────────────────────────────
        if self.safety_triggered(state) {
            self.safety_cycles = self.safety_cycles.saturating_add(1);
            if self.safety_cycles >= 3 {
                self.compressor_on = false;
                self.iso_heating   = false;
                return ControlOutput::emergency_stop();
            }
        } else {
            self.safety_cycles = 0;
        }

        // ── 2. Compresseur — tourne en continu dès qu'il est autorisé ───────
        self.compressor_on = true;

        // ── 3. Chauffage isopropanol — thermostat à hystérésis ───────────────
        let iso_duty = self.iso_duty(state, target, true);

        // ── 4. Haut voltage — suit la commande opérateur ─────────────────────
        let high_voltage = target.high_voltage_enabled;

        ControlOutput {
            compressor: self.compressor_on && state.compressor_allowed,
            isopropanol_heater_duty: iso_duty,
            high_voltage,
            safety_override: false,
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Helpers privés
    // ─────────────────────────────────────────────────────────────────────────

    /// Conditions d'arrêt d'urgence (chemin historique non verrouillant).
    fn safety_triggered(&self, state: &SystemState) -> bool {
        if state.pressure_hp.valid && state.pressure_hp.pressure > SAFETY_HP_MAX {
            return true;
        }
        let t_comp = &state.temperatures[COMPRESSOR_OUT_IDX];
        if t_comp.valid && t_comp.value > SAFETY_TEMP_COMPRESSOR_MAX {
            return true;
        }
        if state.pressure_bp.valid && state.pressure_bp.pressure < SAFETY_BP_MIN {
            return true;
        }
        false
    }

    /// La chambre est prête pour le HV si elle est suffisamment froide.
    fn chamber_ready(&self, state: &SystemState, target: &TargetState) -> bool {
        let chamber = &state.temperatures[CHAMBER_TEMP_IDX];
        if !chamber.valid { return false; }
        chamber.value <= target.chamber_temp_c + HV_READY_WINDOW_C
    }
}
