/// Driver DS18B20 via protocole 1-Wire implémenté directement (sans crate externe).
///
/// Timing calibré pour RP2040 @ 125 MHz avec TimerDelay :
///   Slot lecture  : bas 2µs → relâche → attend 8µs → sample à ~10µs (< 15µs max)
///   Slot écriture 1 : bas 6µs, haut 64µs
///   Slot écriture 0 : bas 60µs, haut 10µs
///   Reset : bas 480µs → relâche → attend 70µs → sample → attend 410µs
///
/// Compatibilité clones : si SEARCH ROM échoue (clone sans ROM search),
/// repli automatique sur SKIP ROM pour bus mono-capteur.

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use heapless::Vec;

use crate::cloud_chamber_hal::sensors::TemperatureSensor;
use crate::config::CRITICAL_TEMP_INDICES;
use crate::shared::data::TemperatureReading;

const DS18B20_FAMILY:    u8 = 0x28;
const CMD_SEARCH_ROM:    u8 = 0xF0;
const CMD_MATCH_ROM:     u8 = 0x55;
const CMD_SKIP_ROM:      u8 = 0xCC;
const CMD_CONVERT_T:     u8 = 0x44;
const CMD_READ_SCRATCH:  u8 = 0xBE;
const CMD_WRITE_SCRATCH: u8 = 0x4E;

// ════════════════════════════════════════════════════════════════════════════
// Résolution
// ════════════════════════════════════════════════════════════════════════════

/// Résolution de conversion du DS18B20.
///
/// Correspond aux bits R1:R0 du registre de configuration (octet 4 du scratchpad).
/// Plus la résolution est élevée, plus le temps de conversion est long.
///
/// Sécurité : après `set_resolution()`, attendre au minimum `conversion_time_ms()`
/// avant d'appeler `read_celsius()`, sinon le scratchpad contiendra la mesure
/// précédente (ou une valeur indéfinie au premier démarrage).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Resolution {
    Bits9,        // 0.5 °C    — 93.75 ms max
    Bits10,       // 0.25 °C   — 187.5 ms max
    Bits11,       // 0.125 °C  — 375 ms   max
    #[default]
    Bits12,       // 0.0625 °C — 750 ms   max (valeur usine)
}

impl Resolution {
    /// Valeur à écrire dans le registre de configuration du scratchpad.
    pub fn config_byte(self) -> u8 {
        match self {
            Self::Bits9  => 0x1F,
            Self::Bits10 => 0x3F,
            Self::Bits11 => 0x5F,
            Self::Bits12 => 0x7F,
        }
    }

    /// Délai de conversion à respecter après `start_conversion()`, en millisecondes.
    ///
    /// Valeurs datasheet + 50 ms de marge pour les clones et les pull-up lents.
    pub fn conversion_time_ms(self) -> u32 {
        match self {
            Self::Bits9  => 150,
            Self::Bits10 => 240,
            Self::Bits11 => 430,
            Self::Bits12 => 800,
        }
    }
}

pub const MAX_SENSORS: usize = 5;

type RomCode = [u8; 8];

/// Code sentinel indiquant le mode SKIP ROM (clone sans ROM search).
/// Serial bytes tous à 0 → jamais un vrai ROM code (CRC invalide).
const SKIP_ROM_SENTINEL: RomCode = [DS18B20_FAMILY, 0, 0, 0, 0, 0, 0, 0];

// ════════════════════════════════════════════════════════════════════════════
// Erreur
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug)]
pub enum Ds18b20Error { Bus, NoSensor, CrcError }

// ════════════════════════════════════════════════════════════════════════════
// Primitives 1-Wire
// ════════════════════════════════════════════════════════════════════════════

fn ow_reset<P, D>(pin: &mut P, delay: &mut D) -> bool
where P: InputPin + OutputPin, D: DelayNs,
{
    pin.set_high().ok();
    delay.delay_us(5);
    pin.set_low().ok();
    delay.delay_us(480);
    pin.set_high().ok();
    delay.delay_us(70);
    let presence = pin.is_low().unwrap_or(false);
    delay.delay_us(410);
    presence
}

