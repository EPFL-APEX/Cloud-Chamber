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
use embedded_hal::digital::PinState;
use rp2040_hal::{self as hal, Sio, Watchdog, clocks::init_clocks_and_plls, gpio::Pins, pac};

use cloud_chamber_firmware::config::wiring::PIN_ONEWIRE;
use cloud_chamber_firmware::drivers::ds18b20::{Ds18b20Bus, Resolution, rp2040_adapter::Rp2040OpenDrain};

/// Fréquence du cristal externe du Pico — cf. `hal::clocks::init_clocks_and_plls`.
const XOSC_CRYSTAL_FREQ: u32 = 12_000_000;

// `pins.gpio15` ci-dessous doit rester le même numéro que PIN_ONEWIRE : le
// champ se sélectionne par un identifiant littéral (`gpio<N>`), pas par une
// variable, donc rien ne les lie automatiquement. Si `PIN_ONEWIRE` change
// dans config/wiring.rs, cette assertion casse la compilation ici plutôt
// que de laisser ce fichier sonder la mauvaise broche en silence.
const _: () = assert!(PIN_ONEWIRE == 23, "adapter pins.gpio23 ci-dessous si PIN_ONEWIRE change");

#[unsafe(link_section = ".boot2")]
#[used]
static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

#[hal::entry]
fn main() -> ! {
    let mut pac = pac::Peripherals::take().unwrap();
    let mut watchdog = Watchdog::new(pac.WATCHDOG);

    let clocks = init_clocks_and_plls(
        XOSC_CRYSTAL_FREQ,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();

    // `hal::Timer` implémente `embedded_hal::delay::DelayNs` directement :
    // pas besoin d'un `cortex_m::delay::Delay` séparé (celui-ci n'implémente
    // que l'ancienne API embedded-hal 0.2, incompatible avec `Ds18b20Bus`).
    let mut timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

    let sio = Sio::new(pac.SIO);
    let pins = Pins::new(pac.IO_BANK0, pac.PADS_BANK0, sio.gpio_bank0, &mut pac.RESETS);

    // Séquence attendue par `Rp2040OpenDrain` (cf. sa doc) : la broche doit
    // être configurée en sortie niveau bas puis remise en haute impédance
    // avant de lui être confiée — ensuite il pilote lui-même `gpio_oe`
    // directement en registre, sans repasser par ce type.
    let _onewire_pin = pins
        .gpio15
        .into_push_pull_output_in_state(PinState::Low)
        .into_floating_input();

    let adapter = Rp2040OpenDrain::new(1u32 << PIN_ONEWIRE);
    let mut bus = Ds18b20Bus::new(adapter);

    let count = bus.discover(&mut timer);
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
        let _ = bus.start_conversion_broadcast(&mut timer);
        timer.delay_ms(Resolution::Bits12.conversion_time_ms().as_millis() as u32);

        for index in 0..count {
            match bus.read_celsius(index, &mut timer) {
                Ok(temp_c) => defmt::info!("  [{}] {} C", index, temp_c),
                Err(e) => defmt::warn!("  [{}] lecture invalide : {}", index, defmt::Debug2Format(&e)),
            }
        }
        timer.delay_ms(1_000);
    }
}
