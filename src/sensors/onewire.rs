//! Bus 1-Wire — implémentation par accès direct aux registres SIO du RP2040.
//!
//! Déplacé depuis `src/bin/main.rs` suite à la review PR #20 (le main ne doit
//! porter que la logique globale).
//!
//! # Pourquoi cette implémentation et pas celle de `sensors::ds18b20`
//!
//! Le crate contient deux jeux de primitives 1-Wire. Celui-ci est le seul
//! utilisé par le firmware de production, pour deux raisons mesurées sur
//! matériel :
//!
//! 1. **Open-drain strict.** Le 1-Wire est un bus à collecteur ouvert : le
//!    maître ne doit JAMAIS forcer la ligne à l'état haut. Ici on ne touche
//!    qu'au registre `gpio_oe` (output enable) : « tirer bas » = activer la
//!    sortie (dont le niveau est figé à 0), « relâcher » = haute impédance,
//!    et c'est la résistance de tirage externe qui remonte la ligne. Le
//!    driver générique passe par `set_high()` / `set_low()`, ce qui exige un
//!    wrapper émulant l'open-drain côté appelant.
//! 2. **Fenêtre de lecture.** Le DS18B20 ne tient la ligne basse que 15 µs
//!    max pour un bit à 0, et on échantillonne à ~10 µs : la marge est de
//!    quelques microsecondes. L'accès registre direct tient cette fenêtre ;
//!    la version générique, avec sa couche de traits, ne la tenait pas sur
//!    les clones utilisés dans le montage (constat documenté dans
//!    `src/bin/test_capteurs.rs`, qui abandonne lui aussi `Ds18b20Bus` au
//!    profit de primitives brutes pour la mesure).
//!
//! Les délais viennent du timer matériel (`hal::Timer`), pas d'un compteur de
//! cycles : le décompte reste juste même si une interruption s'intercale.
//!
//! À trancher avec l'équipe : `sensors::ds18b20` conserve des primitives
//! équivalentes mais inutilisées en production, ainsi qu'un `ow_reset_long`
//! (800 µs) de récupération des clones bloqués que cette version n'a pas.

use rp2040_hal as hal;
use hal::pac;

/// Attente active en microsecondes, basée sur le timer matériel.
#[inline(always)]
pub fn ow_wait(timer: &hal::Timer, us: u64) {
    let end = timer.get_counter().ticks() + us;
    while timer.get_counter().ticks() < end { cortex_m::asm::nop(); }
}

/// Impulsion de reset + détection de présence.
///
/// `mask` est le masque de bit du GPIO (ex. `1 << 15` pour GP15).
/// Le GPIO doit avoir été configuré au préalable en sortie niveau bas puis
/// remis en haute impédance (cf. séquence d'init dans le binaire appelant).
pub fn ow_reset(timer: &hal::Timer, mask: u32) -> bool {
    unsafe {
        let sio = &*pac::SIO::ptr();
        sio.gpio_oe_clr().write(|w| w.bits(mask)); ow_wait(timer, 5);
        sio.gpio_oe_set().write(|w| w.bits(mask)); ow_wait(timer, 480);
        sio.gpio_oe_clr().write(|w| w.bits(mask)); ow_wait(timer, 70);
        let presence = sio.gpio_in().read().bits() & mask == 0;
        ow_wait(timer, 410);
        presence
    }
}

/// Écriture d'un bit (brique de base du SEARCH ROM).
pub fn ow_write_bit(timer: &hal::Timer, mask: u32, bit: bool) {
    unsafe {
        let sio = &*pac::SIO::ptr();
        sio.gpio_oe_set().write(|w| w.bits(mask));
        if bit {
            ow_wait(timer, 6);
            sio.gpio_oe_clr().write(|w| w.bits(mask));
            ow_wait(timer, 64);
        } else {
            ow_wait(timer, 60);
            sio.gpio_oe_clr().write(|w| w.bits(mask));
            ow_wait(timer, 10);
        }
    }
}

