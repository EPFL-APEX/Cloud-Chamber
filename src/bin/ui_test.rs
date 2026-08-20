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
//! # Deux problèmes de performance constatés sur matériel réel
//!
//! ## Rendu lent (plusieurs secondes pour un menu)
//!
//! Résolu en déplaçant l'écran vers `drivers::display::FramebufferedDisplay`
//! (dessine dans un framebuffer RAM bandé, transfère chaque bande en une
//! seule transaction SPI, au lieu d'une transaction par pixel individuel)
//! — cf. sa documentation de module pour le détail et le raisonnement sur
//! la marge de pile.
//!
//! ## Rotation perdue pendant un dessin
//!
//! `FramebufferedDisplay::render` bloque quand même le cœur pendant le
//! transfert SPI de chaque bande. Avec un `encoder.poll()` appelé depuis la
//! boucle principale (comme dans les versions précédentes de ce bin), toute
//! rotation survenant *pendant* ce blocage n'est jamais lue — pas juste
//! retardée, perdue : deux rotations rapprochées ne comptaient que pour une.
//!
//! Fix : `RotaryEncoder::poll()` tourne maintenant depuis une interruption
//! matérielle périodique (`TIMER_IRQ_0`, alarme 0 du périphérique `TIMER`,
//! réarmée toutes les 1 ms), indépendante de ce que fait la boucle
//! principale. Une routine d'interruption reste préemptive même pendant un
//! blocage SPI classique (celui-ci n'attend pas dans une section critique,
//! seul du code protégé par `critical_section::with` — bref, autour des
//! accès aux statics partagés ci-dessous — désactive les interruptions).
//! La boucle principale ne fait plus que lire un drapeau "quelque chose a
//! changé", router l'événement déjà appliqué à `Screens`, et redessiner.
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
//! RP2040 uniquement pour l'instant, même limite que les autres bins de
//! bring-up. Derrière la feature `bin-ui-test` (désactivée par défaut)
//! pour ne pas être construit par les jobs CI `cargo check` sur les
//! autres cibles :
//!
//! ```text
//! cargo run --release --target thumbv6m-none-eabi --features bin-ui-test \
//!     --bin ui_test
//! ```
#![no_std]
#![no_main]

use core::cell::{Cell, RefCell};

use defmt_rtt as _;
use panic_probe as _;

use critical_section::Mutex;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::MODE_0;
use rp2040_hal::{
    Clock, Sio, Watchdog, self as hal,
    clocks::init_clocks_and_plls,
    fugit::{ExtU32, RateExtU32},
    gpio::{DynBankId, DynPinId, DynPullType, FunctionSio, FunctionSpi, Pin, Pins, SioInput, SioOutput, new_pin},
    pac::{self, interrupt},
    spi::{Spi, ValidatedPinSck, ValidatedPinTx},
    timer::{Alarm, Alarm0},
};

use display_interface_spi::SPIInterface;
use embedded_hal_bus::spi::ExclusiveDevice;
use ili9341::{DisplaySize240x320, Ili9341, Orientation};

use cloud_chamber_firmware::config::wiring::{
    PIN_ENCODER_A, PIN_ENCODER_B, PIN_ENCODER_SW, PIN_SCREEN_CS, PIN_SCREEN_DC, PIN_SCREEN_MOSI,
    PIN_SCREEN_RESET, PIN_SCREEN_SCK,
};
use cloud_chamber_firmware::drivers::display::FramebufferedDisplay;
use cloud_chamber_firmware::drivers::encoder::{EncoderEvent, RotaryEncoder};
use cloud_chamber_firmware::shared::data::SHARED_STATE;
use cloud_chamber_firmware::ui::router::Screens;

/// Fréquence du cristal externe du Pico — cf. `hal::clocks::init_clocks_and_plls`.
const XOSC_CRYSTAL_FREQ: u32 = 12_000_000;

#[unsafe(link_section = ".boot2")]
#[used]
static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

// ─── État partagé avec l'interruption TIMER_IRQ_0 ──────────────────────────

type EncPin = Pin<DynPinId, FunctionSio<SioInput>, DynPullType>;
type Encoder = RotaryEncoder<EncPin, EncPin, EncPin>;

