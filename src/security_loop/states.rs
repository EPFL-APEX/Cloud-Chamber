use crate::shared::error as shared;
use crate::security_loop::error::Error;

const NUMBER_OF_TEMPS: usize = 5;
const NUMBER_OF_VOLT: usize = 3;
const HISTORY_LENGTH: usize = 10;


pub struct SensorHistory {
    temps: [RingBuffer<f32, HISTORY_LENGTH>; NUMBER_OF_TEMPS],
    volts: [RingBuffer<f32, HISTORY_LENGTH>; NUMBER_OF_VOLT],
    amps: [RingBuffer<f32, HISTORY_LENGTH>; NUMBER_OF_VOLT],
    closeness: RingBuffer<bool, HISTORY_LENGTH>,
}


fn map_error(err: shared::Error, index: usize) -> HistoryError {
    match err {
        shared::Error::IndexOutOfBounds { .. } => {
            Error::HistoryIndexOutOfBounds { index }
        }
    }
}


impl SensorHistory {
    pub fn new() -> Self {
        Self {
            temps: core::array::from_fn(|_| RingBuffer::new()),
            volts: core::array::from_fn(|_| RingBuffer::new()),
            amps: core::array::from_fn(|_| RingBuffer::new()),
            closeness: RingBuffer::new(),
        }
    }
}

impl SensorHistory {
    pub fn push_temp(&mut self, sensor: usize, value: f32) -> Result<()> {
        if sensor >= NUMBER_OF_TEMPS {
            return Err(Error::SensorIndexOutOfBounds { index: sensor });
        }

        self.temps[sensor].push(value);
        Ok(())
    }

    pub fn push_voltage(&mut self, sensor: usize, value: f32) -> Result<()> {
        if sensor >= NUMBER_OF_VOLT {
            return Err(Error::SensorIndexOutOfBounds { index: sensor });
        }

        self.volts[sensor].push(value);
        Ok(())
    }

    pub fn push_amperage(&mut self, sensor: usize, value: f32) -> Result<()> {
        if sensor >= NUMBER_OF_VOLT {
            return Err(Error::SensorIndexOutOfBounds { index: sensor });
        }

        self.amps[sensor].push(value);
        Ok(())
    }

    pub fn push_closeness(&mut self, value: bool) {
        self.closeness.push(value);
    }
}

impl SensorHistory {
    pub fn get_temp(&self, sensor: usize, index: usize) -> HistoryResult<f32> {
        if sensor >= NUMBER_OF_TEMPS {
            return Err(HistoryError::SensorIndexOutOfBounds { index: sensor });
        }

        self.temps[sensor]
            .get(index)
            .map_err(|e| map_error(e, index))
    }

    pub fn get_voltage(&self, sensor: usize, index: usize) -> HistoryResult<f32> {
        if sensor >= NUMBER_OF_VOLT {
            return Err(HistoryError::SensorIndexOutOfBounds { index: sensor });
        }

        self.volts[sensor]
            .get(index)
            .map_err(|e| map_error(e, index))
    }

    pub fn get_amperage(&self, sensor: usize, index: usize) -> HistoryResult<f32> {
        if sensor >= NUMBER_OF_VOLT {
            return Err(HistoryError::SensorIndexOutOfBounds { index: sensor });
        }

        self.amps[sensor]
            .get(index)
        .map_err(|e| map_error(e, index))
    }

    pub fn get_closeness(&self, index: usize) -> HistoryResult<bool> {
        self.closeness
            .get(index)
            .map_err(|e| map_error(e, index))
    }
}