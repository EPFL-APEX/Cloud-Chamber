/// Driver Honeywell ABP2 (capteur de pression I2C).
///
/// # Format de trame (4 octets lus sur I2C)
///
/// ```text
/// Octet 0 : [7:6] = status, [5:0] = pression bits [13:8]
/// Octet 1 :                          pression bits  [7:0]
/// Octet 2 : [7:0] = temperature bits [10:3]
/// Octet 3 : [7:5] = temperature bits  [2:0], [4:0] inutilises
/// ```
///
/// # Conversion (Application Note Honeywell AN-1728)
///
/// ```text
/// pression  = (P_max - P_min) * (raw_p - 0x0666) / (0x3999 - 0x0666) + P_min
/// temperature = raw_t * 200.0 / 2047.0 - 50.0   [degres C]
/// ```

use embedded_hal::i2c::I2c as I2cTrait;
use crate::cloud_chamber_hal::sensors::PressureSensor;

// ════════════════════════════════════════════════════════════════════════════
// Constantes ABP2 (datasheet Honeywell)
// ════════════════════════════════════════════════════════════════════════════

/// Valeur de sortie minimale du pont (10 % de 2^14 = 1638)
const OUTPUT_MIN: u16 = 0x0666;
/// Valeur de sortie maximale du pont (90 % de 2^14 = 14746)
const OUTPUT_MAX: u16 = 0x3999;

// ════════════════════════════════════════════════════════════════════════════
// Erreur
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug)]
pub enum Abp2Error {
    /// Erreur de communication I2C.
    I2c,
    /// Le capteur est en mode commande (non utilisé en mode normal).
    CommandMode,
    /// La donnée lue est la même que la précédente (conversion pas encore prête).
    StaleData,
    /// Défaut interne du capteur.
    Fault,
}

// ════════════════════════════════════════════════════════════════════════════
// Structure de lecture
// ════════════════════════════════════════════════════════════════════════════

/// Résultat d'une mesure ABP2.
#[derive(Clone, Copy, Debug)]
pub struct Abp2Reading {
    /// Pression en bar.
    pub pressure_bar: f32,
    /// Température en degrés Celsius (capteur intégré au boîtier).
    pub temperature_c: f32,
}

// ════════════════════════════════════════════════════════════════════════════
// Driver
// ════════════════════════════════════════════════════════════════════════════

/// Driver ABP2 générique. Instancier un `Abp2Driver` par capteur.
///
/// Le bus I2C est possédé par le driver. Utiliser [`Abp2Driver::release`]
/// pour le récupérer si on veut le partager avec un autre périphérique.
pub struct Abp2Driver<I> {
    i2c:   I,
    addr:  u8,
    /// Pression minimale de la plage en bar (ex. 0.0)
    p_min: f32,
    /// Pression maximale de la plage en bar (ex. 1.0 ou 12.0)
    p_max: f32,
}

impl<I: I2cTrait> Abp2Driver<I> {
    /// Crée un nouveau driver.
    ///
    /// - `addr`  : adresse I2C 7 bits (depuis `config::ABP2_BP_ADDR` / `ABP2_HP_ADDR`)
    /// - `p_min` : borne basse de la plage de pression en bar
    /// - `p_max` : borne haute de la plage de pression en bar
    pub fn new(i2c: I, addr: u8, p_min: f32, p_max: f32) -> Self {
        Self { i2c, addr, p_min, p_max }
    }

    /// Récupère le bus I2C (permet de le partager en séquence avec d'autres drivers).
    pub fn release(self) -> I {
        self.i2c
    }

    /// Lit les 4 octets de mesure et retourne la pression et la température.
    ///
    /// Renvoie une erreur si le status indique stale data ou un défaut.
    pub fn read(&mut self) -> Result<Abp2Reading, Abp2Error> {
        let mut buf = [0u8; 4];
        self.i2c.read(self.addr, &mut buf).map_err(|_| Abp2Error::I2c)?;

        // Bits de status — octet 0, bits [7:6]
        match buf[0] >> 6 {
            0b01 => return Err(Abp2Error::CommandMode),
            0b10 => return Err(Abp2Error::StaleData),
            0b11 => return Err(Abp2Error::Fault),
            _    => {} // 0b00 = normal
        }

        // Pression brute : 14 bits = [octet0 bits 5:0] concat [octet1 bits 7:0]
        let raw_p = (((buf[0] & 0x3F) as u16) << 8) | (buf[1] as u16);

        // Température brute : 11 bits = [octet2 bits 7:0] concat [octet3 bits 7:5]
        let raw_t = ((buf[2] as u16) << 3) | ((buf[3] as u16) >> 5);

        // Conversion pression (interpolation linéaire sur la plage de sortie)
        let span   = OUTPUT_MAX as f32 - OUTPUT_MIN as f32; // 13107.0
        let offset = raw_p as f32 - OUTPUT_MIN as f32;
        let pressure_bar = (self.p_max - self.p_min) * offset / span + self.p_min;

        // Conversion température : résolution 0.097 °C, plage -50..+150 °C
        let temperature_c = (raw_t as f32) * 200.0 / 2047.0 - 50.0;

        Ok(Abp2Reading { pressure_bar, temperature_c })
    }

    /// Raccourci : pression en bar uniquement.
    pub fn read_bar(&mut self) -> Result<f32, Abp2Error> {
        Ok(self.read()?.pressure_bar)
    }

    /// Raccourci : température en degrés Celsius uniquement.
    pub fn read_celsius(&mut self) -> Result<f32, Abp2Error> {
        Ok(self.read()?.temperature_c)
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Trait PressureSensor — retourne en Pascal
// ════════════════════════════════════════════════════════════════════════════

impl<I: I2cTrait> PressureSensor for Abp2Driver<I> {
    type Error = Abp2Error;

    /// Lit la pression et la convertit en Pascal (1 bar = 100 000 Pa).
    fn read_pascal(&mut self) -> Result<f32, Self::Error> {
        Ok(self.read()?.pressure_bar * 100_000.0)
    }
}
