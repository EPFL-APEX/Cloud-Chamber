///

use crate::shared::{data::{SharedState, TemperatureReading}, ring_buffer::RingBuffer};


const TEMP_GRAPH_BUFFER_LENGTH:usize = 100;

pub struct TempGraphScreen {
    temps_buffer:RingBuffer<TemperatureReading, TEMP_GRAPH_BUFFER_LENGTH>
}

impl TempGraphScreen {
    fn test() {todo!()}
}