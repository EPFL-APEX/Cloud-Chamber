//! Sanity check de bring-up : bascule ensemble les trois relais haute
//! tension/compresseur/chauffage iso — allumés 3 s, éteints 3 s, en boucle
//! — pour vérifier le câblage et l'attaque des relais indépendamment du
//! reste de la logique de contrôle.
//!
//! Broches tirées directement de `config::wiring` (`PIN_COMPRESSOR_RELAY`,
//! `PIN_HV_RELAY`, `PIN_ISO_HEATER_RELAY`...), configurées par
//! `board::configure_relay_pin` — c'est lui qui porte la force de commande
//! à 8 mA pour l'amorçage des MOC3043, cf. `board::RELAY_DRIVE_STRENGTH`.
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
use embedded_hal::digital::OutputPin;
use rp2040_hal as hal;

use cloud_chamber_firmware::board;
use cloud_chamber_firmware::config::wiring::{PIN_COMPRESSOR_RELAY, PIN_FANS_RELAY, PIN_HV_RELAY, PIN_ISO_HEATER_RELAY, PIN_LIGHTS_RELAY, PIN_PUMP_RELAY};

const RELAY_PINS: [u8; 5] = [PIN_COMPRESSOR_RELAY, PIN_HV_RELAY, PIN_LIGHTS_RELAY, PIN_FANS_RELAY, PIN_LIGHTS_RELAY];
const RELAY_ON_OFF: [u8; 2] = [PIN_PUMP_RELAY, PIN_ISO_HEATER_RELAY];

#[hal::entry]
fn main() -> ! {
    let mut board = board::init();

    let mut relays = RELAY_PINS.map(board::configure_relay_pin);
    let mut relays_on_off = RELAY_ON_OFF.map(board::configure_relay_pin);

    defmt::info!(
        "relay_test demarre — GP{} (compresseur), GP{} (HV), GP{} (chauffage iso), GP{} (PUMP), GP{} (LIGHTS) 3s on / 3s off",
        PIN_COMPRESSOR_RELAY,
        PIN_HV_RELAY,
        PIN_ISO_HEATER_RELAY,
        PIN_PUMP_RELAY,
        PIN_LIGHTS_RELAY
    );

    for relay in relays.iter_mut() {
        let _ = relay.set_high();
    }
    let mut on = true;
    loop {
        on = !on;
        if on {
            board.timer.delay_ms(120_000);
        }

        for i in 1..30 {
        relays_on_off[1].set_high();
        board.timer.delay_ms(0_100);
        for relay in relays_on_off.iter_mut() {
            relay.set_high();
        }
        board.timer.delay_ms(0_500);
        for relay in relays_on_off.iter_mut() {
            relay.set_low();
        }
        board.timer.delay_ms(2_000);
        }
    }
}
