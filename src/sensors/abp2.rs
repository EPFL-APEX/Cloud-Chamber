/// Driver Honeywell ABP2 Series (capteurs de pression I²C) — bloquant.
///
/// Code bloquant, Rust stable, pas d'Embassy.

use embedded_hal::i2c::I2c as I2cTrait;
use embedded_hal::delay::DelayNs;
use crate::data::PressureReading;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Abp2Status { Normal, CommandMode, StaleData, Diagnostic }

impl From<u8> for Abp2Status {
    fn from(val: u8) -> Self {
        match (val >> 6) & 0x03 {
            0 => Self::Normal,
            1 => Self::CommandMode,
            2 => Self::StaleData,
            _ => Self::Diagnostic,
        }
    }
}

pub struct Abp2Config {
    pub address: u8,
    pub p_min:   f32,
    pub p_max:   f32,
    pub label:   &'static str,
}

/// Lit un capteur ABP2 : commande → attente 5ms → lecture 7 octets.
pub fn read_abp2<I, D>(
    i2c: &mut I, delay: &mut D, config: &Abp2Config,
) -> Result<PressureReading, ()>
where I: I2cTrait, D: DelayNs,
{
    i2c.write(config.address, &[0xAA, 0x00, 0x00]).map_err(|_| ())?;
    delay.delay_ms(5);

    let mut buf = [0u8; 7];
    i2c.read(config.address, &mut buf).map_err(|_| ())?;

    let status = Abp2Status::from(buf[0]);
    if status == Abp2Status::Diagnostic { return Err(()); }

    let raw_p: u32 = ((buf[0] as u32 & 0x3F) << 16) | ((buf[1] as u32) << 8) | (buf[2] as u32);
    const OUT_MAX: f32 = 0.9 * 16_777_216.0;
    const OUT_MIN: f32 = 0.1 * 16_777_216.0;
    let pressure = (raw_p as f32 - OUT_MIN) / (OUT_MAX - OUT_MIN)
        * (config.p_max - config.p_min) + config.p_min;

    let raw_t: u32 = ((buf[3] as u32) << 16) | ((buf[4] as u32) << 8) | (buf[5] as u32);
    let temperature = raw_t as f32 / 16_777_216.0 * 200.0 - 50.0;

    Ok(PressureReading { pressure, temperature, valid: status == Abp2Status::Normal })
}
