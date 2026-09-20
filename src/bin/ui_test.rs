//! Sanity check de bring-up : câble l'écran ILI9341 et l'encodeur rotatif
//! réels sur `ui::app::UiApp` — la vraie UI, pas un mock — pour la
//! vérifier de bout en bout (tourner/cliquer, écrans qui s'affichent,
//! démarrage d'un cycle) avant intégration dans `logic::control_loop`.
//!
//! # Répartition du travail
//!
//! Ce fichier ne contient plus que ce qui est propre à la puce : le
//! bring-up matériel, les statics partagés, l'ISR, et le `loop {}`. Tout
//! ce qui est décidable sans matériel — quel événement fait quoi, quand
//! redessiner, quand un démarrage est légitime — vit dans `ui::app`, testé
//! sur hôte. Voir sa documentation de module pour le pourquoi de ce
//! découpage.
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
//! `FramebufferedDisplay::render` bloque le cœur pendant le dessin de
//! chaque bande puis son transfert SPI. Avec un `encoder.poll()` appelé
//! depuis la boucle principale, toute rotation survenant *pendant* ce
//! blocage n'était jamais lue — pas retardée, perdue : deux rotations
//! rapprochées ne comptaient que pour une.
//!
//! Premier correctif : `RotaryEncoder::poll()` depuis une interruption
//! périodique (`TIMER_IRQ_0`, alarme 0 du `TIMER`, réarmée toutes les
//! 1 ms), préemptive même pendant un blocage SPI.
//!
//! **Mais l'interruption seule ne suffisait pas.** En faisant appliquer la
//! navigation par l'ISR, `UiApp` devenait un static partagé, donc le dessin
//! se faisait `critical_section::with` pris — et une section critique
//! masque justement `TIMER_IRQ_0`. Le dessin des bandes dans le
//! framebuffer, la partie la plus coûteuse en calcul, se déroulait
//! interruptions coupées : l'encodeur n'y était pas scruté, et le bug
//! d'origine revenait par l'autre bout.
//!
//! Correctif complet, repris de `main.rs` : l'ISR ne fait **qu'empiler**
//! dans une [`EventQueue`](cloud_chamber_firmware::ui::event_queue) —
//! quelques instructions, section critique de longueur bornée. `UiApp`
//! appartient à la boucle principale et n'est partagé avec personne, donc
//! le dessin se fait sans aucun verrou : l'encodeur reste scruté à 1 ms
//! d'un bout à l'autre du rendu. Voir la documentation de ce module pour le
//! raisonnement complet.
//!
//! # Écrans pas encore construits
//!
//! Les 6 items du menu principal sont tous sûrs à ouvrir, y compris en
//! tournant et en cliquant dedans. **Données** et **Info** n'ont pas encore
//! d'écran réel : ils affichent le carton d'attente de
//! `ui::screens::placeholder`, dont un clic ressort. Ils paniquaient
//! jusqu'ici dès leur premier `draw()`.
//!
//! # Démarrage d'un cycle
//!
//! Cliquer sur le premier item du menu écrit
//! `SystemTask::Cooling(SensorCheck)` dans `SHARED_STATE.task` et bascule
//! sur l'écran de suivi. Ce bin n'exécute **pas**
//! `logic::control_loop::run()` (pas de capteurs ni d'actionneurs réels
//! câblés ici) : l'état écrit reste donc figé sur `SensorCheck` et aucune
//! phase n'avance. C'est le comportement attendu de ce bring-up — il
//! vérifie le chemin UI → état partagé, pas la machine à états.
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

use core::cell::RefCell;

use defmt_rtt as _;
use panic_probe as _;

use critical_section::Mutex;
use embedded_hal::delay::DelayNs;
use embedded_hal::spi::MODE_0;
use rp2040_hal::{
    Clock, self as hal,
    fugit::{ExtU32, RateExtU32},
    gpio::{DynBankId, DynPinId, DynPullType, FunctionSio, FunctionSpi, Pin, SioInput, new_pin},
    pac::{self, interrupt},
    spi::{Spi, ValidatedPinSck, ValidatedPinTx},
    timer::{Alarm, Alarm0},
};

use display_interface_spi::SPIInterface;
use embedded_hal_bus::spi::ExclusiveDevice;
use ili9341::{DisplaySize240x320, Ili9341, Orientation};

use cloud_chamber_firmware::board;
use cloud_chamber_firmware::config::wiring::{
    PIN_ENCODER_A, PIN_ENCODER_B, PIN_ENCODER_SW, PIN_SCREEN_CS, PIN_SCREEN_DC, PIN_SCREEN_MOSI,
    PIN_SCREEN_RESET, PIN_SCREEN_SCK,
};
use cloud_chamber_firmware::drivers::display::{self, FramebufferedDisplay};
use cloud_chamber_firmware::drivers::encoder::{EncoderEvent, RotaryEncoder};
use cloud_chamber_firmware::shared::data::{SHARED_STATE, SharedState};
use cloud_chamber_firmware::ui::app::UiApp;
use cloud_chamber_firmware::ui::event_queue::EventQueue;

