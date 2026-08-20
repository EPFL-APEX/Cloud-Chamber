//! Sanity check de bring-up : câble l'écran ILI9341 et l'encodeur rotatif
//! réels sur `ui::router::Screens` — le vrai routeur de navigation, pas un
//! mock — pour vérifier l'UI de bout en bout (tourner/cliquer, écrans qui
//! s'affichent) avant intégration dans `logic::control_loop`.
//!
//! Combine les deux bring-up précédents (`screen_test`, `encoder_test`) :
//! mêmes broches (`config::wiring::PIN_SCREEN_*`/`PIN_ENCODER_*`), mêmes
//! techniques de configuration (`gpio::new_pin`/`DynPinId`,
//! `ValidatedPinTx`/`ValidatedPinSck` pour SPI0). Voir leur documentation
//! de module pour le détail de chaque étape.
//!
//! # Écrans qui vont paniquer (attendu, pas un bug matériel)
//!
//! `ui::router::Screens` a plusieurs branches `todo!()` pour des écrans pas
//! encore construits (état actuel du repo, cf. `src/ui/router.rs`) :
//! - Depuis le menu principal, seuls **Réglages** (Settings) et
//!   **Statistiques** (Stats, affichage seul) sont sûrs à ouvrir.
//! - Les 4 autres items du menu (Contrôle manuel, Refroidissement en
//!   cours, Données, Info) panniquent dès leur premier `draw()`.
//! - Une fois sur Stats, tourner ou cliquer panique aussi (`right_turn`/
//!   `left_turn`/`click` pas encore câblés pour cet écran).
//!
//! Un panic ici (message RTT clair via `panic-probe`, puis reset) n'est
//! donc pas forcément un problème de câblage — vérifier d'abord si le
//! chemin de navigation emprunté est un de ceux ci-dessus avant de
//! suspecter le matériel.
//!
//! RP2040 uniquement pour l'instant, même limite que les autres bins de
//! bring-up. Derrière la feature `bin-ui-test` (désactivée par défaut)
//! pour ne pas être construit par les jobs CI `cargo check` sur les
//! autres cibles :
//!
//! ```text
//! cargo run --target thumbv6m-none-eabi --features bin-ui-test \
//!     --bin ui_test
//! ```
#![no_std]
#![no_main]

use defmt_rtt as _;
use panic_probe as _;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::MODE_0;
use rp2040_hal::{
    Clock, Sio, Watchdog, self as hal,
    clocks::init_clocks_and_plls,
    fugit::RateExtU32,
    gpio::{DynBankId, DynPinId, DynPullType, FunctionSio, FunctionSpi, Pin, Pins, SioInput, SioOutput, new_pin},
    pac,
    spi::{Spi, ValidatedPinSck, ValidatedPinTx},
};

use display_interface_spi::SPIInterface;
use embedded_hal_bus::spi::ExclusiveDevice;
use ili9341::{DisplaySize240x320, Ili9341, Orientation};

use cloud_chamber_firmware::config::wiring::{
    PIN_ENCODER_A, PIN_ENCODER_B, PIN_ENCODER_SW, PIN_SCREEN_CS, PIN_SCREEN_DC, PIN_SCREEN_MOSI,
    PIN_SCREEN_RESET, PIN_SCREEN_SCK,
};
use cloud_chamber_firmware::drivers::encoder::{EncoderEvent, RotaryEncoder};
use cloud_chamber_firmware::shared::data::SHARED_STATE;
use cloud_chamber_firmware::ui::router::Screens;

/// Fréquence du cristal externe du Pico — cf. `hal::clocks::init_clocks_and_plls`.
const XOSC_CRYSTAL_FREQ: u32 = 12_000_000;

#[unsafe(link_section = ".boot2")]
#[used]
static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

/// Configure GP`pin` en sortie push-pull logicielle (CS/DC/RESET), démarrée
/// à l'état bas — cf. `screen_test`.
///
/// # Safety
/// `new_pin` exige qu'aucune autre instance de `Pin` pour cette broche
/// n'existe en parallèle. `Pins::new(...)` (appelé juste avant, pour ses
/// effets de bord de sortie de reset) réserve bien un champ typé
/// `pins.gpio<N>` pour ce même numéro, mais ce champ n'est ni lu ni écrit
/// nulle part dans ce fichier : aucun accès concurrent réel aux registres
/// n'en résulte.
fn configure_output_pin(pin: u8) -> Pin<DynPinId, FunctionSio<SioOutput>, DynPullType> {
    let id = DynPinId { bank: DynBankId::Bank0, num: pin };
    let raw = unsafe { new_pin(id) };
    let mut out = raw
        .try_into_function::<FunctionSio<SioOutput>>()
        .ok()
        .expect("SIO est une fonction valide sur toute broche de Bank0");
    out.set_pull_type(DynPullType::None);
    let _ = out.set_low();
    out
}

