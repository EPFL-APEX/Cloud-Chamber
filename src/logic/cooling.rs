//! Séquence de démarrage (refroidissement) — mêmes phases que la branche
//! add-phase-transition-logic, transitions implémentées (les todo!() de la
//! branche cible sont remplacés par les conditions réelles).

use crate::config::{
    CHAMBER_TEMP_IDX, FINAL_CHECK_MS, FINAL_CHECK_TIMEOUT_MS, HV_STABILISE_TIMEOUT_MS,
    IPA_CIRCULATION_MS, PRECOOL_TARGET_C, PRECOOL_TIMEOUT_MS, SATURATION_TARGET_C,
    SATURATION_TIMEOUT_MS, SENSOR_CHECK_TIMEOUT_MS, STABLE_TOLERANCE_C, STABLE_WINDOW_MS,
};

use super::{PhaseCtx, SystemTask};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoolingPhase {
    SensorCheck,
    PreCoolingThePlate,
    StartingIpaCirculation,
    SaturatingAirWithIpa,
    HighVoltage,
    FinalCheckBeforeStabilising,
}

impl CoolingPhase {
    pub fn react_to(self, ctx: &PhaseCtx) -> SystemTask {
        use CoolingPhase::*;

        let chamber = ctx.state.temperatures[CHAMBER_TEMP_IDX];
        let chamber_t = if chamber.valid { Some(chamber.value) } else { None };

        match self {
            // Tous les capteurs nécessaires répondent avant de démarrer.
            SensorCheck => {
                if chamber_t.is_some() && ctx.state.bme280.valid {
                    SystemTask::Cooling(PreCoolingThePlate)
                } else if ctx.elapsed_ms > SENSOR_CHECK_TIMEOUT_MS {
                    SystemTask::Idle // capteurs absents → abandon
                } else {
                    SystemTask::Cooling(self)
                }
            }

            // Compresseur ON, on attend que la base passe sous PRECOOL_TARGET_C.
            PreCoolingThePlate => match chamber_t {
                Some(t) if t <= PRECOOL_TARGET_C => SystemTask::Cooling(StartingIpaCirculation),
                _ if ctx.elapsed_ms > PRECOOL_TIMEOUT_MS => SystemTask::Idle, // n'atteint pas la cible
                _ => SystemTask::Cooling(self),
            },

            // Chauffage IPA ON — pas de capteur dédié : temporisation.
            StartingIpaCirculation => {
                if ctx.elapsed_ms >= IPA_CIRCULATION_MS {
                    SystemTask::Cooling(SaturatingAirWithIpa)
                } else {
                    SystemTask::Cooling(self)
                }
            }

            // L'IPA sature l'air pendant que la base continue de descendre.
            SaturatingAirWithIpa => match chamber_t {
                Some(t) if t <= SATURATION_TARGET_C => SystemTask::Cooling(HighVoltage),
                _ if ctx.elapsed_ms > SATURATION_TIMEOUT_MS => SystemTask::Idle,
                _ => SystemTask::Cooling(self),
            },

            // HV ON — on attend la stabilisation thermique de la base.
            HighVoltage => {
                if ctx.hist.temp_stable(CHAMBER_TEMP_IDX, STABLE_WINDOW_MS,
                                        STABLE_TOLERANCE_C, ctx.now_ms) {
                    SystemTask::Cooling(FinalCheckBeforeStabilising)
                } else if ctx.elapsed_ms > HV_STABILISE_TIMEOUT_MS {
                    SystemTask::Idle
                } else {
                    SystemTask::Cooling(self)
                }
            }

            // Vérifications finales avant le régime permanent.
            FinalCheckBeforeStabilising => {
                let cold_enough = matches!(chamber_t, Some(t) if t <= SATURATION_TARGET_C + 2.0);
                if ctx.elapsed_ms >= FINAL_CHECK_MS && cold_enough {
                    SystemTask::Stabilising
                } else if ctx.elapsed_ms > FINAL_CHECK_TIMEOUT_MS {
                    SystemTask::Idle
                } else {
                    SystemTask::Cooling(self)
                }
            }
        }
    }
}
