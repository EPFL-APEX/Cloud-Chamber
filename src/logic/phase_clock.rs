//! Limites temporelles de chaque état.
//!
//! `cooling.rs` et `stopping.rs` ne portent aucune notion de durée : leurs
//! transitions ne dépendent que des mesures. C'est donc ici, et nulle part
//! ailleurs, qu'un état est associé à un temps.
//!
//! `min_duration_ms` fait avancer les phases sans capteur dédié (circulation
//! IPA, décharge HT) ; `timeout_ms` abandonne celles qui attendent un seuil
//! jamais atteint. `None` = pas de limite, et le type interdit de comparer
//! par inadvertance une limite absente à une durée.

use crate::logic::timing::{
    FINAL_CHECK_TIMEOUT_MS, HV_STABILISE_TIMEOUT_MS, IPA_CIRCULATION_MS, PRECOOL_TIMEOUT_MS,
    SATURATION_TIMEOUT_MS, SENSOR_CHECK_TIMEOUT_MS, SENSOR_LOSS_MS, STOP_COMPRESSOR_SETTLE_MS,
    STOP_EQUALIZE_FALLBACK_MS, STOP_HV_SETTLE_MS, STOP_ISOPROP_SETTLE_MS,
};
use crate::cloud_chamber_hal::actuators::ActuatorPlan;
use crate::cloud_chamber_hal::timer::{Duration, Instant, MonotonicTimer};
use crate::logic::cooling::CoolingPhase;
use crate::logic::probing::MeasurementHistory;
use crate::logic::stopping::StoppingPhase;
use crate::shared::data::SystemTask;

// #todo Est-ce qu'on utilise les unités définies dans le hal ?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseDurations {
    Duration(u64),
    Timeout(u64),
    Unbound,
}

impl PhaseDurations {
    const fn unbounded() -> Self {
        Self::Unbound
    }

    const fn timed(min_duration_ms: u64) -> Self {
        Self::Duration(min_duration_ms)
    }

    const fn with_timeout(timeout_ms: u64) -> Self {
        Self::Timeout(timeout_ms)
    }
}

impl SystemTask {
    pub fn durations(&self) -> PhaseDurations {
        use CoolingPhase::*;
        use StoppingPhase::*;

        match self {
            SystemTask::Idle => PhaseDurations::unbounded(),

            SystemTask::Cooling(SensorCheck) => {
                PhaseDurations::with_timeout(SENSOR_CHECK_TIMEOUT_MS)
            }
            SystemTask::Cooling(PreCoolingThePlate) => {
                PhaseDurations::with_timeout(PRECOOL_TIMEOUT_MS)
            }
            // Aucun capteur ne mesure la circulation d'isopropanol : le temps
            // est le seul témoin, il déclenche donc la transition.
            SystemTask::Cooling(StartingIpaCirculation) => {
                PhaseDurations::timed(IPA_CIRCULATION_MS)
            }
            SystemTask::Cooling(SaturatingAirWithIpa) => {
                PhaseDurations::with_timeout(SATURATION_TIMEOUT_MS)
            }
            SystemTask::Cooling(HighVoltage) => {
                PhaseDurations::with_timeout(HV_STABILISE_TIMEOUT_MS)
            }
            SystemTask::Cooling(FinalCheckBeforeStabilising) => {
                PhaseDurations::with_timeout(FINAL_CHECK_TIMEOUT_MS)
            }

            SystemTask::Stabilising => PhaseDurations::unbounded(),

            SystemTask::Stopping(CutHighVoltage) => PhaseDurations::timed(STOP_HV_SETTLE_MS),
            SystemTask::Stopping(CutIsoprop) => PhaseDurations::timed(STOP_ISOPROP_SETTLE_MS),
            SystemTask::Stopping(CutCompressor) => {
                PhaseDurations::timed(STOP_COMPRESSOR_SETTLE_MS)
            }
            // Repli quand le capteur HP est absent ou trop lent : contrairement
            // aux autres timeouts, la fin de phase reste normale.
            SystemTask::Stopping(WaitPressureEquilibrium) => {
                PhaseDurations::with_timeout(STOP_EQUALIZE_FALLBACK_MS)
            }

            // Verrouillé jusqu'au réarmement opérateur.
            SystemTask::Tripped(_) => PhaseDurations::unbounded(),
        }
    }
}

