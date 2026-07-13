use crate::config::{
    NUMBER_OF_TEMP_SENSOR, NUMBER_OF_PRESSURE_SENSOR,
    NUMBER_OF_VOLTMETER,
};

#[derive(Clone, Copy, Debug)]
pub struct ProbingPlan {
    temp_sensor_mask:u8,
    pressure_sensor_mask:u8,
    voltmeter_mask:u8,
}

impl ProbingPlan {
    pub fn should_probe() -> bool {
        todo!()
    }
}