/// `None` jusqu'à ce que `main()` y dépose l'encodeur — l'ISR ne fait rien
/// tant que ce n'est pas fait (ne peut pas se produire avant la fin de
/// `main()`'s setup, l'interruption n'étant démasquée qu'après).
static ENCODER: Mutex<RefCell<Option<Encoder>>> = Mutex::new(RefCell::new(None));
static ALARM: Mutex<RefCell<Option<Alarm0>>> = Mutex::new(RefCell::new(None));
/// Écrans + pile de navigation : mutée uniquement par l'ISR (right_turn/
/// left_turn/click), lue par la boucle principale pour dessiner. Un seul
/// `Screens` partagé plutôt que des compteurs d'événements en attente :
/// aucune raison de rejouer les événements côté boucle principale, l'ISR
/// peut appliquer la navigation directement.
static SCREENS: Mutex<RefCell<Option<Screens>>> = Mutex::new(RefCell::new(None));
/// Mis à `true` par l'ISR dès que `SCREENS` a changé ; la boucle principale
/// le lit puis le remet à `false` avant de redessiner.
static DIRTY: Mutex<Cell<bool>> = Mutex::new(Cell::new(false));

/// Routine d'interruption : appelée toutes les 1 ms par l'alarme 0 du
/// `TIMER`, indépendamment de ce que fait `main()` (y compris pendant un
/// blocage SPI). Fait le travail minimal — poller l'encodeur, router
/// l'événement, réarmer l'alarme — puis rend la main.
#[interrupt]
fn TIMER_IRQ_0() {
    critical_section::with(|cs| {
        if let Some(alarm) = ALARM.borrow(cs).borrow_mut().as_mut() {
            alarm.clear_interrupt();
            let _ = alarm.schedule(1_u32.millis());
        }

        let event = match ENCODER.borrow(cs).borrow_mut().as_mut() {
            Some(encoder) => encoder.poll(),
            None => return,
        };

        let mut screens_ref = SCREENS.borrow(cs).borrow_mut();
        let Some(screens) = screens_ref.as_mut() else { return };

        match event {
            EncoderEvent::RotateClockwise => {
                screens.right_turn();
                DIRTY.borrow(cs).set(true);
            }
            EncoderEvent::RotateCounterClockwise => {
                screens.left_turn();
                DIRTY.borrow(cs).set(true);
            }
            EncoderEvent::ButtonPressed => {
                screens.click();
                DIRTY.borrow(cs).set(true);
            }
            EncoderEvent::None => {}
        }
    });
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

    let ili9341_display = match Ili9341::new(iface, rst, &mut timer, Orientation::Landscape, DisplaySize240x320) {
        Ok(display) => display,
        Err(e) => {
            defmt::error!("echec init ecran : {}", defmt::Debug2Format(&e));
            loop {
                timer.delay_ms(1_000);
            }
        }
    };
    let mut display = FramebufferedDisplay::new(ili9341_display);

    // ─── Encodeur (A/B/SW, pull-up interne) — piloté par interruption ──────
    let pin_a = configure_input_pin(PIN_ENCODER_A);
    let pin_b = configure_input_pin(PIN_ENCODER_B);
    let pin_sw = configure_input_pin(PIN_ENCODER_SW);
    let encoder = RotaryEncoder::new(pin_a, pin_b, pin_sw);

    let mut alarm = timer.alarm_0().expect("alarme 0 disponible au premier appel");
    alarm.schedule(1_u32.millis()).expect("planification initiale valide");
    alarm.enable_interrupt();

    critical_section::with(|cs| {
        ENCODER.borrow(cs).replace(Some(encoder));
        ALARM.borrow(cs).replace(Some(alarm));
        SCREENS.borrow(cs).replace(Some(Screens::new()));
    });

    // Sûr : ENCODER/ALARM/SCREENS sont déposés juste au-dessus, avant que
    // l'interruption ne puisse jamais se déclencher.
    unsafe {
        pac::NVIC::unmask(pac::Interrupt::TIMER_IRQ_0);
    }

    defmt::info!("ui_test demarre — premier rendu (MainMenu)");

    // `Screens`/`SHARED_STATE` sont partagés avec l'ISR : empruntés depuis
    // la fermeture passée à `render`, rappelée une fois par bande.
    let redraw = |display: &mut FramebufferedDisplay<_, _>| {
        let _ = display.render(|target| {
            critical_section::with(|cs| {
                if let Some(screens) = SCREENS.borrow(cs).borrow().as_ref() {
                    let state = SHARED_STATE.borrow(cs).borrow();
                    screens.draw(target, &state)
                } else {
                    Ok(())
                }
            })
        });
    };

    redraw(&mut display);

    loop {
        let dirty = critical_section::with(|cs| {
            let was_dirty = DIRTY.borrow(cs).get();
            DIRTY.borrow(cs).set(false);
            was_dirty
        });

        if dirty {
            redraw(&mut display);
        }
    }
}

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
