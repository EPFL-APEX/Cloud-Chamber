//! Point d'entrée du second cœur (Core1) — boucle de sécurité temps-réel.
//!
//! # Multi-cœurs sur RP2040/RP2350
//!
//! Le RP2040 et le RP2350 possèdent deux cœurs ARM Cortex-M identiques.
//! Core0 démarre automatiquement. Core1 doit être lancé explicitement
//! avec `multicore.spawn_core1(func, stack)`.
//!
//! # Note pour l'implémentation finale
//!
//! Ce module est un squelette structurel. Les types concrets des drivers
//! (GPIO, SPI, ADC) dépendent du câblage physique de votre système.
//! Remplacez les commentaires `TODO:` par les initialisations HAL réelles.

// Imports utilisés dans les TODO — silencer les warnings jusqu'à l'implémentation.
#[allow(unused_imports)]
use crate::security_loop::{
    loop_runner::{CriticalSectionWriter, SecurityLoop},
    safety::SafetyConfig,
};

// Alias HAL avec garde architecture — build.rs émet `rp2350` même sur desktop
// quand `.pico-rs` est absent, mais rp235x_hal n'est pas une dépendance x86.
#[cfg(all(rp2040, target_arch = "arm"))]
use rp2040_hal as hal;

#[cfg(all(rp2350, any(target_arch = "arm", target_arch = "riscv32")))]
use rp235x_hal as hal;

/// Point d'entrée de Core1 — ne retourne jamais.
pub fn core1_task() -> ! {
    // TODO: Construire la SecurityLoop avec les drivers concrets :
    //
    // ```rust
    // use crate::drivers::{adc::AdcCurrentSensor, breaker::GpioBreaker};
    //
    // let mut security_loop = SecurityLoop::new(
    //     hal_timer,
    //     hal_watchdog,
    //     [Ds18b20Sensor::new(...); NUMBER_OF_TEMPS],
    //     [AdcVoltageSensor::new(3.3, 75.0); NUMBER_OF_VOLT],
    //     [AdcCurrentSensor::new(0.1, 1.65); NUMBER_OF_AMP],
    //     GpioClosureSensor::new(pin),
    //     GpioBreaker::new(breaker_pin, true),
    //     CriticalSectionWriter,
    //     SafetyConfig::default(),
    // );
    // security_loop.run()
    // ```

    loop {
        #[cfg(target_arch = "arm")]
        cortex_m::asm::nop();
        #[cfg(not(target_arch = "arm"))]
        core::hint::spin_loop();
    }
}

/// Taille de la pile allouée pour Core1 (4096 × 4 = 16 Ko).
pub const CORE1_STACK_SIZE: usize = 4096;

/// Pile statique de Core1.
#[cfg(any(target_arch = "arm", target_arch = "riscv32"))]
pub static CORE1_STACK: hal::multicore::Stack<CORE1_STACK_SIZE> =
    hal::multicore::Stack::new();
