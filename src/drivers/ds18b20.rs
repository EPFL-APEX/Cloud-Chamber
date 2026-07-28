//! Driver DS18B20 via protocole 1-Wire implémenté directement (sans crate externe).
//!
//! # Historique de convergence
//!
//! Deux implémentations coexistaient :
//! - une version RP2040-only à accès registre SIO direct, seule utilisée en
//!   production car elle respecte l'open-drain strict et tient la fenêtre de
//!   lecture (~10µs) sur les clones DS18B20 testés sur matériel ;
//! - une version générique `embedded-hal` (`InputPin`/`OutputPin`), plus
//!   portable mais qui pousse activement un niveau haut via `set_high()`,
//!   ce qui n'est PAS de l'open-drain à moins que la broche soit déjà
//!   configurée comme telle en amont (et qui ne tenait pas la fenêtre de
//!   lecture sur les clones du montage).
//!
//! Ce fichier fusionne les deux : la structure (Resolution, erreurs typées,
//! SKIP ROM fallback, ow_reset_long, BatchSensor/DeferredBatchSensor) vient
//! de la version générique. Le contact avec le bus passe par [`OpenDrainPin`],
//! un trait maison qui ne modélise que "tirer bas" / "relâcher" — jamais
//! "pousser haut" — implémenté en accès registre direct pour RP2040 et
//! RP2350 (cfg `rp2040`/`rp2350` posés par `build.rs`).
//!
//! Timing calibré pour RP2040/RP2350 avec DelayNs :
//!   Slot lecture    : bas 2µs → relâche → attend 8µs → sample à ~10µs (< 15µs max)
//!   Slot écriture 1 : bas 6µs, haut 64µs
//!   Slot écriture 0 : bas 60µs, haut 10µs
//!   Reset           : bas 480µs → relâche → attend 70µs → sample → attend 410µs
//!   Reset prolongé  : bas 800µs (récupération des clones bloqués après un
//!                     SEARCH ROM incomplet)
//!
//! Compatibilité clones : si SEARCH ROM échoue mais qu'un capteur répond au
//! reset, repli automatique sur SKIP ROM pour bus mono-capteur.

use embedded_hal::delay::DelayNs;
use heapless::Vec;

use crate::cloud_chamber_hal::sensors::{BatchSensor, DeferredBatchSensor};
use crate::cloud_chamber_hal::measurement::Measurement;
use crate::cloud_chamber_hal::timer::{Duration, MonotonicTimer};
use crate::cloud_chamber_hal::units::Celsius;
use crate::cloud_chamber_hal::config::NUMBER_OF_TEMP_SENSOR;

const DS18B20_FAMILY:    u8 = 0x28;
const CMD_SEARCH_ROM:    u8 = 0xF0;
const CMD_MATCH_ROM:     u8 = 0x55;
const CMD_SKIP_ROM:      u8 = 0xCC;
const CMD_CONVERT_T:     u8 = 0x44;
const CMD_READ_SCRATCH:  u8 = 0xBE;
const CMD_WRITE_SCRATCH: u8 = 0x4E;

// ════════════════════════════════════════════════════════════════════════════
// Abstraction open-drain — le cœur de la fusion
// ════════════════════════════════════════════════════════════════════════════

/// Abstraction minimale d'une ligne 1-Wire en open-drain strict.
///
/// Contrairement à `embedded_hal::digital::OutputPin`, cette trait ne pousse
/// JAMAIS activement un niveau haut : [`release`](OpenDrainPin::release) met
/// la broche en haute impédance et laisse la résistance de tirage externe
/// (pull-up ~4.7 kΩ) remonter la ligne. C'est le contrat que le 1-Wire exige :
/// un maître qui forcerait le niveau haut provoquerait des collisions bus
/// dès qu'un esclave (ou un autre maître) tire la ligne à 0 en même temps.
pub trait OpenDrainPin {
    /// Active la sortie, niveau figé à 0 : tire la ligne bas.
    fn drive_low(&mut self);
    /// Désactive la sortie (haute impédance / entrée) : relâche la ligne.
    fn release(&mut self);
    /// État actuel de la ligne, lu quelle que soit la direction courante.
    fn is_low(&self) -> bool;
}