fn ow_write_bit<P, D>(pin: &mut P, delay: &mut D, bit: bool)
where P: OutputPin, D: DelayNs,
{
    pin.set_low().ok();
    if bit {
        delay.delay_us(6);
        pin.set_high().ok();
        delay.delay_us(64);
    } else {
        delay.delay_us(60);
        pin.set_high().ok();
        delay.delay_us(10);
    }
}

/// Reset prolongé (800µs) pour forcer les clones à sortir d'un état bloqué
/// (ex. après une séquence SEARCH ROM incomplète).
fn ow_reset_long<P, D>(pin: &mut P, delay: &mut D)
where P: InputPin + OutputPin, D: DelayNs,
{
    pin.set_high().ok();
    delay.delay_us(5);
    pin.set_low().ok();
    delay.delay_us(800); // 800µs au lieu de 480µs → force la sortie de tout état interne
    pin.set_high().ok();
    delay.delay_us(500); // Attente de récupération allongée
}

/// Sample à ~10µs depuis le début du slot.
/// Le DS18B20 tient la ligne basse MAX 15µs pour un bit '0' → on est dans la fenêtre.
fn ow_read_bit<P, D>(pin: &mut P, delay: &mut D) -> bool
where P: InputPin + OutputPin, D: DelayNs,
{
    pin.set_low().ok();
    delay.delay_us(2);
    pin.set_high().ok();
    delay.delay_us(8);
    let bit = pin.is_high().unwrap_or(true);
    delay.delay_us(50);
    bit
}

fn ow_write_byte<P, D>(pin: &mut P, delay: &mut D, byte: u8)
where P: OutputPin, D: DelayNs,
{
    for i in 0..8 { ow_write_bit(pin, delay, (byte >> i) & 1 != 0); }
}

fn ow_read_byte<P, D>(pin: &mut P, delay: &mut D) -> u8
where P: InputPin + OutputPin, D: DelayNs,
{
    let mut byte = 0u8;
    for i in 0..8 { if ow_read_bit(pin, delay) { byte |= 1 << i; } }
    byte
}

/// CRC-8 Dallas/Maxim (polynôme inversé 0x8C).
/// Sur N octets dont le dernier est le CRC → résultat 0 si valide.
fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for &b in data {
        let mut byte = b;
        for _ in 0..8 {
            let mix = (crc ^ byte) & 1;
            crc >>= 1;
            if mix != 0 { crc ^= 0x8C; }
            byte >>= 1;
        }
    }
    crc
}

// ════════════════════════════════════════════════════════════════════════════
// ROM Search — Dallas Application Note 187
// ════════════════════════════════════════════════════════════════════════════

fn search_step<P, D>(
    pin:               &mut P,
    delay:             &mut D,
    rom:               &mut RomCode,
    last_discrepancy:  &mut u8,
    last_device_flag:  &mut bool,
) -> bool
where P: InputPin + OutputPin, D: DelayNs,
{
    if *last_device_flag { return false; }
    if !ow_reset(pin, delay) {
        *last_discrepancy = 0;
        *last_device_flag = false;
        return false;
    }

    ow_write_byte(pin, delay, CMD_SEARCH_ROM);

    let mut last_zero:       u8    = 0;
    let mut rom_byte_number: usize = 0;
    let mut rom_byte_mask:   u8    = 1;
    let mut id_bit_number:   u8    = 1;
    let mut ok = true;

    while id_bit_number <= 64 {
        let id_bit  = ow_read_bit(pin, delay);
        let cmp_bit = ow_read_bit(pin, delay);

        if id_bit && cmp_bit { ok = false; break; }

        let dir = if !id_bit && !cmp_bit {
            let d = if id_bit_number < *last_discrepancy {
                rom[rom_byte_number] & rom_byte_mask != 0
            } else {
                id_bit_number == *last_discrepancy
            };
            if !d { last_zero = id_bit_number; }
            d
        } else {
            id_bit
        };

        if dir { rom[rom_byte_number] |=  rom_byte_mask; }
        else   { rom[rom_byte_number] &= !rom_byte_mask; }

        ow_write_bit(pin, delay, dir);

        id_bit_number  += 1;
        rom_byte_mask   = rom_byte_mask.wrapping_shl(1);
        if rom_byte_mask == 0 { rom_byte_mask = 1; rom_byte_number += 1; }
    }

    if !ok || id_bit_number != 65 || crc8(rom) != 0 {
        *last_discrepancy = 0;
        *last_device_flag = false;
        return false;
    }

    *last_discrepancy = last_zero;
    if last_zero == 0 { *last_device_flag = true; }
    true
}