/// Configure GP`pin` en entrée avec pull-up interne (broches encodeur) —
/// cf. `encoder_test`.
///
/// # Safety
/// Même raisonnement que `configure_output_pin`.
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

    // ─── Écran (SPI0 + CS/DC/RESET logiciels) ──────────────────────────────
    let tx = unsafe { new_pin(DynPinId { bank: DynBankId::Bank0, num: PIN_SCREEN_MOSI }) }
        .try_into_function::<FunctionSpi>()
        .ok()
        .expect("SPI est une fonction valide sur toute broche de Bank0");
    let sck = unsafe { new_pin(DynPinId { bank: DynBankId::Bank0, num: PIN_SCREEN_SCK }) }
        .try_into_function::<FunctionSpi>()
        .ok()
        .expect("SPI est une fonction valide sur toute broche de Bank0");

    let tx = ValidatedPinTx::validate(tx, &pac.SPI0).unwrap_or_else(|_| {
        panic!("PIN_SCREEN_MOSI (GP{}) n'est pas une broche Tx/MOSI valide pour SPI0", PIN_SCREEN_MOSI)
    });
    let sck = ValidatedPinSck::validate(sck, &pac.SPI0).unwrap_or_else(|_| {
        panic!("PIN_SCREEN_SCK (GP{}) n'est pas une broche Sck valide pour SPI0", PIN_SCREEN_SCK)
    });

    // Turbofish DS=8 (taille de trame en bits) : plusieurs impls existent
    // (4/5/8...), rien ne force le choix sans cette annotation explicite.
    let spi = Spi::<_, _, _, 8>::new(pac.SPI0, (tx, sck)).init(
        &mut pac.RESETS,
        clocks.peripheral_clock.freq(),
        16_000_000u32.Hz(),
        MODE_0,
    );

    let cs = configure_output_pin(PIN_SCREEN_CS);
    let dc = configure_output_pin(PIN_SCREEN_DC);
    let rst = configure_output_pin(PIN_SCREEN_RESET);

    // CS::Error = Infallible (broche GPIO simple) : ne peut pas échouer en pratique.
    let spi_device = ExclusiveDevice::new_no_delay(spi, cs).unwrap();
    let iface = SPIInterface::new(spi_device, dc);

    let mut display = match Ili9341::new(iface, rst, &mut timer, Orientation::Landscape, DisplaySize240x320) {
        Ok(display) => display,
        Err(e) => {
            defmt::error!("echec init ecran : {}", defmt::Debug2Format(&e));
            loop {
                timer.delay_ms(1_000);
            }
        }
    };

    // ─── Encodeur (A/B/SW, pull-up interne) ────────────────────────────────
    let pin_a = configure_input_pin(PIN_ENCODER_A);
    let pin_b = configure_input_pin(PIN_ENCODER_B);
    let pin_sw = configure_input_pin(PIN_ENCODER_SW);
    let mut encoder = RotaryEncoder::new(pin_a, pin_b, pin_sw);

    // ─── Routeur UI réel ────────────────────────────────────────────────────
    let mut screens = Screens::new();

    defmt::info!("ui_test demarre — premier rendu (MainMenu)");
    critical_section::with(|cs| {
        let state = SHARED_STATE.borrow(cs).borrow();
        let _ = screens.draw(&mut display, &state);
    });

    // Redessine uniquement sur événement (rotation/clic) : un rendu complet
    // à 320x240 sur SPI à 16 MHz prend un temps non négligeable, inutile de
    // le refaire à chaque tour de boucle de poll (10 ms) sans rien de
    // nouveau à afficher.
    loop {
        match encoder.poll() {
            EncoderEvent::RotateClockwise => {
                defmt::info!("rotation horaire");
                screens.right_turn();
                critical_section::with(|cs| {
                    let state = SHARED_STATE.borrow(cs).borrow();
                    let _ = screens.draw(&mut display, &state);
                });
            }
            EncoderEvent::RotateCounterClockwise => {
                defmt::info!("rotation anti-horaire");
                screens.left_turn();
                critical_section::with(|cs| {
                    let state = SHARED_STATE.borrow(cs).borrow();
                    let _ = screens.draw(&mut display, &state);
                });
            }
            EncoderEvent::ButtonPressed => {
                defmt::info!("bouton presse");
                screens.click();
                critical_section::with(|cs| {
                    let state = SHARED_STATE.borrow(cs).borrow();
                    let _ = screens.draw(&mut display, &state);
                });
            }
            EncoderEvent::None => {}
        }
        timer.delay_ms(10);
    }
}
