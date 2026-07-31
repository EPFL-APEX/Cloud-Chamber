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

use crate::config::{
    FINAL_CHECK_TIMEOUT_MS, HV_STABILISE_TIMEOUT_MS, IPA_CIRCULATION_MS, PRECOOL_TIMEOUT_MS,
    SATURATION_TIMEOUT_MS, SENSOR_CHECK_TIMEOUT_MS, STOP_COMPRESSOR_SETTLE_MS,
    STOP_EQUALIZE_FALLBACK_MS, STOP_HV_SETTLE_MS,
};
use crate::logic::cooling::CoolingPhase;
use crate::logic::stopping::StoppingPhase;
use crate::shared::data::SystemTask;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseDurations {
    Duration(u64),
    Timout(u64),
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
        Self::Timout(timeout_ms)
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

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::security::SafetyCause;

    #[test]
    fn idle_has_no_limit() {
        let d = SystemTask::Idle.durations();
        assert!(d.min_duration_ms.is_none());
        assert!(d.timeout_ms.is_none());
    }

    #[test]
    fn tripped_never_expires_on_its_own() {
        let d = SystemTask::Tripped(SafetyCause::CompressorOverheat).durations();
        assert!(d.timeout_ms.is_none());
    }

    #[test]
    fn ipa_circulation_is_purely_timed() {
        let d = SystemTask::Cooling(CoolingPhase::StartingIpaCirculation).durations();
        assert_eq!(d.min_duration_ms, Some(IPA_CIRCULATION_MS));
        assert!(d.timeout_ms.is_none());
    }

    #[test]
    fn sensor_driven_phases_have_a_timeout() {
        let d = SystemTask::Cooling(CoolingPhase::PreCoolingThePlate).durations();
        assert_eq!(d.timeout_ms, Some(PRECOOL_TIMEOUT_MS));
        assert!(d.min_duration_ms.is_none());
    }

    /// Aucun état n'utilise les deux limites. Si ce test casse, c'est un
    /// changement de conception à trancher, pas un détail.
    #[test]
    fn no_phase_uses_both_limits() {
        let all = [
            SystemTask::Idle,
            SystemTask::Cooling(CoolingPhase::SensorCheck),
            SystemTask::Cooling(CoolingPhase::PreCoolingThePlate),
            SystemTask::Cooling(CoolingPhase::StartingIpaCirculation),
            SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa),
            SystemTask::Cooling(CoolingPhase::HighVoltage),
            SystemTask::Cooling(CoolingPhase::FinalCheckBeforeStabilising),
            SystemTask::Stabilising,
            SystemTask::Stopping(StoppingPhase::CutHighVoltage),
            SystemTask::Stopping(StoppingPhase::CutCompressor),
            SystemTask::Stopping(StoppingPhase::WaitPressureEquilibrium),
            SystemTask::Tripped(SafetyCause::PressureHigh),
        ];
        for task in all {
            let d = task.durations();
            assert!(!(d.min_duration_ms.is_some() && d.timeout_ms.is_some()), "{task:?}");
        }
    }
}