// ─── État partagé avec l'interruption TIMER_IRQ_0 ──────────────────────────

type EncPin = Pin<DynPinId, FunctionSio<SioInput>, DynPullType>;
type Encoder = RotaryEncoder<EncPin, EncPin, EncPin>;

/// `None` jusqu'à ce que `main()` y dépose l'encodeur — l'ISR ne fait rien
/// tant que ce n'est pas fait (ne peut pas se produire avant la fin de
/// `main()`'s setup, l'interruption n'étant démasquée qu'après).
static ENCODER: Mutex<RefCell<Option<Encoder>>> = Mutex::new(RefCell::new(None));
static ALARM: Mutex<RefCell<Option<Alarm0>>> = Mutex::new(RefCell::new(None));
/// Événements encodeur en attente, entre l'ISR qui les produit et la boucle
/// qui les applique.
///
/// C'est **le** point de la correction de performance : `UiApp` n'est pas
/// ici, il appartient à `main()`. L'ISR n'a donc rien à emprunter de long,
/// et la boucle peut dessiner sans section critique. Cf. la doc de module
/// et celle de [`EventQueue`](cloud_chamber_firmware::ui::event_queue).
static EVENTS: Mutex<RefCell<EventQueue>> = Mutex::new(RefCell::new(EventQueue::new()));

/// Routine d'interruption : appelée toutes les 1 ms par l'alarme 0 du
/// `TIMER`, indépendamment de ce que fait `main()` (y compris pendant un
/// rendu). Poller l'encodeur, empiler, réarmer l'alarme — et rien d'autre :
/// tout ce qui prendrait du temps ici allongerait une section critique, ce
/// qui reviendrait à se masquer soi-même au tour suivant.
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

        // `None` est de très loin le cas le plus fréquent (1000 scrutations
        // par seconde) : ne rien empiler évite de saturer la file pour rien.
        if event != EncoderEvent::None {
            EVENTS.borrow(cs).borrow_mut().push(event);
        }
    });
}

