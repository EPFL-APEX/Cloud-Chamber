//! Test capteur DS18B20 (1-Wire).
//!
//! Flasher sur le Pico (RP2040) ou Pico 2 (RP2350), ouvrir une console defmt (probe-rs run / defmt-print).
//! Toutes les constantes modifiables sont regroupées ici en haut.

#![no_std]
#![no_main]

// ─── Configuration ────────────────────────────────────────────────────────────

/// Broche GPIO connectée à la ligne DATA du bus 1-Wire.
/// Résistance pull-up 4.7kΩ obligatoire entre DATA et 3.3V.
const DATA_PIN: u8 = 22;

/// Délai entre deux lectures (en millisecondes).
const LOOP_PERIOD_MS: u32 = 1_000;

/// Délai de conversion DS18B20 en résolution 12 bits (en millisecondes).
const CONVERSION_DELAY_MS: u32 = 750;

// ─── Dépendances ──────────────────────────────────────────────────────────────

use defmt::{error, info};
use defmt_rtt as _;
use panic_probe as _;

#[cfg(rp2040)] use rp2040_hal as hal;
#[cfg(rp2350)] use rp235x_hal as hal;

/// Bootloader 2e étape requis par le RP2040 pour configurer la flash externe.
#[cfg(rp2040)]
#[unsafe(link_section = ".boot2")]
#[used]
pub static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

/// Signature d'image requise par la ROM du RP2350.
#[cfg(rp2350)]
#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: rp235x_hal::block::ImageDef = rp235x_hal::block::ImageDef::secure_exe();

use hal::pac;

// ─── Point d'entrée ──────────────────────────────────────────────────────────

#[hal::entry]
fn main() -> ! {
    let _ = DATA_PIN; // documentaire — voir câblage ci-dessus

    let mut pac = pac::Peripherals::take().unwrap();
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);

    let _clocks = hal::clocks::init_clocks_and_plls(
        12_000_000,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .unwrap();

    let sio = hal::Sio::new(pac.SIO);
    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // Broche 1-Wire configurée en sortie open-drain + pull-up interne.
    // Le protocole 1-Wire exige que la ligne soit tirée HIGH au repos.
    let one_wire_pin = pins.gpio22.into_pull_up_input();

    // Initialisation du bus 1-Wire.
    // `onewire::OneWire` accepte n'importe quelle broche implémentant
    // `embedded_hal::digital::InputPin + OutputPin`.
    let mut ow = onewire::OneWire::new(one_wire_pin);

    info!("Test DS18B20 — DATA_PIN=GPIO{}", DATA_PIN);

    loop {
        // 1. Lancer une conversion de température
        match ow.reset() {
            Ok(true) => {
                // Au moins un périphérique présent sur le bus
                ow.write_byte(onewire::commands::SKIP_ROM).ok();
                ow.write_byte(onewire::ds18b20::CONVERT_TEMP).ok();
            }
            Ok(false) => {
                error!("Aucun périphérique sur le bus 1-Wire !");
                delay_ms(LOOP_PERIOD_MS);
                continue;
            }
            Err(_) => {
                error!("Erreur reset bus 1-Wire");
                delay_ms(LOOP_PERIOD_MS);
                continue;
            }
        }

        // 2. Attendre la fin de la conversion (résolution 12 bits = 750 ms)
        delay_ms(CONVERSION_DELAY_MS);

        // 3. Lire le scratchpad
        match ow.reset() {
            Ok(true) => {
                ow.write_byte(onewire::commands::SKIP_ROM).ok();
                ow.write_byte(onewire::ds18b20::READ_SCRATCHPAD).ok();

                let mut buf = [0u8; 9];
                for b in &mut buf {
                    *b = ow.read_byte().unwrap_or(0);
                }

                // Décodage température (2 octets LSB, résolution 1/16 °C)
                let raw = i16::from_le_bytes([buf[0], buf[1]]);
                let celsius = raw as f32 / 16.0;
                info!("Température : {=f32:.2} °C (brut={})", celsius, raw);
            }
            Ok(false) => error!("Périphérique disparu après conversion"),
            Err(_) => error!("Erreur lecture scratchpad"),
        }

        delay_ms(LOOP_PERIOD_MS);
    }
}

#[inline(always)]
fn delay_ms(ms: u32) {
    // 125 000 cycles ≈ 1 ms à 125 MHz
    cortex_m::asm::delay(ms * 125_000);
}
