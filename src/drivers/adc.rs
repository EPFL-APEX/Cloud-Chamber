//! Drivers ADC pour les capteurs de tension et de courant.
//!
//! # Conversion ADC → valeur physique
//!
//! L'ADC du RP2040/RP2350 est 12 bits (0–4095). La conversion en grandeur
//! physique dépend du gain du circuit de conditionnement :
//!
//! ```text
//! tension = (raw / 4095.0) * V_REF * GAIN_FACTOR
//! ```
//!
//! Les valeurs de `VOLTAGE_SCALE` et `CURRENT_SCALE` doivent être ajustées
//! selon le schéma électrique du projet.

use crate::cloud_chamber_hal::sensors::{CurrentSensor, VoltageSensor};

const ADC_VREF: f32 = 3.3;   // fixe, lié au matériel RP2040/2350
const ADC_MAX:  f32 = 4095.;  // 2^12 - 1

// ── Alias HAL (même pattern que main.rs) ────────────────────────────────────
#[cfg(all(rp2040, target_arch = "arm"))]
use rp2040_hal as hal;
#[cfg(all(rp2350, any(target_arch = "arm", target_arch = "riscv32")))]
use rp235x_hal as hal;

// ── Trait local — détail driver, pas dans sensors.rs ────────────────────────
pub trait AdcChannel {
    fn read_raw(&mut self) -> u16;
}

// ── Périphérique ADC partagé (même pattern que SHARED dans shared/data.rs) ──
#[cfg(any(target_arch = "arm", target_arch = "riscv32"))]
static ADC: critical_section::Mutex<core::cell::RefCell<Option<hal::adc::Adc>>>
    = critical_section::Mutex::new(core::cell::RefCell::new(None));

/// À appeler une fois dans main() après `Adc::new(...)`.
#[cfg(any(target_arch = "arm", target_arch = "riscv32"))]
pub fn init_adc(adc: hal::adc::ADC) {
    critical_section::with(|cs| { ADC.borrow_ref_mut(cs).replace(adc); })
}

// ── Struct concrète — analogue à AdcPin::new() du rp-hal ────────────────────
#[cfg(any(target_arch = "arm", target_arch = "riscv32"))]
pub struct AdcPin<P: hal::adc::AdcChannel> {
    pin: hal::adc::AdcChannel
}

// Pourquoi le new ?
#[cfg(any(target_arch = "arm", target_arch = "riscv32"))]
impl<P: hal::adc::AdcChannel> AdcPin<P>{
    pub fn new(pin: P) -> Self {
        Self { pin: hal::adc::AdcPin::new(pin).unwrap()}
    }
}

#[cfg(any(target_arch = "arm", target_arch = "riscv32"))]
impl<P: hal::adc::AdcChannel> AdcChannel for AdcPin<P> {
    fn read_raw(&mut self) -> u16 {
        critical_section::with(|cs| {
            ADC.borrow_ref_mut(cs)
            .as_mut()
            .expect("init_adc() non appelé")
            .read(&mut self.pin)
            .unwrap_or(0)
        })
    }
}


/// Capteur de tension basé sur l'ADC embarqué.
///
/// # Paramètre générique `Channel`
///
/// En production, `Channel` sera le type de canal ADC du HAL embarqué.
/// Pour les tests, on peut substituer un type mock.
/// 
/// # Paramètre `voltage_scale` facteur de conversion ADC → Volts (à calibrer selon le diviseur de tension).
/// 
pub struct AdcVoltageSensor<Channel: AdcChannel> {
    channel: Channel,
    gain: f32,
}

impl<Channel: AdcChannel> AdcVoltageSensor<Channel> {
    pub fn new(channel: Channel, gain: f32) -> Self {
        Self { channel, gain }
    }
}

impl<Channel: AdcChannel> VoltageSensor for AdcVoltageSensor<Channel> {
    type Error = core::convert::Infallible;

    fn read_voltage(&mut self) -> Result<f32, Self::Error> {
        let raw = self.channel.read_raw();
        Ok(raw as f32 / ADC_MAX * ADC_VREF * self.gain)
    }
}

/// Capteur de courant basé sur l'ADC embarqué.
/// Facteur de conversion ADC → Ampères (à calibrer selon le shunt/amplificateur).
pub struct AdcCurrentSensor<Channel: AdcChannel> {
    channel: Channel,
    gain: f32,
}

impl<Channel: AdcChannel> AdcCurrentSensor<Channel> {
    pub fn new(channel: Channel, gain: f32) -> Self {
        Self { channel, gain }
    }
}

impl<Channel: AdcChannel> CurrentSensor for AdcCurrentSensor<Channel> {
    type Error = core::convert::Infallible;

    fn read_amperes(&mut self) -> Result<f32, Self::Error> {
        let raw = self.channel.read_raw();
        Ok(raw as f32 / ADC_MAX * ADC_VREF * self.gain)
    }
}


// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    
    /// Lecture ADC brute — stub retournant mi-échelle.
    ///
    /// À remplacer par un vrai appel HAL en production :
    /// `adc.read(&mut channel).unwrap_or(0)`
    #[inline]
    fn read_raw_stub() -> u16 {
        2048
    }

    struct MockChannel;

    impl AdcChannel for MockChannel {
        fn read_raw(&mut self) -> u16 {
           read_raw_stub() 
        }
    }

    #[test]
    fn voltage_sensor_returns_positive_value() {
        let mut sensor = AdcVoltageSensor::new(MockChannel, 3.);
        let v = sensor.read_voltage().unwrap();
        assert!(v > 0.0, "tension doit être positive");
    }

    #[test]
    /// Il faudrait faire des tests plus complets sur le read_amperes
    fn current_sensor_returns_positive_value() {
        let mut sensor = AdcCurrentSensor::new(MockChannel, 3.);
        let a = sensor.read_amperes().unwrap();
        assert!(a > 0.0, "courant doit être positif");
    }

    #[test]
    fn voltage_midscale_is_reasonable() {
        let mut sensor = AdcVoltageSensor::new(MockChannel, 11.);
        let v = sensor.read_voltage().unwrap();
        // 2048/4095 * 3.3 * 11 ≈ 18.17 V
        assert!(v > 17.0 && v < 19.0);
    }
}