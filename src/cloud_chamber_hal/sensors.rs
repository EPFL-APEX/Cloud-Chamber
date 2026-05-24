//! Traits pour les capteurs de mesure.
//!
//! # Pourquoi des traits séparés par type de capteur ?
//!
//! Chaque trait représente une **capacité** précise. Une structure peut
//! implémenter plusieurs traits (ex: un module I2C multi-capteurs).
//! La logique métier dépend uniquement du trait, pas du type concret :
//! on peut substituer n'importe quelle implémentation sans modifier
//! `SecurityLoop`.

/// Capteur de température retournant des degrés Celsius.
pub trait TemperatureSensor {
    type Error;
    /// Déclenche une conversion (peut être asynchrone sur certains capteurs).
    fn start_measurement(&mut self) -> Result<(), Self::Error>;
    /// Lit la dernière température convertie, en °C.
    fn read_celsius(&mut self) -> Result<f32, Self::Error>;
}

/// Capteur de tension retournant des Volts.
pub trait VoltageSensor {
    type Error;
    fn read_voltage(&mut self) -> Result<f32, Self::Error>;
}

/// Capteur de courant retournant des Ampères.
pub trait CurrentSensor {
    type Error;
    fn read_amperes(&mut self) -> Result<f32, Self::Error>;
}

/// Capteur de pression retournant des pascal
pub trait PressureSensor {
    type Error;
    fn read_pascal(&mut self) -> Result<f32, Self::Error>;
}

/// Capteur de fermeture (contact sec) retournant un booléen.
pub trait ClosureSensor {
    type Error;
    /// Retourne `true` si la chambre est physiquement fermée.
    fn is_closed(&mut self) -> Result<bool, Self::Error>;
}
