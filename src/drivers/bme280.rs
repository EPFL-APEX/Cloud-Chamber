/// Driver BME280 (température + humidité + pression) via I²C bloquant.
///
/// Code bloquant, Rust stable, pas d'Embassy.
/// Arithmétique entière selon la datasheet Bosch BME280 §4.2.3.
///
/// # Pattern delay
///
/// Le délai est passé explicitement à `start_measurement(delay)`.
/// Pour les traits `Sensor`/`DeferredSensor`, utiliser `Bme280Sensor<I, D, C>`
/// qui stocke le delay et l'horloge en interne.

use embedded_hal::i2c::I2c as I2cTrait;
use embedded_hal::delay::DelayNs;
use crate::cloud_chamber_hal::sensors::{Sensor, DeferredSensor};
use crate::cloud_chamber_hal::measurement::Measurement;
use crate::cloud_chamber_hal::timer::{Duration, MonotonicTimer};
use crate::cloud_chamber_hal::units::{Celsius, HectoPascal};

const BME_ADDR:       u8 = 0x76;

/// Durée d'une mesure forcée (§9.1 de la datasheet, marge incluse).
const BME280_MEASURE_MS: u32 = 15;
const REG_CHIP_ID:    u8 = 0xD0;
const REG_CALIB_T:    u8 = 0x88;
const REG_CALIB_H:    u8 = 0xE1;
const REG_CTRL_HUM:   u8 = 0xF2;
const REG_CTRL_MEAS:  u8 = 0xF4;
const REG_DATA:       u8 = 0xF7;
const CHIP_ID_BME280: u8 = 0x60;

// ════════════════════════════════════════════════════════════════════════════
// Calibration (privée)
// ════════════════════════════════════════════════════════════════════════════

struct BmeCalib {
    // Température
    t1: u16, t2: i16, t3: i16,
    // Pression (P1 non signé, P2-P9 signés — datasheet §4.2.2 tableau 16)
    p1: u16, p2: i16, p3: i16, p4: i16, p5: i16,
    p6: i16, p7: i16, p8: i16, p9: i16,
    // Humidité
    h1: u8,  h2: i16, h3: u8,
    h4: i16, h5: i16, h6: i8,
}

// ════════════════════════════════════════════════════════════════════════════
// Erreur
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug)]
pub enum Bme280Error { I2c, WrongChipId, NotInitialized }

// ════════════════════════════════════════════════════════════════════════════
// Driver
// ════════════════════════════════════════════════════════════════════════════

pub struct Bme280Driver<I> {
    i2c:        I,
    calib:      Option<BmeCalib>,
    last_adc_t: i32,
    last_adc_p: i32,
    last_adc_h: i32,
}

impl<I: I2cTrait> Bme280Driver<I> {
    pub fn new(i2c: I) -> Self {
        Self { i2c, calib: None, last_adc_t: 0, last_adc_p: 0, last_adc_h: 0 }
    }

    fn reg_read(&mut self, reg: u8, buf: &mut [u8]) -> Result<(), Bme280Error> {
        self.i2c.write_read(BME_ADDR, &[reg], buf).map_err(|_| Bme280Error::I2c)
    }

    fn reg_write(&mut self, reg: u8, val: u8) -> Result<(), Bme280Error> {
        self.i2c.write(BME_ADDR, &[reg, val]).map_err(|_| Bme280Error::I2c)
    }

    fn comp_temp(adc_t: i32, c: &BmeCalib) -> (i32, i32) {
        let var1   = (((adc_t >> 3) - ((c.t1 as i32) << 1)) * (c.t2 as i32)) >> 11;
        let tmp    = (adc_t >> 4) - (c.t1 as i32);
        let var2   = ((tmp * tmp) >> 12) * (c.t3 as i32) >> 14;
        let t_fine = var1 + var2;
        ((t_fine * 5 + 128) >> 8, t_fine)
    }

    /// Compensation pression — datasheet §4.2.3 (arithmétique i64, Q24.8 → Pa).
    /// Retourne la pression en hPa.
    fn comp_press(adc_p: i32, t_fine: i32, c: &BmeCalib) -> f32 {
        let mut var1: i64 = (t_fine as i64) - 128_000;
        let mut var2: i64 = var1 * var1 * (c.p6 as i64);
        var2 += (var1 * (c.p5 as i64)) << 17;
        var2 += (c.p4 as i64) << 35;
        var1  = ((var1 * var1 * (c.p3 as i64)) >> 8) + ((var1 * (c.p2 as i64)) << 12);
        var1  = (((1i64 << 47) + var1) * (c.p1 as i64)) >> 33;
        if var1 == 0 { return 0.0; }
        let mut p: i64 = 1_048_576 - (adc_p as i64);
        p = (((p << 31) - var2) * 3125) / var1;
        var1 = ((c.p9 as i64) * (p >> 13) * (p >> 13)) >> 25;
        var2 = ((c.p8 as i64) * p) >> 19;
        p = ((p + var1 + var2) >> 8) + ((c.p7 as i64) << 4);
        // p est en Pa × 256 (format Q24.8) → diviser par 256 pour Pa, par 25600 pour hPa
        (p as u32) as f32 / 25_600.0
    }

