///

use crate::cloud_chamber_hal::{measurement::Measurement, ring_buffer::RingBuffer, units::Celsius};


const TEMP_GRAPH_BUFFER_LENGTH:usize = 100;

pub struct TempGraphScreen {
    temps_buffer:RingBuffer<Measurement<Celsius>, TEMP_GRAPH_BUFFER_LENGTH>
}

impl TempGraphScreen {
    fn test() {todo!()}
}