#[hal::entry]
fn main() -> ! {
    let mut board = board::init();

    // ─── Écran (SPI0 + CS/DC/RESET logiciels) ──────────────────────────────
    let tx = unsafe { new_pin(DynPinId { bank: DynBankId::Bank0, num: PIN_SCREEN_MOSI }) }
        .try_into_function::<FunctionSpi>()
        .ok()
        .expect("SPI est une fonction valide sur toute broche de Bank0");
    let sck = unsafe { new_pin(DynPinId { bank: DynBankId::Bank0, num: PIN_SCREEN_SCK }) }
        .try_into_function::<FunctionSpi>()
        .ok()
        .expect("SPI est une fonction valide sur toute broche de Bank0");

    let tx = ValidatedPinTx::validate(tx, &board.spi0).unwrap_or_else(|_| {
        panic!("PIN_SCREEN_MOSI (GP{}) n'est pas une broche Tx/MOSI valide pour SPI0", PIN_SCREEN_MOSI)
    });
    let sck = ValidatedPinSck::validate(sck, &board.spi0).unwrap_or_else(|_| {
        panic!("PIN_SCREEN_SCK (GP{}) n'est pas une broche Sck valide pour SPI0", PIN_SCREEN_SCK)
    });

    // Turbofish DS=8 (taille de trame en bits) : plusieurs impls existent
    // (4/5/8...), rien ne force le choix sans cette annotation explicite.
    //
    // 32 MHz : doublé depuis les 16 MHz initiaux. L'ILI9341 est souvent
    // documenté prudemment (~10-15 MHz) mais couramment poussé à 40 MHz+
    // sur un câblage court et propre — 32 MHz reste une marge raisonnable
    // sans matériel sous la main pour vérifier le point de rupture réel. Si
    // l'écran affiche du bruit visuel (pixels aléatoires, lignes
    // corrompues), c'est le signe d'être allé trop loin : rebaisser cette
    // valeur (le maximum matériel du RP2040 est peripheral_clock / 2, soit
    // ~62.5 MHz à l'horloge système par défaut).
    let spi = Spi::<_, _, _, 8>::new(board.spi0, (tx, sck)).init(
        &mut board.resets,
        board.clocks.peripheral_clock.freq(),
        32_000_000u32.Hz(),
        MODE_0,
    );

    let cs = board::configure_output_pin(PIN_SCREEN_CS);
    let dc = board::configure_output_pin(PIN_SCREEN_DC);
    let rst = board::configure_output_pin(PIN_SCREEN_RESET);

    // CS::Error = Infallible (broche GPIO simple) : ne peut pas échouer en pratique.
    let spi_device = ExclusiveDevice::new_no_delay(spi, cs).unwrap();
    let iface = SPIInterface::new(spi_device, dc);

    let ili9341_display = match Ili9341::new(iface, rst, &mut board.timer, Orientation::Landscape, DisplaySize240x320) {
        Ok(display) => display,
        Err(e) => {
            defmt::error!("echec init ecran : {}", defmt::Debug2Format(&e));
            loop {
                board.timer.delay_ms(1_000);
            }
        }
    };
    let framebuffer = display::take_framebuffer().expect("le framebuffer n'est reclame qu'ici");
    let mut display = FramebufferedDisplay::new(ili9341_display, framebuffer);

    // ─── Encodeur (A/B/SW, pull-up interne) — piloté par interruption ──────
    let pin_a = board::configure_input_pin(PIN_ENCODER_A);
    let pin_b = board::configure_input_pin(PIN_ENCODER_B);
    let pin_sw = board::configure_input_pin(PIN_ENCODER_SW);
    let encoder = RotaryEncoder::new(pin_a, pin_b, pin_sw);

    let mut alarm = board.timer.alarm_0().expect("alarme 0 disponible au premier appel");
    alarm.schedule(1_u32.millis()).expect("planification initiale valide");
    alarm.enable_interrupt();

    critical_section::with(|cs| {
        ENCODER.borrow(cs).replace(Some(encoder));
        ALARM.borrow(cs).replace(Some(alarm));
    });

    // L'UI appartient à cette fonction — c'est tout l'objet du découplage.
    let mut app = UiApp::new();

    // Sûr : ENCODER/ALARM sont déposés juste au-dessus, avant que
    // l'interruption ne puisse jamais se déclencher.
    unsafe {
        pac::NVIC::unmask(pac::Interrupt::TIMER_IRQ_0);
    }

    defmt::info!("ui_test demarre — premier rendu (MainMenu)");

    // Redessin : aucune section critique. `app` appartient à cette
    // fonction et `state` est une copie déjà prise — le dessin des bandes
    // dans le framebuffer **et** leur transfert SPI se font donc
    // interruptions ouvertes, ce qui laisse `TIMER_IRQ_0` scruter
    // l'encodeur pendant tout le rendu. C'est la différence entre une UI
    // qui rate des crans et une qui n'en rate pas.
    //
    // Chronométrage : `Timer` est `Copy` (juste un accès aux registres
    // matériels), capturer une copie dans la fermeture ne pose pas de
    // problème de possession face au `board.timer` utilisé plus haut. Sert à
    // vérifier concrètement l'effet des optimisations (framebuffer,
    // interruption, vitesse SPI) plutôt que de se fier à une impression.
    let redraw = |display: &mut FramebufferedDisplay<_, _>, app: &UiApp, state: &SharedState| {
        let start = board.timer.get_counter();
        let _ = display.render(|target| app.draw(target, state));
        let elapsed = board.timer.get_counter() - start;
        defmt::info!("redraw termine en {} ms", elapsed.to_millis());
    };

    let initial_state = critical_section::with(|cs| *SHARED_STATE.borrow_ref(cs));
    redraw(&mut display, &app, &initial_state);
    app.take_redraw_request();

    loop {
        // Applique les événements empilés par l'interruption. L'état
        // courant est relu à chaque tour de boucle : `UiApp` s'en sert pour
        // refuser un démarrage si la machine tourne déjà.
        while let Some(event) = critical_section::with(|cs| EVENTS.borrow(cs).borrow_mut().pop()) {
            let current = critical_section::with(|cs| SHARED_STATE.borrow_ref(cs).task);

            // Un clic peut demander un changement d'état (premier item du
            // menu : démarrage d'un cycle). L'UI ne fait que le demander —
            // c'est ici qu'on l'applique.
            if let Some(task) = app.handle_event(event, current) {
                critical_section::with(|cs| SHARED_STATE.borrow_ref_mut(cs).task = task);
                defmt::info!("demande operateur : nouvel etat systeme");
            }
        }

        let dropped = critical_section::with(|cs| EVENTS.borrow(cs).borrow_mut().take_dropped());
        if dropped > 0 {
            defmt::warn!("{} evenement(s) encodeur perdus : file pleine", dropped);
        }

        // Copie de l'état sous verrou court ; le dessin travaille dessus,
        // hors verrou.
        let state = critical_section::with(|cs| *SHARED_STATE.borrow_ref(cs));

        if app.take_redraw_request() {
            redraw(&mut display, &app, &state);
        }
    }
}
