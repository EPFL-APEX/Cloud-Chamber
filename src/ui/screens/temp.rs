///

use crate::{
    cloud_chamber_hal::{sensors::Measurement, units::Celsius},
    shared::ring_buffer::RingBuffer,
};


const TEMP_GRAPH_BUFFER_LENGTH:usize = 100;

pub struct TempGraphScreen {
    temps_buffer:RingBuffer<Measurement<Celsius>, TEMP_GRAPH_BUFFER_LENGTH>
}

impl TempGraphScreen {
    fn test() {todo!()}
}