// ════════════════════════════════════════════════════════════════════════════
// Bus multi-capteurs
// ════════════════════════════════════════════════════════════════════════════

pub struct Ds18b20Bus<P> {
    pin:     P,
    sensors: Vec<RomCode, MAX_SENSORS>,
}

impl<P: InputPin + OutputPin> Ds18b20Bus<P> {
    pub fn new(pin: P) -> Self {
        Self { pin, sensors: Vec::new() }
    }

    /// Recherche tous les DS18B20 sur le bus.
    ///
    /// Essaie d'abord SEARCH ROM (DS18B20 authentique).
    /// Si aucun trouvé mais présence détectée → mode SKIP ROM (clone/contrefaçon)
    /// qui enregistre un capteur virtuel et utilise SKIP ROM pour toutes les opérations.
    pub fn discover<D: DelayNs>(&mut self, delay: &mut D) -> usize {
        self.sensors.clear();

        // ── Tentative 1 : SEARCH ROM ──────────────────────────────────────────
        {
            let mut last_discrepancy: u8   = 0;
            let mut last_device_flag: bool = false;
            let mut rom = [0u8; 8];

            loop {
                if !search_step(
                    &mut self.pin, delay,
                    &mut rom,
                    &mut last_discrepancy,
                    &mut last_device_flag,
                ) { break; }
                if rom[0] == DS18B20_FAMILY {
                    let _ = self.sensors.push(rom);
                }
                if self.sensors.is_full() { break; }
            }
        }

        // ── Repli : SKIP ROM si SEARCH ROM a échoué ────────────────────────────
        // Certains clones DS18B20 entrent dans un état bloqué après avoir reçu
        // la commande SEARCH ROM (0xF0) — ils continuent à ne répondre qu'à la
        // présence mais ignorent les commandes suivantes.
        // Remède : reset prolongé 800µs pour forcer la réinitialisation interne,
        // puis reset standard pour vérifier la présence dans un état propre.
        if self.sensors.is_empty() {
            ow_reset_long(&mut self.pin, delay); // force-reset du clone bloqué
            if ow_reset(&mut self.pin, delay) {  // reset standard → présence propre
                let _ = self.sensors.push(SKIP_ROM_SENTINEL);
            }
        }

        self.sensors.len()
    }

    /// Envoie Convert T au capteur `index` (sans attente de conversion).
    pub fn start_conversion<D: DelayNs>(
        &mut self, index: usize, delay: &mut D,
    ) -> Result<(), Ds18b20Error> {
        let rom = *self.sensors.get(index).ok_or(Ds18b20Error::NoSensor)?;
        if !ow_reset(&mut self.pin, delay) { return Err(Ds18b20Error::Bus); }
        self.send_address(&rom, delay);
        ow_write_byte(&mut self.pin, delay, CMD_CONVERT_T);
        Ok(())
    }

    /// Lit la température en °C du capteur `index`.
    pub fn read_celsius<D: DelayNs>(
        &mut self, index: usize, delay: &mut D,
    ) -> Result<f32, Ds18b20Error> {
        let rom = *self.sensors.get(index).ok_or(Ds18b20Error::NoSensor)?;
        if !ow_reset(&mut self.pin, delay) { return Err(Ds18b20Error::Bus); }
        self.send_address(&rom, delay);
        ow_write_byte(&mut self.pin, delay, CMD_READ_SCRATCH);
        let mut sp = [0u8; 9];
        for b in sp.iter_mut() { *b = ow_read_byte(&mut self.pin, delay); }
        if crc8(&sp) != 0 { return Err(Ds18b20Error::CrcError); }
        let raw = (sp[0] as u16) | ((sp[1] as u16) << 8);
        Ok(raw as i16 as f32 / 16.0)
    }

    /// Adressage : SKIP ROM pour les clones (sentinel), MATCH ROM + ROM pour les vrais.
    fn send_address<D: DelayNs>(&mut self, rom: &RomCode, delay: &mut D) {
        if rom == &SKIP_ROM_SENTINEL {
            ow_write_byte(&mut self.pin, delay, CMD_SKIP_ROM);
        } else {
            ow_write_byte(&mut self.pin, delay, CMD_MATCH_ROM);
            for &b in rom { ow_write_byte(&mut self.pin, delay, b); }
        }
    }

