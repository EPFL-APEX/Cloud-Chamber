//! Sanity check de bring-up : bascule ensemble les trois relais haute
//! tension/compresseur/chauffage iso — allumés 3 s, éteints 3 s, en boucle
//! — pour vérifier le câblage et l'attaque des relais indépendamment du
//! reste de la logique de contrôle.
//!
//! Broches tirées directement de `config::wiring` (`PIN_COMPRESSOR_RELAY`,
//! `PIN_HV_RELAY`, `PIN_ISO_HEATER_RELAY`), configurées par écriture
//! registre brute plutôt que par l'API typée `pins.gpio<N>` : même
//! raisonnement que `configure_onewire_pin` dans `identify_temp_sensors`,
//! pour ne jamais avoir de champ littéral à garder synchronisé à la main
//! avec ces constantes.
//!
//! Suppose une sortie active à l'état haut (relais commandé "on" par
//! GPIO = 1) — à inverser ici si le module de relais utilisé est actif
//! bas.
//!
//! RP2040 uniquement pour l'instant, même limite que `identify_temp_sensors`
//! et `blinky`. Derrière la feature `bin-relay-test` (désactivée par
//! défaut) pour ne pas être construit par les jobs CI `cargo check` sur les
//! autres cibles :
//!
//! ```text
//! cargo run --target thumbv6m-none-eabi --features bin-relay-test \
//!     --bin relay_test
//! ```
#![no_std]
#![no_main]

use defmt_rtt as _;
use panic_probe as _;

use embedded_hal::delay::DelayNs;
use rp2040_hal::{self as hal, Sio, Watchdog, clocks::init_clocks_and_plls, gpio::Pins, pac};

use cloud_chamber_firmware::config::wiring::{PIN_COMPRESSOR_RELAY, PIN_HV_RELAY, PIN_ISO_HEATER_RELAY};

/// Fréquence du cristal externe du Pico — cf. `hal::clocks::init_clocks_and_plls`.
const XOSC_CRYSTAL_FREQ: u32 = 12_000_000;

#[unsafe(link_section = ".boot2")]
#[used]
static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

/// Configure GP`pin` en sortie push-pull, démarrée à l'état bas (relais
/// éteint). Passe par les registres bruts, indexés directement sur `pin`,
/// plutôt que par l'API typée `pins.gpio<N>` — cf. doc de module.
///
/// À appeler après `Pins::new(...)`, qui sort IO_BANK0/PADS_BANK0 de reset.
fn configure_output_pin(pin: u8) {
    let n = pin as usize;
    let mask = 1u32 << pin;
    unsafe {
        (*pac::IO_BANK0::ptr()).gpio(n).gpio_ctrl().modify(|_, w| w.funcsel().sio());
        (*pac::PADS_BANK0::ptr()).gpio(n).modify(|_, w| w.pue().bit(false).pde().bit(false).ie().bit(false));
        (*pac::SIO::ptr()).gpio_out_clr().write(|w| w.bits(mask));
        (*pac::SIO::ptr()).gpio_oe_set().write(|w| w.bits(mask));
    }
}

/// Pilote GP`pin` à l'état haut (`high = true`) ou bas, via les registres
/// `gpio_out_set`/`gpio_out_clr` (écriture "bits à modifier" — n'affecte pas
/// les autres broches).
fn set_pin(pin: u8, high: bool) {
    let mask = 1u32 << pin;
    unsafe {
        if high {
            (*pac::SIO::ptr()).gpio_out_set().write(|w| w.bits(mask));
        } else {
            (*pac::SIO::ptr()).gpio_out_clr().write(|w| w.bits(mask));
        }
    }
}

const RELAY_PINS: [u8; 3] = [PIN_COMPRESSOR_RELAY, PIN_HV_RELAY, PIN_ISO_HEATER_RELAY];

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

    let mut timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

    let sio = Sio::new(pac.SIO);
    // `Pins::new` reste nécessaire même si son API typée n'est pas utilisée
    // ensuite : c'est cet appel qui sort IO_BANK0/PADS_BANK0 de reset.
    let _pins = Pins::new(pac.IO_BANK0, pac.PADS_BANK0, sio.gpio_bank0, &mut pac.RESETS);

    for &pin in RELAY_PINS.iter() {
        configure_output_pin(pin);
    }

    defmt::info!(
        "relay_test demarre — GP{} (compresseur), GP{} (HV), GP{} (chauffage iso), 3s on / 3s off",
        PIN_COMPRESSOR_RELAY,
        PIN_HV_RELAY,
        PIN_ISO_HEATER_RELAY
    );

    let mut on = false;
    loop {
        on = !on;
        for &pin in RELAY_PINS.iter() {
            set_pin(pin, on);
        }
        defmt::info!("relais = {}", on);
        timer.delay_ms(3_000);
    }
}
