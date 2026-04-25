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

/// Délai entre deux lectures (en millisecondes), en plus du temps de conversion.
const LOOP_PERIOD_MS: u32 = 1_000;

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

use embedded_hal::delay::DelayNs;
use hal::pac;
use onewire::{DeviceSearch, OneWire, DS18B20};

// ─── Point d'entrée ──────────────────────────────────────────────────────────
//
// Câblage :
//   GPIO22 ──┬── 4.7kΩ ── 3.3V   (pull-up externe obligatoire)
//            └── DATA du DS18B20
//   GND    ── GND du DS18B20
//   3.3V   ── VDD du DS18B20  (ou laisser VDD=GND en mode parasite, parasite_mode=true)

#[hal::entry]
fn main() -> ! {
    let _ = DATA_PIN; // documentaire — voir câblage ci-dessus

    let mut pac = pac::Peripherals::take().unwrap();
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);

    let clocks = hal::clocks::init_clocks_and_plls(
        12_000_000,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .unwrap();

    // Le timer sert de source de délai pour le protocole 1-Wire.
    #[cfg(rp2040)]
    let mut timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);
    #[cfg(rp2350)]
    let mut timer = hal::Timer::new_timer0(pac.TIMER0, &mut pac.RESETS, &clocks);

    let sio = hal::Sio::new(pac.SIO);
    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // La broche doit implémenter InputPin + OutputPin simultanément.
    // Sur RP2040/2350, une broche en sortie est toujours lisible en entrée.
    let one_wire_pin = pins.gpio22.into_push_pull_output();
    // parasite_mode = false : le DS18B20 est alimenté par VDD (pas parasite)
    let mut ow = OneWire::new(one_wire_pin, false);

    info!("Test DS18B20 — recherche sur GPIO{}...", DATA_PIN);

    // ── Recherche du premier DS18B20 sur le bus ───────────────────────────────
    let sensor = loop {
        let mut search = DeviceSearch::new_for_family(onewire::ds18b20::FAMILY_CODE);
        match ow.search_next(&mut search, &mut timer) {
            Ok(Some(device)) => {
                info!("DS18B20 trouvé : {}", device);
                match DS18B20::new(device) {
                    Ok(s) => break s,
                    Err(_) => error!("Family code inattendu"),
                }
            }
            Ok(None) => error!("Aucun DS18B20 sur le bus — vérifier câblage et pull-up 4.7kΩ"),
            Err(_) => error!("Erreur bus 1-Wire"),
        }
        timer.delay_ms(2_000);
    };

    // ── Boucle de mesure ──────────────────────────────────────────────────────
    loop {
        // 1. Déclencher la conversion et récupérer la résolution choisie
        match sensor.measure_temperature(&mut ow, &mut timer) {
            Ok(resolution) => {
                // 2. Attendre la fin de la conversion (94–750 ms selon résolution)
                timer.delay_ms(resolution.time_ms() as u32);

                // 3. Lire la température convertie
                match sensor.read_temperature(&mut ow, &mut timer) {
                    Ok(raw) => {
                        // raw est un i16 encodé sur 4 bits fractionnaires (1/16 °C)
                        let celsius = raw as i16 as f32 / 16.0;
                        info!("Température : {=f32:.2} °C", celsius);
            }
            Err(_) => error!("Erreur lecture scratchpad"),
        }
            }
            Err(_) => error!("Erreur démarrage conversion"),
        }

        timer.delay_ms(LOOP_PERIOD_MS);
    }
}
