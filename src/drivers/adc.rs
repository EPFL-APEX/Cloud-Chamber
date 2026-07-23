//! Drivers ADC pour les capteurs de tension et de courant.
//!
//! # Architecture
//!
//! Ce module est organisé en trois couches :
//!
//! ```text
//! AdcVoltageSensor / AdcCurrentSensor   ← logique de conversion (générique)
//!         │ utilise
//!         ▼
//!     AdcChannel (trait)                ← contrat de lecture brute
//!         │ implémenté par
//!         ├── AdcPin<P>  (ARM / RISC-V) ← accès matériel via HAL
//!         └── MockChannel (tests)        ← valeur fixe, sans matériel
//! ```
//!
//! # Conversion ADC → valeur physique
//!
//! L'ADC du RP2040/RP2350 est 12 bits (0–4095), référence 3,3 V câblée sur AVDD.
//! Ces constantes sont identiques sur les deux puces et non exposées par le HAL —
//! elles sont donc intégrées directement dans ce module.
//!
//! ```text
//! valeur = (raw / 4095) × 3,3 V × gain_circuit
//! ```
//!
//! Le paramètre `gain` passé au constructeur représente **uniquement le facteur
//! du circuit de conditionnement** (diviseur de tension, shunt + amplificateur…).
//!
//! | Circuit                              | Calcul du gain          |
//! |--------------------------------------|-------------------------|
//! | Diviseur résistif 1/11 (36 V max)    | `11.0`                  |
//! | Shunt 100 mΩ, ampli gain = 1         | `1.0 / 0.1`             |
//! | Shunt 10 mΩ, ampli ×100             | `1.0 / (0.01 * 100.0)`  |
//!
//! # Partage du périphérique ADC
//!
//! Le RP2040/RP2350 possède un seul convertisseur physique partagé entre tous
//! les canaux. [`init_adc`] doit être appelé une fois dans `main()` pour
//! enregistrer le périphérique dans un `static`. Chaque [`AdcPin`] y accède
//! ensuite via une section critique, garantissant un accès exclusif.

use crate::cloud_chamber_hal::sensors::{Sensor, Measurement};
use crate::cloud_chamber_hal::timer::MonotonicTimer;
use crate::cloud_chamber_hal::units::{Ampere, Volt};

/// Tension de référence de l'ADC, câblée sur AVDD (RP2040/RP2350).
const ADC_VREF: f32 = 3.3;
/// Valeur maximale de l'ADC 12 bits (2¹² − 1).
const ADC_MAX: f32 = 4095.0;

// ── Alias HAL selon la cible (même pattern que main.rs) ─────────────────────
#[cfg(all(rp2040, target_arch = "arm"))]
use rp2040_hal as hal;
#[cfg(all(rp2350, any(target_arch = "arm", target_arch = "riscv32")))]
use rp235x_hal as hal;

// ── Trait local ──────────────────────────────────────────────────────────────

/// Lecture ADC brute sur un canal donné.
///
/// Analogue à [`embedded_hal::digital::InputPin`] pour les GPIO : définit un
/// contrat indépendant du HAL, ce qui permet de substituer un mock dans les
/// tests desktop (où les HAL embarqués ne sont pas compilés).
pub trait AdcChannel {
    /// Retourne la valeur brute du convertisseur (0–4095 sur RP2040/RP2350).
    fn read_raw(&mut self) -> u16;
}

// ── Périphérique ADC partagé ─────────────────────────────────────────────────

/// Périphérique ADC partagé entre tous les canaux.
///
/// - `Mutex` : garantit un accès exclusif en désactivant les interruptions
///   (et la synchronisation inter-cœurs sur RP2040/RP2350).
/// - `RefCell` : permet la mutabilité intérieure depuis la référence partagée
///   fournie par la section critique.
/// - `Option` : autorise une initialisation au runtime via [`init_adc`],
///   car un `static` doit être initialisable à la compilation.
#[cfg(any(target_arch = "arm", target_arch = "riscv32"))]
static ADC: critical_section::Mutex<core::cell::RefCell<Option<hal::adc::Adc>>>
    = critical_section::Mutex::new(core::cell::RefCell::new(None));

/// Enregistre le périphérique ADC dans le `static` partagé.
///
/// À appeler **une seule fois** dans `main()`, après `hal::adc::Adc::new(…)`,
/// avant toute création d'[`AdcPin`]. Appeler [`AdcChannel::read_raw`] sans
/// avoir appelé cette fonction provoque un panic.
#[cfg(any(target_arch = "arm", target_arch = "riscv32"))]
pub fn init_adc(adc: hal::adc::Adc) {
    critical_section::with(|cs| { ADC.borrow_ref_mut(cs).replace(adc); });
}

// ── Struct concrète ──────────────────────────────────────────────────────────

/// Broche GPIO configurée en entrée ADC, implémentant [`AdcChannel`].
///
/// Wraps [`hal::adc::AdcPin`] pour exposer notre trait [`AdcChannel`] sans
/// introduire de dépendance directe au HAL dans la logique de conversion.
/// Les lectures utilisent le périphérique ADC partagé initialisé par [`init_adc`].
///
/// # Exemple
///
/// ```ignore
/// init_adc(hal::adc::Adc::new(pac.ADC, &mut pac.RESETS));
///
/// let sensor = AdcVoltageSensor::new(
///     AdcPin::new(pins.gpio26.into_function()),
///     11.0, // gain du diviseur de tension
/// );
/// ```
#[cfg(any(target_arch = "arm", target_arch = "riscv32"))]
pub struct AdcPin<P: hal::adc::AdcChannel + hal::gpio::AnyPin> {
    pin: hal::adc::AdcPin<P>,
}

