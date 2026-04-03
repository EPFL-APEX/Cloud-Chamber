use cloud_chamber_hal::TemperatureSensor;
use onewire;

pub enum Resolution {
    B9,
    B10,
    B11,
    B12
}


pub struct DS18B20<T> {
    wire:&onewire::OneWire<T>,
    device:onewire::Device,
    res:Resolution
} 