    pub fn sensor_count(&self) -> usize { self.sensors.len() }

    /// Configure la résolution d'un capteur via la commande WriteScratchpad (0x4E).
    ///
    /// Les alarmes TH/TL sont mises à zéro (désactivées).
    /// La nouvelle résolution prend effet dès la prochaine conversion.
    pub fn set_resolution<D: DelayNs>(
        &mut self, index: usize, delay: &mut D, resolution: Resolution,
    ) -> Result<(), Ds18b20Error> {
        let rom = *self.sensors.get(index).ok_or(Ds18b20Error::NoSensor)?;
        if !ow_reset(&mut self.pin, delay) { return Err(Ds18b20Error::Bus); }
        self.send_address(&rom, delay);
        ow_write_byte(&mut self.pin, delay, CMD_WRITE_SCRATCH);
        ow_write_byte(&mut self.pin, delay, 0x00); // TH alarm désactivé
        ow_write_byte(&mut self.pin, delay, 0x00); // TL alarm désactivé
        ow_write_byte(&mut self.pin, delay, resolution.config_byte());
        Ok(())
    }

    /// Lecture bloquante de tous les capteurs.
    ///
    /// `resolution` doit correspondre à celle configurée sur les capteurs ;
    /// elle détermine le délai d'attente après Convert T.
    pub fn read_all<D: DelayNs>(
        &mut self, delay: &mut D, resolution: Resolution,
    ) -> [TemperatureReading; MAX_SENSORS] {
        let mut readings = [TemperatureReading::default(); MAX_SENSORS];
        for idx in 0..self.sensors.len() {
            let is_critical = CRITICAL_TEMP_INDICES.contains(&idx);
            if self.start_conversion(idx, delay).is_ok() {
                delay.delay_ms(resolution.conversion_time_ms());
                readings[idx] = match self.read_celsius(idx, delay) {
                    Ok(t)  => TemperatureReading { value: t,        valid: true,  critical: is_critical },
                    Err(_) => TemperatureReading { value: f32::NAN, valid: false, critical: is_critical },
                };
            }
        }
        readings
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Wrapper single-capteur implémentant TemperatureSensor
// ════════════════════════════════════════════════════════════════════════════

pub struct Ds18b20Sensor<P, D> {
    bus:        Ds18b20Bus<P>,
    delay:      D,
    index:      usize,
    resolution: Resolution,
}

impl<P: InputPin + OutputPin, D: DelayNs> Ds18b20Sensor<P, D> {
    /// Crée un wrapper single-capteur et configure immédiatement la résolution
    /// via WriteScratchpad (envoi unique au capteur).
    ///
    /// Après construction, `start_measurement()` utilise le délai correspondant
    /// sans jamais renvoyer WriteScratchpad.
    pub fn new(
        mut bus: Ds18b20Bus<P>, mut delay: D, index: usize, resolution: Resolution,
    ) -> Result<Self, Ds18b20Error> {
        bus.set_resolution(index, &mut delay, resolution)?;
        Ok(Self { bus, delay, index, resolution })
    }

    /// Reconfigure la résolution et l'envoie au capteur via WriteScratchpad.
    /// À appeler uniquement lorsqu'on veut changer de résolution en cours d'utilisation.
    pub fn set_resolution(&mut self, resolution: Resolution) -> Result<(), Ds18b20Error> {
        self.bus.set_resolution(self.index, &mut self.delay, resolution)?;
        self.resolution = resolution;
        Ok(())
    }
}

impl<P: InputPin + OutputPin, D: DelayNs> TemperatureSensor for Ds18b20Sensor<P, D> {
    type Error = Ds18b20Error;

    /// Déclenche la conversion et attend le temps correspondant à la résolution
    /// configurée. Cohérent avec `Bme280Sensor` qui bloque aussi pendant la mesure.
    fn start_measurement(&mut self) -> Result<(), Self::Error> {
        self.bus.start_conversion(self.index, &mut self.delay)?;
        self.delay.delay_ms(self.resolution.conversion_time_ms());
        Ok(())
    }

    fn read_celsius(&mut self) -> Result<f32, Self::Error> {
        self.bus.read_celsius(self.index, &mut self.delay)
    }
}