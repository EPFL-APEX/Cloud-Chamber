/// Driver Honeywell ABP2 Series pour capteurs de pression I²C.
///
/// Supporte la lecture de pression (24 bits) et de température
/// intégrée (24 bits) via le protocole I²C du ABP2.
///
/// # Protocole
///
/// 1. Envoyer la commande de mesure : 0xAA 0x00 0x00
/// 2. Attendre ~5ms (conversion)
/// 3. Lire 7 octets : status(1) + pression(3) + température(3)
///
/// La fonction de transfert est 10%–90% de 2^24 counts.

use embassy_rp::i2c::{self, I2c, Async};
use embassy_time::{Duration, Timer};
use defmt;
use embedded_hal_async::i2c::I2c as I2cTrait;


use crate::data::PressureReading;

/// Status bits retournés par le capteur (bits 7:6 du premier octet)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Abp2Status {
    /// Données valides
    Normal,
    /// Le capteur est en mode commande (pas de données)
    CommandMode,
    /// Données périmées (pas de nouvelle conversion depuis la dernière lecture)
    StaleData,
    /// Condition de diagnostic (erreur capteur)
    Diagnostic,
}

impl From<u8> for Abp2Status {
    fn from(val: u8) -> Self {
        match (val >> 6) & 0x03 {
            0 => Self::Normal,
            1 => Self::CommandMode,
            2 => Self::StaleData,
            3 => Self::Diagnostic,
            _ => unreachable!(),
        }
    }
}

/// Configuration d'un capteur ABP2.
pub struct Abp2Config {
    /// Adresse I²C du capteur
    pub address: u8,
    /// Pression minimale de la plage (bar)
    pub p_min: f32,
    /// Pression maximale de la plage (bar)
    pub p_max: f32,
    /// Nom du capteur (pour le logging)
    pub label: &'static str,
}

/// Résultat brut d'une lecture ABP2.
pub struct Abp2RawReading {
    pub status: Abp2Status,
    pub pressure: f32,
    pub temperature: f32,
}

/// Lit un capteur ABP2 sur le bus I²C.
///
/// # Arguments
/// * `i2c` — Bus I²C partagé (embassy async)
/// * `config` — Configuration du capteur (adresse, plage)
///
/// # Retourne
/// `Ok(PressureReading)` si la lecture est valide,
/// `Err(())` en cas d'erreur de communication.
pub async fn read_abp2<T: i2c::Instance>(
    i2c: &mut I2c<'_, T, Async>,
    config: &Abp2Config,
) -> Result<PressureReading, ()> {
    // Étape 1 : envoyer la commande de mesure
    let cmd: [u8; 3] = [0xAA, 0x00, 0x00];
    i2c.write(config.address as u16, &cmd).await.map_err(|e| {
        defmt::error!("ABP2 [{}] write error: {}", config.label, defmt::Debug2Format(&e));
    })?;

    // Étape 2 : attendre la conversion (~5ms)
    Timer::after(Duration::from_millis(5)).await;

    // Étape 3 : lire 7 octets
    let mut buf = [0u8; 7];
    i2c.read(config.address as u16, &mut buf).await.map_err(|e| {
        defmt::error!("ABP2 [{}] read error: {}", config.label, defmt::Debug2Format(&e));
    })?;

    // Parser le status
    let status = Abp2Status::from(buf[0]);
    if status == Abp2Status::Diagnostic {
        defmt::warn!("ABP2 [{}] diagnostic condition", config.label);
        return Err(());
    }

    // Parser la pression (24 bits)
    // Les 6 bits de poids faible de buf[0] + buf[1] + buf[2]
    let raw_pressure: u32 =
        ((buf[0] as u32 & 0x3F) << 16) | ((buf[1] as u32) << 8) | (buf[2] as u32);

    // Fonction de transfert : 10% à 90% de 2^24
    const OUTPUT_MAX: f32 = 0.9 * 16_777_216.0; // 90% of 2^24
    const OUTPUT_MIN: f32 = 0.1 * 16_777_216.0; // 10% of 2^24

    let pressure = (raw_pressure as f32 - OUTPUT_MIN) / (OUTPUT_MAX - OUTPUT_MIN)
        * (config.p_max - config.p_min)
        + config.p_min;

    // Parser la température (24 bits)
    let raw_temp: u32 =
        ((buf[3] as u32) << 16) | ((buf[4] as u32) << 8) | (buf[5] as u32);

    // T = raw / 2^24 * 200 - 50
    let temperature = (raw_temp as f32) / 16_777_216.0 * 200.0 - 50.0;

    Ok(PressureReading {
        pressure,
        temperature,
        valid: status == Abp2Status::Normal,
    })
}