#[cfg(any(target_arch = "arm", target_arch = "riscv32"))]
impl<P: hal::adc::AdcChannel + hal::gpio::AnyPin> AdcPin<P> {
    /// Convertit une broche GPIO en entrée ADC.
    ///
    /// `pin` doit être une broche compatible ADC (GP26–GP29 sur RP2040/RP2350).
    /// Passer une broche non-ADC est une erreur détectée à la compilation via
    /// le bound `P: hal::adc::AdcChannel`.
    pub fn new(pin: P) -> Self {
        Self { pin: hal::adc::AdcPin::new(pin).unwrap() }
    }
}

#[cfg(any(target_arch = "arm", target_arch = "riscv32"))]
impl<P: hal::adc::AdcChannel + hal::gpio::AnyPin> AdcChannel for AdcPin<P> {
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

// ── Capteurs ─────────────────────────────────────────────────────────────────

/// Capteur de tension basé sur l'ADC embarqué.
///
/// # Paramètre générique `C`
///
/// `C` doit implémenter [`AdcChannel`]. En production, utiliser [`AdcPin`].
/// Pour les tests, tout type retournant une valeur `u16` fixe suffit.
///
/// # Paramètre `gain`
///
/// Facteur du circuit de conditionnement **uniquement** — les constantes ADC
/// (Vref 3,3 V et résolution 12 bits) sont déjà intégrées dans la conversion.
pub struct AdcVoltageSensor<C: AdcChannel, Clk> {
    channel: C,
    gain: f32,
    clock: Clk,
}

impl<C: AdcChannel, Clk: MonotonicTimer> AdcVoltageSensor<C, Clk> {
    pub fn new(channel: C, gain: f32, clock: Clk) -> Self {
        Self { channel, gain, clock }
    }
}

impl<C: AdcChannel, Clk: MonotonicTimer> Sensor<Measurement<Volt>> for AdcVoltageSensor<C, Clk> {
    type Error = core::convert::Infallible;

    fn read(&mut self) -> Result<Measurement<Volt>, Self::Error> {
        let raw = self.channel.read_raw();
        let value = raw as f32 / ADC_MAX * ADC_VREF * self.gain;
        Ok(Measurement::new(self.clock.get_counter_us(), Volt(value)))
    }
}

/// Capteur de courant basé sur l'ADC embarqué.
///
/// # Paramètre `gain`
///
/// Facteur du circuit de conditionnement **uniquement**.
/// Exemples :
/// - Shunt 100 mΩ, ampli gain = 1 → `gain = 1.0 / 0.1`
/// - Shunt 10 mΩ, ampli ×100 → `gain = 1.0 / (0.01 * 100.0)`
pub struct AdcCurrentSensor<C: AdcChannel, Clk> {
    channel: C,
    gain: f32,
    clock: Clk,
}

impl<C: AdcChannel, Clk: MonotonicTimer> AdcCurrentSensor<C, Clk> {
    pub fn new(channel: C, gain: f32, clock: Clk) -> Self {
        Self { channel, gain, clock }
    }
}

impl<C: AdcChannel, Clk: MonotonicTimer> Sensor<Measurement<Ampere>> for AdcCurrentSensor<C, Clk> {
    type Error = core::convert::Infallible;

    fn read(&mut self) -> Result<Measurement<Ampere>, Self::Error> {
        let raw = self.channel.read_raw();
        let value = raw as f32 / ADC_MAX * ADC_VREF * self.gain;
        Ok(Measurement::new(self.clock.get_counter_us(), Ampere(value)))
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Canal mock retournant toujours la mi-échelle (2048/4095 × 3,3 V ≈ 1,65 V).
    struct MockChannel;

    impl AdcChannel for MockChannel {
        fn read_raw(&mut self) -> u16 { 2048 }
    }

    /// Horloge mock retournant toujours l'instant zéro.
    struct MockClock;

    impl MonotonicTimer for MockClock {
        fn get_counter_us(&self) -> crate::cloud_chamber_hal::timer::Instant {
            crate::cloud_chamber_hal::timer::Instant::new(0)
        }
    }

    #[test]
    fn voltage_sensor_returns_positive_value() {
        let mut sensor = AdcVoltageSensor::new(MockChannel, 3.0, MockClock);
        let v = sensor.read().unwrap().value.0;
        assert!(v > 0.0, "tension doit être positive");
    }

    #[test]
    fn current_sensor_returns_positive_value() {
        let mut sensor = AdcCurrentSensor::new(MockChannel, 3.0, MockClock);
        let a = sensor.read().unwrap().value.0;
        assert!(a > 0.0, "courant doit être positif");
    }

    #[test]
    fn voltage_midscale_is_reasonable() {
        let mut sensor = AdcVoltageSensor::new(MockChannel, 11.0, MockClock);
        let v = sensor.read().unwrap().value.0;
        // 2048/4095 × 3,3 × 11 ≈ 18,17 V
        assert!(v > 17.0 && v < 19.0);
    }
}