/// Lecture d'un bit — échantillonnage à ~10 µs du début du slot.
pub fn ow_read_bit(timer: &hal::Timer, mask: u32) -> bool {
    unsafe {
        let sio = &*pac::SIO::ptr();
        sio.gpio_oe_set().write(|w| w.bits(mask)); ow_wait(timer, 2);
        sio.gpio_oe_clr().write(|w| w.bits(mask)); ow_wait(timer, 8);
        let bit = sio.gpio_in().read().bits() & mask != 0;
        ow_wait(timer, 50);
        bit
    }
}

/// Écriture d'un octet, bit de poids faible en premier.
pub fn ow_write_byte(timer: &hal::Timer, mask: u32, byte: u8) {
    for i in 0..8u32 { ow_write_bit(timer, mask, (byte >> i) & 1 == 1); }
}

/// Lecture d'un octet, bit de poids faible en premier.
pub fn ow_read_byte(timer: &hal::Timer, mask: u32) -> u8 {
    let mut b = 0u8;
    for i in 0..8u32 { if ow_read_bit(timer, mask) { b |= 1 << i; } }
    b
}

/// CRC-8 Dallas / Maxim (polynôme réfléchi 0x8C) — ROM codes et scratchpad.
/// Sur N octets dont le dernier est le CRC → résultat 0 si valide.
pub fn crc8(data: &[u8]) -> u8 {
    let mut c = 0u8;
    for &b in data {
        let mut x = b;
        for _ in 0..8 { let m = (c ^ x) & 1; c >>= 1; if m != 0 { c ^= 0x8C; } x >>= 1; }
    }
    c
}

/// SEARCH ROM — algorithme Dallas AN187.
///
/// Appeler [`OwSearch::next`] en boucle jusqu'à `None` pour énumérer tous les
/// esclaves présents sur le bus.
pub struct OwSearch {
    last_discrepancy: u8,
    last_device_flag: bool,
    rom: [u8; 8],
}

impl Default for OwSearch {
    fn default() -> Self { Self::new() }
}

impl OwSearch {
    pub const fn new() -> Self {
        Self { last_discrepancy: 0, last_device_flag: false, rom: [0u8; 8] }
    }

    /// Retourne le ROM code suivant, ou `None` quand l'énumération est finie
    /// (ou si le bus ne répond pas).
    pub fn next(&mut self, timer: &hal::Timer, mask: u32) -> Option<[u8; 8]> {
        if self.last_device_flag { return None; }
        if !ow_reset(timer, mask) {
            self.last_discrepancy = 0;
            self.last_device_flag = false;
            return None;
        }
        ow_write_byte(timer, mask, 0xF0); // SEARCH ROM

        let mut last_zero = 0u8;
        let mut bit_number = 1u8;
        for byte_idx in 0..8usize {
            for bit_shift in 0..8u8 {
                let id_bit  = ow_read_bit(timer, mask);
                let cmp_bit = ow_read_bit(timer, mask);
                if id_bit && cmp_bit { return None; } // erreur bus

                let dir = if id_bit != cmp_bit {
                    id_bit
                } else {
                    // discordance : suivre le chemin précédent ou choisir 0
                    if bit_number < self.last_discrepancy {
                        (self.rom[byte_idx] >> bit_shift) & 1 == 1
                    } else {
                        bit_number == self.last_discrepancy
                    }
                };

                if !id_bit && !cmp_bit && !dir { last_zero = bit_number; }
                if dir { self.rom[byte_idx] |= 1 << bit_shift; }
                else   { self.rom[byte_idx] &= !(1 << bit_shift); }
                ow_write_bit(timer, mask, dir);
                bit_number += 1;
            }
        }
        self.last_discrepancy = last_zero;
        self.last_device_flag = last_zero == 0;
        Some(self.rom)
    }
}