/// Abandon si la sonde base-chambre reste invalide trop longtemps en cours
/// de cycle (hors `SensorCheck`, qui attend justement cette sonde, et hors
/// `Idle`/`Stabilising`/`Tripped`, qui n'en dépendent pas de la même façon).
fn sensor_loss_abort(task: SystemTask, chamber_stale: Duration) -> Option<SystemTask> {
    let mid_cycle = matches!(task, SystemTask::Cooling(p) if p != CoolingPhase::SensorCheck);
    (mid_cycle && chamber_stale.as_millis() > SENSOR_LOSS_MS).then_some(SystemTask::Idle)
}

/// Où va une phase purement temporisée (`PhaseDurations::Duration`) une fois
/// sa durée minimale écoulée — `durations()` dit "combien de temps", pas
/// "vers où", ce petit aiguillage complète l'info pour les 3 seules phases
/// concernées.
fn next_after_duration(task: SystemTask) -> SystemTask {
    use CoolingPhase::*;
    use StoppingPhase::*;
    match task {
        SystemTask::Cooling(StartingIpaCirculation) => SystemTask::Cooling(SaturatingAirWithIpa),
        // Ordre physique de l'arrêt : HV, puis circulation IPA, puis
        // compresseur. `CutIsoprop` était déclaré dans `StoppingPhase` mais
        // sauté ici — la pompe n'était jamais coupée avant le compresseur.
        SystemTask::Stopping(CutHighVoltage) => SystemTask::Stopping(CutIsoprop),
        SystemTask::Stopping(CutIsoprop) => SystemTask::Stopping(CutCompressor),
        SystemTask::Stopping(CutCompressor) => SystemTask::Stopping(WaitPressureEquilibrium),
        // N'arrive pas si `durations()` reste cohérent avec ce match — reste
        // dans la phase plutôt que de paniquer sur une incohérence interne.
        _ => task,
    }
}

/// Délais/timeouts purs, dérivés de `SystemTask::durations()`. Un timeout
/// dépassé abandonne toujours vers `Idle` ; une durée minimale écoulée fait
/// avancer vers la phase suivante (`next_after_duration`).
fn timed_transition(task: SystemTask, elapsed_ms: u64) -> Option<SystemTask> {
    match task.durations() {
        PhaseDurations::Timeout(timeout_ms) if elapsed_ms > timeout_ms => Some(SystemTask::Idle),
        PhaseDurations::Duration(min_duration_ms) if elapsed_ms >= min_duration_ms => {
            Some(next_after_duration(task))
        }
        _ => None,
    }
}

/// Combine mesure (`react_to`, prioritaire), abandon perte-capteur, puis
/// délai/timeout (repli) — la seule décision "quel est le prochain état
/// en mode automatique". Fonction pure : ne possède rien, ne consulte pas
/// la sécurité (priorité absolue gérée par l'appelant, cf.
/// `control_loop.rs::run()`), ne sait rien d'une horloge — juste des
/// durées déjà calculées.
pub fn advance(
    current: SystemTask, history: &MeasurementHistory, elapsed: Duration, chamber_stale: Duration,
) -> (SystemTask, ActuatorPlan) {
    let (reacted, plan) = current.react_to(history);
    if reacted != current {
        return (reacted, plan);
    }
    let next = sensor_loss_abort(current, chamber_stale)
        .or_else(|| timed_transition(current, elapsed.as_millis()))
        .unwrap_or(current);
    (next, plan)
}

/// Mémorise la phase courante et depuis quand, en lisant l'horloge de
/// l'appareil qu'il possède — seule responsabilité de ce type. Ne connaît
/// ni la sécurité, ni les capteurs, ni comment décider la phase suivante
/// (`advance`, ci-dessus) : uniquement "quelle phase, depuis quand".
pub struct PhaseClock<Clk: MonotonicTimer> {
    clock: Clk,
    current: SystemTask,
    entered_at: Instant,
}

impl<Clk: MonotonicTimer> PhaseClock<Clk> {
    pub fn new(clock: Clk, initial: SystemTask) -> Self {
        let entered_at = clock.now();
        Self { clock, current: initial, entered_at }
    }

