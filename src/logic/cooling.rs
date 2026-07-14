//! All of the logic that goes into cooling the chamber is implemented here

use crate::{logic::probing::{MeasurementHistory, ProbingPlan}, shared::data::SystemTask};


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
    pub fn create_probing_plan(&self, prob_hist: &MeasurementHistory) -> ProbingPlan {
        match self {
            Self::SensorCheck => todo!(),
            Self::PreCoolingThePlate => todo!(),
            Self::StartingIpaCirculation => todo!(),
            Self::SaturatingAirWithIpa => todo!(),
            Self::HighVoltage => todo!(),
            Self::FinalCheckBeforeStabilising => todo!(),
        }
    }

    pub fn react_to(self, current_state: &MeasurementHistory) -> SystemTask {
        match self {
            Self::SensorCheck => todo!(),
            Self::PreCoolingThePlate => todo!(),
            Self::StartingIpaCirculation => todo!(),
            Self::SaturatingAirWithIpa => todo!(),
            Self::HighVoltage => todo!(),
            Self::FinalCheckBeforeStabilising => todo!(),
        }
    }
}