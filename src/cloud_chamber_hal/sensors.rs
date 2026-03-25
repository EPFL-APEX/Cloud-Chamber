pub trait TemperatureSensor {
    type Error;
    fn read_celsius(&mut self) -> Result<f32, Self::Error>;
}

pub trait CurrentSensor {
    type Error;
    fn read_amperes(&mut self) -> Result<f32, Self::Error>;
}

pub trait VoltageSensor {
    type Error;
    fn read_voltage(&mut self) -> Result<f32, Self::Error>;
 }

pub trait ClosureSensor {
    type Error;
    fn is_closed(&mut self) -> Result<bool, Self::Error>;
}