    pub fn current(&self) -> SystemTask {
        self.current
    }

    /// Instant courant — ce que réclament `SafetyMonitor::check` et
    /// `MeasurementHistory::chamber_stale_duration`, qui raisonnent en
    /// [`Instant`] et non en millisecondes nues.
    pub fn now(&self) -> Instant {
        self.clock.now()
    }

    /// Temps passé dans la phase courante. Renvoie une [`Duration`] et non
    /// des millisecondes nues : c'est ce que `advance` attend, et passer par
    /// un `u64` de ms au milieu perdrait la précision sous la milliseconde
    /// que `elapsed_since` fournit déjà.
    pub fn elapsed(&self) -> Duration {
        self.clock.elapsed_since(self.entered_at)
    }

    /// Force une transition — remet l'horloge de phase à zéro si l'état
    /// change, sans effet sinon.
    pub fn set(&mut self, task: SystemTask) {
        if task != self.current {
            self.current = task;
            self.entered_at = self.clock.now();
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_chamber_hal::config::CHAMBER_TEMP_IDX;
    use crate::cloud_chamber_hal::measurement::Measurement;
    use crate::cloud_chamber_hal::units::Celsius;
    use crate::config::operating::{IPA_HEATER_TARGET_C, SATURATION_TARGET_C};
    use crate::drivers::mock::MockClock;
    use crate::logic::security::SafetyCause;

    // ─── PhaseDurations ─────────────────────────────────────────────────────

    #[test]
    fn idle_has_no_limit() {
        assert_eq!(SystemTask::Idle.durations(), PhaseDurations::Unbound);
    }

    #[test]
    fn tripped_never_expires_on_its_own() {
        let d = SystemTask::Tripped(SafetyCause::CompressorOverheat).durations();
        assert_eq!(d, PhaseDurations::Unbound);
    }

    #[test]
    fn ipa_circulation_is_purely_timed() {
        let d = SystemTask::Cooling(CoolingPhase::StartingIpaCirculation).durations();
        assert_eq!(d, PhaseDurations::Duration(IPA_CIRCULATION_MS));
    }

    #[test]
    fn sensor_driven_phases_have_a_timeout() {
        let d = SystemTask::Cooling(CoolingPhase::PreCoolingThePlate).durations();
        assert_eq!(d, PhaseDurations::Timeout(PRECOOL_TIMEOUT_MS));
    }

    // ─── advance() ──────────────────────────────────────────────────────────
    // Fonction pure : pas de PhaseClock/horloge à construire pour la tester.

    fn history_with_chamber_temp(value_c: f32) -> MeasurementHistory {
        let mut h = MeasurementHistory::new();
        h.temps[CHAMBER_TEMP_IDX].push(Measurement::new(Instant::from_micros(0), Celsius(value_c)));
        h
    }

    #[test]
    fn advance_prioritizes_measurement_over_timing() {
        // Un capteur base-chambre valide fait avancer SensorCheck
        // immédiatement, bien avant tout timeout/durée.
        let history = history_with_chamber_temp(-5.0);
        let (next, _plan) =
            advance(SystemTask::Cooling(CoolingPhase::SensorCheck), &history, Duration::from_millis(1), Duration::ZERO);
        assert_eq!(next, SystemTask::Cooling(CoolingPhase::PreCoolingThePlate));
    }

    #[test]
    fn advance_times_out_back_to_idle() {
        let history = MeasurementHistory::new(); // chambre toujours NaN
        let (next, _plan) = advance(
            SystemTask::Cooling(CoolingPhase::SensorCheck), &history, Duration::from_millis(SENSOR_CHECK_TIMEOUT_MS + 1), Duration::ZERO,
        );
        assert_eq!(next, SystemTask::Idle);
    }

    #[test]
    fn advance_advances_after_minimum_duration() {
        // Capteur base-chambre valide (sinon `sensor_loss_abort` prend la
        // main avant même que la durée minimale ne soit évaluée).
        let history = history_with_chamber_temp(-25.0);
        let (next, _plan) = advance(
            SystemTask::Cooling(CoolingPhase::StartingIpaCirculation), &history, Duration::from_millis(IPA_CIRCULATION_MS), Duration::ZERO,
        );
        assert_eq!(next, SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa));
    }

    #[test]
    fn advance_sensor_loss_aborts_mid_cycle() {
        let history = MeasurementHistory::new(); // chambre jamais valide
        let (next, _plan) = advance(
            SystemTask::Cooling(CoolingPhase::PreCoolingThePlate), &history, Duration::ZERO, Duration::from_millis(SENSOR_LOSS_MS + 1),
        );
        assert_eq!(next, SystemTask::Idle);
    }

    #[test]
    fn advance_sensor_loss_does_not_apply_during_sensor_check() {
        // SensorCheck attend justement cette sonde — pas d'abandon perte-
        // capteur ici, seul le timeout de phase (30s, pas encore atteint
        // à SENSOR_LOSS_MS+1 = 10001ms) s'applique.
        let history = MeasurementHistory::new();
        let (next, _plan) = advance(
            SystemTask::Cooling(CoolingPhase::SensorCheck), &history, Duration::ZERO, Duration::from_millis(SENSOR_LOSS_MS + 1),
        );
        assert_eq!(next, SystemTask::Cooling(CoolingPhase::SensorCheck));
    }

    #[test]
    fn advance_stabilising_holds_outputs_without_timing_out() {
        // `Stabilising` lit `shared::settings::get()` (cf. control_loop.rs)
        // — verrou nécessaire, cf. commentaire de `with_isolated_settings`.
        crate::shared::settings::with_isolated_settings(|| {
            let history = MeasurementHistory::new();
            let (next, plan) = advance(SystemTask::Stabilising, &history, Duration::from_millis(10 * 60 * 60 * 1000), Duration::ZERO); // 10h
            assert_eq!(next, SystemTask::Stabilising);
            assert_eq!(
                plan,
                ActuatorPlan {
                    cooling: Some(SATURATION_TARGET_C),
                    iso_heater: Some(IPA_HEATER_TARGET_C),
                    high_voltage: true,
                    iso_pump: false, lights: None, glass_heater: false,
                }
            );
        });
    }

    // ─── PhaseClock ─────────────────────────────────────────────────────────
    // Horloge factice pilotable — `MockClock` (drivers::mock), réutilisée
    // telle quelle par les tests de control_loop.rs.

    #[test]
    fn phase_clock_starts_at_initial_task() {
        let ticks = MockClock::new(0);
        let clock = PhaseClock::new(&ticks, SystemTask::Idle);
        assert_eq!(clock.current(), SystemTask::Idle);
        assert_eq!(clock.elapsed(), Duration::from_millis(0));
    }

    #[test]
    fn phase_clock_elapsed_ms_grows_with_the_device_clock() {
        let ticks = MockClock::new(0);
        let clock = PhaseClock::new(&ticks, SystemTask::Idle);
        ticks.advance_ms(500);
        assert_eq!(clock.elapsed(), Duration::from_millis(500));
    }

    #[test]
    fn phase_clock_set_resets_elapsed_on_transition() {
        let ticks = MockClock::new(0);
        let mut clock = PhaseClock::new(&ticks, SystemTask::Idle);
        ticks.advance_ms(500);
        clock.set(SystemTask::Cooling(CoolingPhase::SensorCheck));
        assert_eq!(clock.elapsed(), Duration::from_millis(0));
        assert_eq!(clock.current(), SystemTask::Cooling(CoolingPhase::SensorCheck));
    }

    #[test]
    fn phase_clock_set_is_a_no_op_for_the_same_task() {
        let ticks = MockClock::new(0);
        let mut clock = PhaseClock::new(&ticks, SystemTask::Idle);
        ticks.advance_ms(500);
        clock.set(SystemTask::Idle); // même état : ne remet rien à zéro
        assert_eq!(clock.elapsed(), Duration::from_millis(500));
    }

    #[test]
    fn phase_clock_now_reflects_the_device_clock() {
        let ticks = MockClock::new(0);
        let clock = PhaseClock::new(&ticks, SystemTask::Idle);
        ticks.advance_ms(250);
        assert_eq!(clock.now(), Instant::from_micros(250_000));
    }
}
