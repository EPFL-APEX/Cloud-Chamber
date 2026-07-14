use crate::logic::probing::{MeasurementHistory, ProbingPlan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoppingPhase {
    Todo
}

impl StoppingPhase {
    pub fn create_probing_plan(&self, prob_hist: &MeasurementHistory) -> ProbingPlan {
        todo!()
    }
}