// ── Adaptateur RP2040 ──────────────────────────────────────────────────────
// target_arch = "arm" en plus de rp2040 : le cfg rp2040 seul est posé par
// build.rs indépendamment de la cible (utile pour memory.x/linker), donc actif
// même sur `cargo check --target x86_64-...` — sans cette garde, ce module
// tente d'importer rp2040_hal sur desktop et casse cargo test-host. Même
// convention que cloud_chamber_hal::timer::MonotonicTimer.
#[cfg(all(rp2040, target_arch = "arm"))]
pub mod rp2040_adapter {
    use super::OpenDrainPin;
    use rp2040_hal::pac;

    /// Broche 1-Wire pilotée par accès direct au registre `gpio_oe` du SIO.
    ///
    /// `mask` est le masque de bit du GPIO (ex. `1 << 15` pour GP15). Le GPIO
    /// doit avoir été configuré au préalable en sortie niveau bas puis remis
    /// en haute impédance (séquence d'init côté appelant), exactement comme
    /// pour un usage direct de `pac::SIO`.
    pub struct Rp2040OpenDrain {
        mask: u32,
    }

    impl Rp2040OpenDrain {
        pub fn new(mask: u32) -> Self {
            Self { mask }
        }
    }

    impl OpenDrainPin for Rp2040OpenDrain {
        fn drive_low(&mut self) {
            unsafe {
                (*pac::SIO::ptr()).gpio_oe_set().write(|w| w.bits(self.mask));
            }
        }
        fn release(&mut self) {
            unsafe {
                (*pac::SIO::ptr()).gpio_oe_clr().write(|w| w.bits(self.mask));
            }
        }
        fn is_low(&self) -> bool {
            unsafe { (*pac::SIO::ptr()).gpio_in().read().bits() & self.mask == 0 }
        }
    }
}

// ── Adaptateur RP2350 ──────────────────────────────────────────────────────
#[cfg(all(rp2350, any(target_arch = "arm", target_arch = "riscv32")))]
pub mod rp2350_adapter {
    use super::OpenDrainPin;
    use rp235x_hal::pac;

    /// Équivalent RP2350 de [`super::rp2040_adapter::Rp2040OpenDrain`].
    ///
    /// Le bloc SIO du RP2350 conserve les mêmes champs `gpio_oe_set` /
    /// `gpio_oe_clr` / `gpio_in` que le RP2040 pour le premier banc de GPIO ;
    /// à réviser si un GPIO du banc étendu QSPI est utilisé un jour ici.
    pub struct Rp2350OpenDrain {
        mask: u32,
    }

    impl Rp2350OpenDrain {
        pub fn new(mask: u32) -> Self {
            Self { mask }
        }
    }

    impl OpenDrainPin for Rp2350OpenDrain {
        fn drive_low(&mut self) {
            unsafe {
                (*pac::SIO::ptr()).gpio_oe_set().write(|w| w.bits(self.mask));
            }
        }
        fn release(&mut self) {
            unsafe {
                (*pac::SIO::ptr()).gpio_oe_clr().write(|w| w.bits(self.mask));
            }
        }
        fn is_low(&self) -> bool {
            unsafe { (*pac::SIO::ptr()).gpio_in().read().bits() & self.mask == 0 }
        }
    }
}

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
    pub fn conversion_time_ms(self) -> Duration {
        match self {
            Self::Bits9  => Duration::new(150),
            Self::Bits10 => Duration::new(240),
            Self::Bits11 => Duration::new(430),
            Self::Bits12 => Duration::new(800),
        }
    }
}

type RomCode = [u8; 8];

/// Code sentinel indiquant le mode SKIP ROM (clone sans ROM search).
/// Serial bytes tous à 0 → jamais un vrai ROM code (CRC invalide).
const SKIP_ROM_SENTINEL: RomCode = [DS18B20_FAMILY, 0, 0, 0, 0, 0, 0, 0];

/// Valeur brute du registre de température à la mise sous tension (85.00 °C),
/// avant toute conversion — datasheet DS18B20 §"Power-up state".
const POWER_ON_RESET_RAW: u16 = 0x0550;

