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
use embedded_hal::digital::OutputPin;
use rp2040_hal::{
    self as hal, Sio, Watchdog,
    clocks::init_clocks_and_plls,
    gpio::{DynBankId, DynPinId, DynPullType, FunctionSio, Pins, SioInput, SioOutput, new_pin},
    pac,
};

use cloud_chamber_firmware::config::wiring::PIN_ONEWIRE;
use cloud_chamber_firmware::drivers::ds18b20::{Ds18b20Bus, Resolution, rp2040_adapter::Rp2040OpenDrain};

/// Fréquence du cristal externe du Pico — cf. `hal::clocks::init_clocks_and_plls`.
const XOSC_CRYSTAL_FREQ: u32 = 12_000_000;

#[unsafe(link_section = ".boot2")]
#[used]
static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

/// Configure GP`pin` pour l'usage 1-Wire (`Rp2040OpenDrain`) : fonction SIO,
/// aucun pull interne (l'externe s'en charge), sortie fixée à 0 puis remise
/// en entrée flottante — exactement ce que `Rp2040OpenDrain` attend avant
/// de prendre la main sur `gpio_oe` en direct registre.
///
/// Passe par `gpio::new_pin`/`DynPinId` (API dynamique de `rp2040-hal`,
/// pensée pour ce cas) plutôt que par l'API typée `pins.gpio<N>`, qui exige
/// un identifiant littéral connu à la compilation — donc un champ dupliqué
/// qu'il faudrait garder manuellement synchronisé avec `PIN_ONEWIRE`. C'est
/// exactement le bug déjà rencontré une fois : `PIN_ONEWIRE` était passé de
/// 15 à 23, `pins.gpio15` en dessous ne l'a pas suivi, et rien au niveau du
/// compilateur ne pouvait le détecter. Ici `PIN_ONEWIRE` est directement
/// utilisé comme index — une seule source, rien à garder synchronisé.
///
/// `try_into_function` fait le travail registre (funcsel, validité pour
/// cette broche) à l'intérieur du HAL, pas à la main : on ne réécrit donc
/// plus soi-même la sémantique des registres IO_BANK0/PADS_BANK0/SIO.
///
/// # Safety
/// `new_pin` exige qu'aucune autre instance de `Pin` pour cette broche
/// n'existe en parallèle. `Pins::new(...)` (appelé juste avant, pour ses
/// effets de bord de sortie de reset) réserve bien un champ typé
/// `pins.gpio<N>` pour ce même numéro, mais ce champ n'est ni lu ni écrit
/// nulle part dans ce fichier : aucun accès concurrent réel aux registres
/// n'en résulte.
fn configure_onewire_pin(pin: u8) {
    let id = DynPinId { bank: DynBankId::Bank0, num: pin };
    let raw = unsafe { new_pin(id) };

    let mut out = raw
        .try_into_function::<FunctionSio<SioOutput>>()
        .ok()
        .expect("SIO est une fonction valide sur toute broche de Bank0");
    out.set_pull_type(DynPullType::None);
    let _ = out.set_low();

    // Remise en entrée flottante : c'est ensuite `Rp2040OpenDrain` qui
    // pilote `gpio_oe` en direct registre, sans repasser par ce type.
    let in_pin = out
        .try_into_function::<FunctionSio<SioInput>>()
        .ok()
        .expect("SIO est une fonction valide sur toute broche de Bank0");
    drop(in_pin);
}

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
    // `Pins::new` reste nécessaire même si le pin 1-Wire n'utilise pas son
    // API typée ensuite : c'est cet appel qui sort IO_BANK0/PADS_BANK0 de
    // reset (cf. doc de `configure_onewire_pin`).
    let _pins = Pins::new(pac.IO_BANK0, pac.PADS_BANK0, sio.gpio_bank0, &mut pac.RESETS);
    configure_onewire_pin(PIN_ONEWIRE);

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
