//! Séquence d'arrêt propre.
//!
//! Note vs le croquis de l'équipe (« HV off → attendre équilibrage → couper
//! compresseur ») : l'équilibrage HP ne peut PAS se produire tant que le
//! compresseur tourne (il maintient le ΔP). L'ordre physique correct est :
//! HV off → compresseur off → attendre l'équilibrage avant de rendre la main
//! (protège contre un redémarrage immédiat sur ΔP élevé). À discuter en équipe.

use crate::config::{STOP_EQUALIZE_FALLBACK_MS, STOP_EQUALIZE_HP_MAX, STOP_HV_SETTLE_MS};

use super::{PhaseContext, SystemTask};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoppingPhase {
    /// HV coupé immédiatement; court délai de décharge.
    CutHighVoltage,
    /// Compresseur coupé.
    CutCompressor,
    /// Attente d'équilibrage HP (capteur si présent, sinon temporisation).
    WaitPressureEquilibrium,
}

impl StoppingPhase {
    pub fn react_to(self, ctx: &PhaseContext) -> SystemTask {
        use StoppingPhase::*;
        match self {
            CutHighVoltage => {
                if ctx.elapsed_ms >= STOP_HV_SETTLE_MS {
                    SystemTask::Stopping(CutCompressor)
                } else {
                    SystemTask::Stopping(self)
                }
            }
            CutCompressor => {
                // La coupure est effective dès l'entrée dans la phase (cf.
                // Controller::outputs) — on enchaîne aussitôt sur l'attente.
                if ctx.elapsed_ms >= 500 {
                    SystemTask::Stopping(WaitPressureEquilibrium)
                } else {
                    SystemTask::Stopping(self)
                }
            }
            WaitPressureEquilibrium => {
                let equalized = if ctx.state.pressure_hp.valid {
                    ctx.state.pressure_hp.pressure < STOP_EQUALIZE_HP_MAX
                } else {
                    ctx.elapsed_ms >= STOP_EQUALIZE_FALLBACK_MS
                };
                if equalized { SystemTask::Idle } else { SystemTask::Stopping(self) }
            }
        }
    }
}
