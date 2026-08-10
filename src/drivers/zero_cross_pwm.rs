

#[cfg(all(rp2040, target_arch = "arm"))]
use rp2040_hal as hal;
#[cfg(all(rp2350, target_arch = "arm"))]
use rp235x_hal as hal;

use crate::cloud_chamber_hal::actuators::{TargetActuator, AnalogActuator};

struct TriacDriver {

}

impl AnalogActuator for TriacDriver {
    fn set_output(&mut self, value: Unit) -> Result<(), Self::Error> {
        todo!()
    }
}


impl TargetActuator for TriacDriver {
    fn regulate(&mut self, hist: &crate::cloud_chamber_hal::ring_buffer::RingBuffer<crate::cloud_chamber_hal::measurement::Measurement<Unit>, N>, target: Option<Unit>) -> Result<(), Self::Error> {
       todo!()
    }
}