    fn comp_hum(adc_h: i32, t_fine: i32, c: &BmeCalib) -> i32 {
        let mut x = (t_fine - 76800) as i64;
        x = ((((adc_h as i64) << 14)
            - ((c.h4 as i64) << 20)
            - (c.h5 as i64) * x + 16384) >> 15)
            * (((((((x * c.h6 as i64) >> 10)
                * (((x * c.h3 as i64) >> 11) + 32768)) >> 10)
                + 2097152) * c.h2 as i64 + 8192) >> 14);
        x -= ((x >> 15) * (x >> 15) >> 7) * c.h1 as i64 >> 4;
        x = x.max(0).min(419_430_400);
        (x >> 12) as i32
    }

    /// Vérifie le chip ID, lit la calibration, configure le capteur.
    pub fn init(&mut self) -> Result<(), Bme280Error> {
        let mut id = [0u8; 1];
        self.reg_read(REG_CHIP_ID, &mut id)?;
        if id[0] != CHIP_ID_BME280 { return Err(Bme280Error::WrongChipId); }

        let mut ct = [0u8; 26];
        let mut ch = [0u8; 7];
        self.reg_read(REG_CALIB_T, &mut ct)?;
        self.reg_read(REG_CALIB_H, &mut ch)?;

        self.calib = Some(BmeCalib {
            // Température (0x88-0x8D)
            t1: u16::from_le_bytes([ct[0], ct[1]]),
            t2: i16::from_le_bytes([ct[2], ct[3]]),
            t3: i16::from_le_bytes([ct[4], ct[5]]),
            // Pression (0x8E-0x9F)
            p1: u16::from_le_bytes([ct[6],  ct[7]]),
            p2: i16::from_le_bytes([ct[8],  ct[9]]),
            p3: i16::from_le_bytes([ct[10], ct[11]]),
            p4: i16::from_le_bytes([ct[12], ct[13]]),
            p5: i16::from_le_bytes([ct[14], ct[15]]),
            p6: i16::from_le_bytes([ct[16], ct[17]]),
            p7: i16::from_le_bytes([ct[18], ct[19]]),
            p8: i16::from_le_bytes([ct[20], ct[21]]),
            p9: i16::from_le_bytes([ct[22], ct[23]]),
            // Humidité (0xA1 + 0xE1-0xE7)
            h1: ct[25],
            h2: i16::from_le_bytes([ch[0], ch[1]]),
            h3: ch[2],
            h4: ((ch[3] as i16) << 4) | ((ch[4] & 0x0F) as i16),
            h5: ((ch[5] as i16) << 4) | ((ch[4] >> 4) as i16),
            h6: ch[6] as i8,
        });

        self.reg_write(REG_CTRL_HUM,  0x01)?; // humidité x1
        // 0x24 = osrs_t=001 (x1), osrs_p=001 (x1), mode=00 (sleep)
        // On démarre en sleep : chaque appel à start_measurement() passe en forced
        // (0x25), fait une mesure, puis le capteur retourne automatiquement en sleep.
        // Évite les mesures continues en tâche de fond entre deux appels à measure().
        self.reg_write(REG_CTRL_MEAS, 0x24)?;
        Ok(())
    }

    /// Mesure forcée complète (~15 ms). Retourne (temp °C, pression hPa, humidité %).
    pub fn measure<D: DelayNs>(&mut self, delay: &mut D) -> Result<(f32, f32, f32), Bme280Error> {
        self.start_measurement(delay)?;
        Ok((self.read_celsius()?, self.read_pressure_hpa()?, self.read_humidity()?))
    }

    /// Déclenche une mesure forcée sans attendre la conversion (~15 ms).
    ///
    /// À combiner avec `fetch_raw()` après le délai de conversion. Séparer les
    /// deux étapes permet de déclencher plusieurs BME280 (adresses I2C
    /// distinctes) avant d'attendre une seule fois, au lieu de bloquer ~15 ms
    /// par capteur.
    pub fn trigger_measurement(&mut self) -> Result<(), Bme280Error> {
        if self.calib.is_none() { return Err(Bme280Error::NotInitialized); }
        self.reg_write(REG_CTRL_HUM,  0x01)?;
        self.reg_write(REG_CTRL_MEAS, 0x25)?; // mode forcé — temp x1, pression x1
        Ok(())
    }

    /// Lit les données brutes après le délai de conversion suivant `trigger_measurement()`.
    pub fn fetch_raw(&mut self) -> Result<(), Bme280Error> {
        let mut raw = [0u8; 8];
        self.reg_read(REG_DATA, &mut raw)?;
        // Registres 0xF7-0xFE : press[2:0], temp[2:0], hum[1:0]
        self.last_adc_p = ((raw[0] as i32) << 12) | ((raw[1] as i32) << 4) | ((raw[2] as i32) >> 4);
        self.last_adc_t = ((raw[3] as i32) << 12) | ((raw[4] as i32) << 4) | ((raw[5] as i32) >> 4);
        self.last_adc_h = ((raw[6] as i32) << 8)  |  (raw[7] as i32);
        Ok(())
    }