// ════════════════════════════════════════════════════════════════════════════
// Erreur
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ds18b20Error {
    /// Aucune impulsion de présence après un reset (`ow_reset`) : bus déconnecté,
    /// pull-up 4.7 kΩ absente, court-circuit, ou plus aucun capteur câblé.
    /// Peut survenir sur un bus qui répondait auparavant si un capteur est
    /// débranché en cours de fonctionnement.
    Bus,
    /// Index demandé au-delà du nombre de capteurs découverts par `discover()`
    /// (ou capteur disparu du bus depuis la dernière découverte).
    NoSensor,
    /// CRC-8 du scratchpad invalide (octet 8) : donnée corrompue par du bruit
    /// électrique, une violation de timing sur le bus, ou un capteur retiré en
    /// plein milieu de la lecture des 9 octets.
    CrcError,
    /// Température lue égale à 85.00 °C (raw `0x0550`), la valeur de reset à la
    /// mise sous tension du DS18B20 (datasheet §"Power-up state"). Le CRC est
    /// valide, mais cette valeur signifie qu'aucune conversion n'a encore
    /// abouti sur ce capteur — `Convert T` n'a jamais été envoyé avec succès,
    /// ou le scratchpad a été lu avant la fin du délai de conversion suivant
    /// une mise sous tension. À distinguer d'une véritable mesure à 85 °C
    /// (improbable dans le contexte de cette chambre à nuages).
    PowerOnReset,
}

// ════════════════════════════════════════════════════════════════════════════
// Primitives 1-Wire — génériques sur OpenDrainPin, plus sur InputPin+OutputPin
// ════════════════════════════════════════════════════════════════════════════

fn ow_reset<P, D>(pin: &mut P, delay: &mut D) -> bool
where P: OpenDrainPin, D: DelayNs,
{
    pin.release();
    delay.delay_us(5);
    pin.drive_low();
    delay.delay_us(480);
    pin.release();
    delay.delay_us(70);
    let presence = pin.is_low();
    delay.delay_us(410);
    presence
}

fn ow_write_bit<P, D>(pin: &mut P, delay: &mut D, bit: bool)
where P: OpenDrainPin, D: DelayNs,
{
    pin.drive_low();
    if bit {
        delay.delay_us(6);
        pin.release();
        delay.delay_us(64);
    } else {
        delay.delay_us(60);
        pin.release();
        delay.delay_us(10);
    }
}

/// Reset prolongé (800µs) pour forcer les clones à sortir d'un état bloqué
/// (ex. après une séquence SEARCH ROM incomplète).
fn ow_reset_long<P, D>(pin: &mut P, delay: &mut D)
where P: OpenDrainPin, D: DelayNs,
{
    pin.release();
    delay.delay_us(5);
    pin.drive_low();
    delay.delay_us(800); // 800µs au lieu de 480µs → force la sortie de tout état interne
    pin.release();
    delay.delay_us(500); // Attente de récupération allongée
}

/// Sample à ~10µs depuis le début du slot.
/// Le DS18B20 tient la ligne basse MAX 15µs pour un bit '0' → on est dans la fenêtre.
fn ow_read_bit<P, D>(pin: &mut P, delay: &mut D) -> bool
where P: OpenDrainPin, D: DelayNs,
{
    pin.drive_low();
    delay.delay_us(2);
    pin.release();
    delay.delay_us(8);
    let bit = !pin.is_low();
    delay.delay_us(50);
    bit
}

fn ow_write_byte<P, D>(pin: &mut P, delay: &mut D, byte: u8)
where P: OpenDrainPin, D: DelayNs,
{
    for i in 0..8 { ow_write_bit(pin, delay, (byte >> i) & 1 != 0); }
}

fn ow_read_byte<P, D>(pin: &mut P, delay: &mut D) -> u8
where P: OpenDrainPin, D: DelayNs,
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
where P: OpenDrainPin, D: DelayNs,
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
    sensors: Vec<RomCode, NUMBER_OF_TEMP_SENSOR>,
}

impl<P: OpenDrainPin> Ds18b20Bus<P> {
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

