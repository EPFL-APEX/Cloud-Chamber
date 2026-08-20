//! Sanity check de bring-up : instancie `drivers::encoder::RotaryEncoder`
//! sur le matériel réel et journalise chaque événement détecté (rotation
//! horaire/anti-horaire, appui bouton) — tourne l'encodeur et presse le
//! bouton en main pour vérifier le câblage et le sens de rotation avant de
//! le brancher dans l'UI.
//!
//! `RotaryEncoder` n'a aujourd'hui aucun appelant dans le reste du repo :
//! ce bin est le premier test sur du vrai matériel.
//!
//! Broches tirées directement de `config::wiring`
//! (`PIN_ENCODER_A`/`PIN_ENCODER_B`/`PIN_ENCODER_SW`), sélectionnées via
//! `gpio::new_pin`/`DynPinId` plutôt que par l'API typée `pins.gpio<N>` —
//! même raisonnement que `identify_temp_sensors`/`relay_test` : pas de
//! champ littéral à garder synchronisé à la main avec ces constantes.
//!
//! Pull-up interne sur les trois broches : standard pour un encodeur
//! mécanique dont le commun est au GND (contact = tire à la masse, repos =
//! haut). Si le tien est câblé autrement (commun au 3.3V), les événements
//! sortiront inversés ou bruités — observable directement dans les logs
//! ci-dessous, à ajuster si besoin une fois testé.
//!
//! # Cadence de poll
//!
//! `RotaryEncoder::poll` ne regarde qu'un front montant sur A et lit B à
//! cet instant précis — pas de machine à états complète sur les 4
//! transitions de quadrature. Un poll trop lent par rapport à la vitesse
//! de rotation manque des transitions ou les lit à moitié faites (A et B
//! pas encore synchrones), ce qui peut renvoyer le mauvais sens sans que
//! rien ne soit cassé côté câblage. 1 ms laisse largement plus de marge
//! que les 10 ms d'origine face à un cycle de quadrature complet, qui peut
//! survenir en 10-20 ms à vitesse de rotation normale.
//!
//! RP2040 uniquement pour l'instant, même limite que les autres bins de
//! bring-up. Derrière la feature `bin-encoder-test` (désactivée par
//! défaut) pour ne pas être construit par les jobs CI `cargo check` sur
//! les autres cibles :
//!
//! ```text
//! cargo run --target thumbv6m-none-eabi --features bin-encoder-test \
//!     --bin encoder_test
//! ```
#![no_std]
#![no_main]

use defmt_rtt as _;
use panic_probe as _;

use embedded_hal::delay::DelayNs;
use rp2040_hal::{
    self as hal, Sio, Watchdog,
    clocks::init_clocks_and_plls,
    gpio::{DynBankId, DynPinId, DynPullType, FunctionSio, Pin, Pins, SioInput, new_pin},
    pac,
};

use cloud_chamber_firmware::config::wiring::{PIN_ENCODER_A, PIN_ENCODER_B, PIN_ENCODER_SW};
use cloud_chamber_firmware::drivers::encoder::{EncoderEvent, RotaryEncoder};

/// Fréquence du cristal externe du Pico — cf. `hal::clocks::init_clocks_and_plls`.
const XOSC_CRYSTAL_FREQ: u32 = 12_000_000;

#[unsafe(link_section = ".boot2")]
#[used]
static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

/// Configure GP`pin` en entrée avec pull-up interne, et retourne le `Pin`
/// prêt à être passé à `RotaryEncoder` (implémente
/// `embedded_hal::digital::InputPin`).
///
/// Passe par `gpio::new_pin`/`DynPinId` plutôt que par l'API typée
/// `pins.gpio<N>` — cf. doc de module.
///
/// # Safety
/// `new_pin` exige qu'aucune autre instance de `Pin` pour cette broche
/// n'existe en parallèle. `Pins::new(...)` (appelé juste avant, pour ses
/// effets de bord de sortie de reset) réserve bien un champ typé
/// `pins.gpio<N>` pour ce même numéro, mais ce champ n'est ni lu ni écrit
/// nulle part dans ce fichier : aucun accès concurrent réel aux registres
/// n'en résulte.
fn configure_input_pin(pin: u8) -> Pin<DynPinId, FunctionSio<SioInput>, DynPullType> {
    let id = DynPinId { bank: DynBankId::Bank0, num: pin };
    let raw = unsafe { new_pin(id) };

    let mut in_pin = raw
        .try_into_function::<FunctionSio<SioInput>>()
        .ok()
        .expect("SIO est une fonction valide sur toute broche de Bank0");
    in_pin.set_pull_type(DynPullType::Up);
    in_pin
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

    let mut timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

    let sio = Sio::new(pac.SIO);
    // `Pins::new` reste nécessaire même si son API typée n'est pas utilisée
    // ensuite : c'est cet appel qui sort IO_BANK0/PADS_BANK0 de reset.
    let _pins = Pins::new(pac.IO_BANK0, pac.PADS_BANK0, sio.gpio_bank0, &mut pac.RESETS);

    let pin_a = configure_input_pin(PIN_ENCODER_A);
    let pin_b = configure_input_pin(PIN_ENCODER_B);
    let pin_sw = configure_input_pin(PIN_ENCODER_SW);
    let mut encoder = RotaryEncoder::new(pin_a, pin_b, pin_sw);

    defmt::info!(
        "encoder_test demarre — A=GP{} B=GP{} SW=GP{}, poll toutes les 1ms",
        PIN_ENCODER_A,
        PIN_ENCODER_B,
        PIN_ENCODER_SW
    );

    loop {
        match encoder.poll() {
            EncoderEvent::RotateClockwise => defmt::info!("rotation horaire"),
            EncoderEvent::RotateCounterClockwise => defmt::info!("rotation anti-horaire"),
            EncoderEvent::ButtonPressed => defmt::info!("bouton presse"),
            EncoderEvent::None => {}
        }
        timer.delay_ms(1);
    }
}
