/// Traits et drivers capteurs.
///
/// Même architecture que le projet partenaire (rust-init-refactor) :
/// les traits définissent les interfaces, les structs concrètes les implémentent.

pub mod ds18b20;
pub mod onewire;
pub mod bme280;
pub mod abp2;

// ════════════════════════════════════════════════════════════════════════════
// Traits — identiques à cloud_chamber_hal/sensors.rs du projet partenaire
// ════════════════════════════════════════════════════════════════════════════

/// Capteur de température retournant des degrés Celsius.
///
/// `start_measurement()` déclenche la conversion (peut revenir immédiatement).
/// L'appelant attend le délai nécessaire avant d'appeler `read_celsius()`.
pub trait TemperatureSensor {
    type Error;
    fn start_measurement(&mut self) -> Result<(), Self::Error>;
    fn read_celsius(&mut self) -> Result<f32, Self::Error>;
}

/// Capteur de pression retournant des Pascal.
pub trait PressureSensor {
    type Error;
    fn read_pascal(&mut self) -> Result<f32, Self::Error>;
}