    /// Envoie une mesure forcée, attend ~15 ms, stocke les données brutes.
    /// Combine `trigger_measurement()` + `fetch_raw()` pour un usage simple,
    /// mono-capteur, bloquant.
    pub fn start_measurement<D: DelayNs>(&mut self, delay: &mut D) -> Result<(), Bme280Error> {
        self.trigger_measurement()?;
        delay.delay_ms(15);
        self.fetch_raw()
    }

    /// Température en °C depuis la dernière mesure.
    pub fn read_celsius(&mut self) -> Result<f32, Bme280Error> {
        let c = self.calib.as_ref().ok_or(Bme280Error::NotInitialized)?;
        let (t100, _) = Self::comp_temp(self.last_adc_t, c);
        Ok(t100 as f32 / 100.0)
    }

    /// Pression en hPa depuis la dernière mesure.
    pub fn read_pressure_hpa(&mut self) -> Result<f32, Bme280Error> {
        let c = self.calib.as_ref().ok_or(Bme280Error::NotInitialized)?;
        let (_, t_fine) = Self::comp_temp(self.last_adc_t, c);
        Ok(Self::comp_press(self.last_adc_p, t_fine, c))
    }

    /// Humidité en % depuis la dernière mesure.
    pub fn read_humidity(&mut self) -> Result<f32, Bme280Error> {
        let c = self.calib.as_ref().ok_or(Bme280Error::NotInitialized)?;
        let (_, t_fine) = Self::comp_temp(self.last_adc_t, c);
        Ok(Self::comp_hum(self.last_adc_h, t_fine, c) as f32 / 1024.0)
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Wrapper implémentant Sensor/DeferredSensor (delay + horloge stockés en interne)
// ════════════════════════════════════════════════════════════════════════════

pub struct Bme280Sensor<I, D, C> {
    driver: Bme280Driver<I>,
    delay:  D,
    clock:  C,
}

impl<I: I2cTrait, D: DelayNs, C: MonotonicTimer> Bme280Sensor<I, D, C> {
    pub fn new(driver: Bme280Driver<I>, delay: D, clock: C) -> Self { Self { driver, delay, clock } }
    pub fn init(&mut self) -> Result<(), Bme280Error> { self.driver.init() }
}

impl<I: I2cTrait, D: DelayNs, C: MonotonicTimer> Sensor<Measurement<Celsius>> for Bme280Sensor<I, D, C> {
    type Error = Bme280Error;

    fn read(&mut self) -> Result<Measurement<Celsius>, Self::Error> {
        self.start_conversion()?;
        self.delay.delay_ms(self.conversion_time_ms().as_millis() as u32);
        self.read_result()
    }
}

/// Pression **atmosphérique absolue** (~1013 hPa au niveau de la mer).
///
/// À ne pas confondre avec ce que mesure `drivers::abp2` : celui-ci lit la
/// pression d'un circuit de la chambre sur une plage 0–1 bar. Les deux
/// remplissent aujourd'hui le même créneau `SensorSnapshot::press`, mais ne
/// décrivent pas la même grandeur physique — si une sécurité pression est
/// ajoutée un jour, elle devra savoir lequel des deux elle lit.
///
/// Une seule mesure forcée sert les deux grandeurs (le BME280 convertit
/// température, pression et humidité en un cycle) : lire la pression coûte
/// donc le même ~15 ms que lire la température, pas le double.
impl<I: I2cTrait, D: DelayNs, C: MonotonicTimer> Sensor<Measurement<HectoPascal>>
    for Bme280Sensor<I, D, C>
{
    type Error = Bme280Error;

    fn read(&mut self) -> Result<Measurement<HectoPascal>, Self::Error> {
        self.driver.trigger_measurement()?;
        self.delay.delay_ms(BME280_MEASURE_MS);
        self.driver.fetch_raw()?;
        let hpa = self.driver.read_pressure_hpa()?;
        Ok(Measurement::new(self.clock.now(), HectoPascal(hpa)))
    }
}

impl<I: I2cTrait, D: DelayNs, C: MonotonicTimer> DeferredSensor<Measurement<Celsius>> for Bme280Sensor<I, D, C> {
    fn start_conversion(&mut self) -> Result<(), Self::Error> {
        self.driver.trigger_measurement()
    }

    fn conversion_time_ms(&self) -> Duration { Duration::from_millis(BME280_MEASURE_MS as u64) }

    fn read_result(&mut self) -> Result<Measurement<Celsius>, Self::Error> {
        self.driver.fetch_raw()?;
        let value = self.driver.read_celsius()?;
        Ok(Measurement::new(self.clock.now(), Celsius(value)))
    }
}
