//! Outil de bring-up : liste, dans l'ordre où `logic::probing` les verra,
//! le code ROM (identifiant unique, 8 octets) de chaque DS18B20 découvert
//! sur le bus 1-Wire, puis boucle en affichant la température de chacun —
//! toucher une sonde et regarder laquelle bouge permet de faire
//! correspondre index/ROM ↔ sonde physique en main.
//!
//! RP2040 uniquement pour l'instant (le port RP2350 suivrait le même
//! adaptateur `Rp2350OpenDrain`, cf. `drivers::ds18b20`, mais n'a pas été
//! testé ici). Derrière la feature `bin-identify-temp-sensors`
//! (désactivée par défaut) pour ne pas être construit par les jobs CI
//! `cargo check` sur les autres cibles :
//!
//! ```text
//! cargo build --target thumbv6m-none-eabi --features bin-identify-temp-sensors \
//!     --bin identify_temp_sensors
//! cargo run --target thumbv6m-none-eabi --features bin-identify-temp-sensors \
//!     --bin identify_temp_sensors
//! ```
#![no_std]
#![no_main]

use defmt_rtt as _;
use panic_probe as _;

use embedded_hal::delay::DelayNs;
use rp2040_hal as hal;

use cloud_chamber_firmware::board;
use cloud_chamber_firmware::config::wiring::PIN_ONEWIRE;
use cloud_chamber_firmware::drivers::ds18b20::{Ds18b20Bus, Resolution, rp2040_adapter::Rp2040OpenDrain};

#[hal::entry]
fn main() -> ! {
    let mut board = board::init();
    board::configure_onewire_pin(PIN_ONEWIRE);

    let adapter = Rp2040OpenDrain::new(1u32 << PIN_ONEWIRE);
    let mut bus = Ds18b20Bus::new(adapter);

    let count = bus.discover(&mut board.timer);
    defmt::info!(
        "{} capteur(s) DS18B20 trouve(s) sur le bus 1-Wire (GP{}) :",
        count,
        PIN_ONEWIRE
    );
    for index in 0..count {
        if let Some(rom) = bus.rom_code(index) {
            defmt::info!(
                "  [{}] {:02x}-{:02x}-{:02x}-{:02x}-{:02x}-{:02x}-{:02x}-{:02x}",
                index, rom[0], rom[1], rom[2], rom[3], rom[4], rom[5], rom[6], rom[7],
            );
        }
    }
    if count == 0 {
        defmt::warn!("Aucun capteur trouve — verifier le bus (pull-up, cablage GP{}).", PIN_ONEWIRE);
    }

    // Boucle de lecture continue : touche une sonde à la main et regarde
    // laquelle de ces lignes bouge pour associer l'index/l'id ci-dessus à
    // la sonde physique correspondante.
    loop {
        let _ = bus.start_conversion_broadcast(&mut board.timer);
        board.timer.delay_ms(Resolution::Bits12.conversion_time_ms().as_millis() as u32);

        for index in 0..count {
            match bus.read_celsius(index, &mut board.timer) {
                Ok(temp_c) => defmt::info!("  [{}] {} C", index, temp_c),
                Err(e) => defmt::warn!("  [{}] lecture invalide : {}", index, defmt::Debug2Format(&e)),
            }
        }
        board.timer.delay_ms(1_000);
    }
}