    /// Envoie Convert T à tous les capteurs du bus simultanément (Skip ROM + Convert T).
    ///
    /// Chaque DS18B20 a son propre ADC interne : la conversion se déroule à
    /// l'intérieur de chaque puce, indépendamment des autres. Diffuser Convert T
    /// une seule fois permet donc à tous les capteurs de convertir en parallèle,
    /// au lieu d'attendre le temps de conversion une fois par capteur.
    pub fn start_conversion_broadcast<D: DelayNs>(&mut self, delay: &mut D) -> Result<(), Ds18b20Error> {
        if !ow_reset(&mut self.pin, delay) { return Err(Ds18b20Error::Bus); }
        ow_write_byte(&mut self.pin, delay, CMD_SKIP_ROM);
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
        if raw == POWER_ON_RESET_RAW { return Err(Ds18b20Error::PowerOnReset); }
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
}

// ════════════════════════════════════════════════════════════════════════════
// Wrapper implémentant BatchSensor/DeferredBatchSensor pour tout le bus
// ════════════════════════════════════════════════════════════════════════════

/// Enveloppe `Ds18b20Bus` avec son délai et son horloge, pour implémenter
/// `BatchSensor<Celsius, N>` : une lecture démarre la conversion sur tous les
/// capteurs découverts en une seule diffusion, attend une seule fois, puis
/// lit chaque capteur individuellement.
pub struct Ds18b20Sensors<P, D, C> {
    bus:        Ds18b20Bus<P>,
    delay:      D,
    clock:      C,
    resolution: Resolution,
}

impl<P: OpenDrainPin, D: DelayNs, C: MonotonicTimer> Ds18b20Sensors<P, D, C> {
    /// Crée le wrapper et configure la résolution de tous les capteurs déjà
    /// découverts via `Ds18b20Bus::discover()`.
    pub fn new(
        mut bus: Ds18b20Bus<P>, mut delay: D, clock: C, resolution: Resolution,
    ) -> Result<Self, Ds18b20Error> {
        for index in 0..bus.sensor_count() {
            bus.set_resolution(index, &mut delay, resolution)?;
        }
        Ok(Self { bus, delay, clock, resolution })
    }

    /// Reconfigure la résolution de tous les capteurs et l'envoie via WriteScratchpad.
    pub fn set_resolution(&mut self, resolution: Resolution) -> Result<(), Ds18b20Error> {
        for index in 0..self.bus.sensor_count() {
            self.bus.set_resolution(index, &mut self.delay, resolution)?;
        }
        self.resolution = resolution;
        Ok(())
    }
}

impl<P: OpenDrainPin, D: DelayNs, C: MonotonicTimer>
BatchSensor<Celsius, NUMBER_OF_TEMP_SENSOR> for Ds18b20Sensors<P, D, C>
{
    type Error = Ds18b20Error;

    fn read(&mut self) -> [Result<Measurement<Celsius>, Self::Error>; NUMBER_OF_TEMP_SENSOR] {
        if let Err(e) = self.start_conversion() {
            return core::array::from_fn(|_| Err(e));
        }
        self.delay.delay_ms(self.resolution.conversion_time_ms().as_millis());
        self.read_result()
    }
}

impl<P: OpenDrainPin, D: DelayNs, C: MonotonicTimer>
DeferredBatchSensor<Celsius, NUMBER_OF_TEMP_SENSOR> for Ds18b20Sensors<P, D, C>
{
    fn start_conversion(&mut self) -> Result<(), Self::Error> {
        self.bus.start_conversion_broadcast(&mut self.delay)
    }

    fn conversion_time_ms(&self) -> Duration {
        self.resolution.conversion_time_ms()
    }

    fn read_result(&mut self) -> [Result<Measurement<Celsius>, Self::Error>; NUMBER_OF_TEMP_SENSOR] {
        let count = self.bus.sensor_count();
        core::array::from_fn(|i| {
            if i >= count { return Err(Ds18b20Error::NoSensor); }
            let value = self.bus.read_celsius(i, &mut self.delay)?;
            Ok(Measurement::new(self.clock.get_counter_us(), Celsius(value)))
        })
    }
}
