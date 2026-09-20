//! La console opérateur : l'écran ILI9341 et l'encodeur rotatif, montés et
//! pilotés d'une seule façon pour tout le firmware.
//!
//! # Pourquoi ce module existe
//!
//! `board` s'arrête à « les périphériques sont sortis de reset ».
//! [`ui::app`](crate::ui::app) commence à « un événement est arrivé ». Entre
//! les deux, il y avait une centaine de lignes sans domicile — montage du
//! SPI, ILI9341, framebuffer, encodeur, alarme, interruption, file — qui
//! existaient **deux fois**, dans `main.rs` et `bin/ui_test.rs`, et
//! partiellement une troisième dans `bin/screen_test.rs`.
//!
//! Ce n'était pas une question de volume. La copie avait déjà divergé sur
//! la chose qui compte : la vitesse du bus SPI. Le firmware attaquait
//! l'écran à 32 MHz, `ui_test` à 32 MHz par une valeur recopiée à la main,
//! et **`screen_test` — le bin dont le seul métier est de valider l'écran —
//! à 16 MHz.** Il ne pouvait donc pas, par construction, reproduire le
//! bruit visuel que [`SCREEN_SPI_HZ`] décrit comme le symptôme d'une
//! fréquence trop haute. C'est exactement le défaut qui avait touché
//! `relay_test` avant `board` : le bin de bring-up validait autre chose que
//! ce que le firmware exécutait.
//!
//! Une seule définition supprime la classe entière du problème. Les bins de
//! bring-up testent désormais le chemin réel — même fréquence, même
//! framebuffer, même interruption.
//!
//! # L'architecture qu'il impose
//!
//! ```text
//!   TIMER_IRQ_0 (1 ms)          boucle principale
//!   ┌──────────────┐            ┌──────────────────────────┐
//!   │ encoder.poll │──push──▶ EVENTS ──pop──▶ UiApp        │
//!   └──────────────┘            │   (possédé par la boucle)│
//!    section critique           │   draw sans verrou       │
//!    de longueur bornée         └──────────────────────────┘
//! ```
//!
//! L'ISR n'applique rien : elle empile. C'est ce qui permet à `UiApp` de
//! n'être partagé avec personne, donc de se dessiner **hors section
//! critique** — et une section critique masque justement `TIMER_IRQ_0`.
//! Dessiner sous verrou revenait à ne pas scruter l'encodeur pendant la
//! partie la plus coûteuse du rendu, c'est-à-dire à réintroduire le bug que
//! l'interruption devait corriger. Cf. [`crate::ui::event_queue`] pour le
//! détail du raisonnement.
//!
//! Sur la machine complète il y a une seconde raison, plus dure : le cœur 1
//! fait tourner la boucle de contrôle, et `critical_section` sur RP2040
//! prend un spinlock **commun aux deux cœurs**. Dessiner sous verrou y
//! bloquerait le contrôle pendant des dizaines de millisecondes.
//!
//! # Utilisation
//!
//! ```text
//! let mut board = board::init();
//! let mut console = console::init(board.spi0, &mut board.resets,
//!                                 board.clocks.peripheral_clock.freq(),
//!                                 &mut board.timer);
//! let mut app = UiApp::new();
//! loop {
//!     console.pump(&mut app);
//!     let state = critical_section::with(|cs| *SHARED_STATE.borrow_ref(cs));
//!     if app.take_redraw_request() {
//!         console.redraw(&app, &state);
//!     }
//! }
//! ```
//!
//! Les deux moitiés se prennent aussi séparément : [`init_display`] sans
//! encodeur, [`start_encoder`] sans écran (c'est ce que fait
//! `bin/encoder_test.rs`, qui n'a pas d'écran câblé).

use core::cell::RefCell;

use critical_section::Mutex;
use display_interface_spi::SPIInterface;
use embedded_hal::delay::DelayNs;
use embedded_hal::spi::MODE_0;
use embedded_hal_bus::spi::{ExclusiveDevice, NoDelay};
use ili9341::{DisplaySize240x320, Ili9341, Orientation};
use rp2040_hal::{
    self as hal,
    fugit::{ExtU32, HertzU32, RateExtU32},
    gpio::{
        DynBankId, DynPinId, DynPullType, FunctionSio, FunctionSpi, Pin, SioInput, SioOutput,
        new_pin,
    },
    pac::{self, interrupt},
    spi::{Enabled, Spi, ValidatedPinSck, ValidatedPinTx},
    timer::{Alarm, Alarm0},
};

use crate::board;
use crate::config::wiring::{
    PIN_ENCODER_A, PIN_ENCODER_B, PIN_ENCODER_SW, PIN_SCREEN_CS, PIN_SCREEN_DC, PIN_SCREEN_MOSI,
    PIN_SCREEN_RESET, PIN_SCREEN_SCK,
};
use crate::drivers::display::{self, FramebufferedDisplay};
use crate::drivers::encoder::{EncoderEvent, RotaryEncoder};
use crate::shared::data::{SHARED_STATE, SharedState};
use crate::ui::app::UiApp;
use crate::ui::event_queue::EventQueue;

// ─── Réglages ────────────────────────────────────────────────────────────────

/// Fréquence du bus SPI de l'écran.
///
/// Doublée depuis les 16 MHz initiaux. L'ILI9341 est souvent documenté
/// prudemment (~10-15 MHz) mais couramment poussé à 40 MHz+ sur un câblage
/// court et propre — 32 MHz reste une marge raisonnable. Le maximum
/// matériel du RP2040 est `peripheral_clock / 2`, soit ~62,5 MHz à
/// l'horloge système par défaut.
///
/// **Symptôme d'un réglage trop haut** : bruit visuel — pixels aléatoires,
/// lignes corrompues. Baisser ici, et `screen_test` en hérite : c'est
/// précisément pour ça que la valeur est unique.
pub const SCREEN_SPI_HZ: u32 = 32_000_000;

/// Période de scrutation de l'encodeur.
///
/// `RotaryEncoder::poll` ne regarde qu'un front montant sur A et lit B à cet
/// instant : une scrutation trop lente manque des transitions ou les lit à
/// moitié faites, ce qui renvoie le mauvais sens sans que rien ne soit cassé
/// côté câblage. Un cycle de quadrature complet dure 10-20 ms à vitesse
/// normale — 1 ms laisse une marge large.
pub const ENCODER_POLL_MS: u32 = 1;

// ─── Types de la chaîne d'affichage ──────────────────────────────────────────
//
// Le type réel de l'écran est une pile de six génériques imbriqués. Il est
// nommé ici une fois pour toutes ; aucun appelant n'a à l'épeler.

/// Broche GPIO logicielle de l'écran (CS, DC, RESET).
type ScreenPin = Pin<DynPinId, FunctionSio<SioOutput>, DynPullType>;
/// Broche confiée au périphérique SPI0 (SCK, MOSI).
type ScreenSpiPin = Pin<DynPinId, FunctionSpi, DynPullType>;
type ScreenSpi = Spi<
    Enabled,
    pac::SPI0,
    (ValidatedPinTx<ScreenSpiPin, pac::SPI0>, ValidatedPinSck<ScreenSpiPin, pac::SPI0>),
    8,
>;
type ScreenInterface = SPIInterface<ExclusiveDevice<ScreenSpi, ScreenPin, NoDelay>, ScreenPin>;

/// L'écran de la chambre, framebuffer compris. Cf. [`init_display`].
pub type OperatorDisplay = FramebufferedDisplay<ScreenInterface, ScreenPin>;

// ─── État partagé avec l'interruption ────────────────────────────────────────

type EncPin = Pin<DynPinId, FunctionSio<SioInput>, DynPullType>;
type Encoder = RotaryEncoder<EncPin, EncPin, EncPin>;

/// `None` jusqu'à ce que [`start_encoder`] y dépose l'encodeur —
/// l'interruption n'étant démasquée qu'après, l'ISR ne peut pas observer ce
/// `None`.
static ENCODER: Mutex<RefCell<Option<Encoder>>> = Mutex::new(RefCell::new(None));
static ALARM: Mutex<RefCell<Option<Alarm0>>> = Mutex::new(RefCell::new(None));

/// Événements encodeur en attente, entre l'ISR qui les produit et la boucle
/// qui les applique. Cf. la doc de module.
static EVENTS: Mutex<RefCell<EventQueue>> = Mutex::new(RefCell::new(EventQueue::new()));

/// Scrutation de l'encodeur, toutes les [`ENCODER_POLL_MS`] millisecondes.
///
/// Empile et rien d'autre : tout travail supplémentaire ici allongerait une
/// section critique, ce qui reviendrait à se masquer soi-même au tour
/// suivant — et, sur la machine complète, à faire attendre le cœur 1 sur le
/// spinlock.
///
/// Vit dans la bibliothèque et non dans chaque binaire. Le symbole fort
/// écrase l'entrée faible de la table de vecteurs à condition que l'objet
/// soit tiré de l'archive — ce que garantit l'appel à [`start_encoder`],
/// qui est dans ce même module (même mécanisme que `board::BOOT2`).
/// Vérifié sur l'ELF produit, pas seulement supposé : cf.
/// `scripts/check_vector_table.py`.
#[interrupt]
fn TIMER_IRQ_0() {
    critical_section::with(|cs| {
        if let Some(alarm) = ALARM.borrow(cs).borrow_mut().as_mut() {
            alarm.clear_interrupt();
            let _ = alarm.schedule(ENCODER_POLL_MS.millis());
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

// ─── Montage ─────────────────────────────────────────────────────────────────

/// Monte l'écran : SPI0 à [`SCREEN_SPI_HZ`], ILI9341 en paysage,
/// framebuffer RAM.
///
/// # Panics
///
/// - si `PIN_SCREEN_MOSI`/`PIN_SCREEN_SCK` ne sont pas les broches Tx/Sck
///   câblées en dur pour SPI0 sur ce silicium (table fixe du datasheet,
///   vérifiée par `ValidatedPinTx`/`ValidatedPinSck`) ;
/// - si le framebuffer a déjà été réclamé — il est unique ;
/// - si l'ILI9341 ne répond pas.
///
/// Les trois sont des défaillances de démarrage. L'écran est le seul canal
/// d'information de l'opérateur : continuer sans lui, ce serait piloter à
/// l'aveugle. `panic_probe` rend la cause visible sur la sonde.
pub fn init_display(
    spi0: pac::SPI0,
    resets: &mut pac::RESETS,
    peripheral_freq: HertzU32,
    delay: &mut impl DelayNs,
) -> OperatorDisplay {
    // Safety : seule construction de `Pin` pour ces broches. `Board::pins`
    // réserve bien un champ typé par numéro, mais aucun n'est lu ni écrit —
    // cf. la note de sûreté commune dans `board`.
    let tx = unsafe { new_pin(DynPinId { bank: DynBankId::Bank0, num: PIN_SCREEN_MOSI }) }
        .try_into_function::<FunctionSpi>()
        .ok()
        .expect("SPI est une fonction valide sur toute broche de Bank0");
    let sck = unsafe { new_pin(DynPinId { bank: DynBankId::Bank0, num: PIN_SCREEN_SCK }) }
        .try_into_function::<FunctionSpi>()
        .ok()
        .expect("SPI est une fonction valide sur toute broche de Bank0");

    let tx = ValidatedPinTx::validate(tx, &spi0).unwrap_or_else(|_| {
        panic!("PIN_SCREEN_MOSI (GP{}) n'est pas une broche Tx/MOSI valide pour SPI0", PIN_SCREEN_MOSI)
    });
    let sck = ValidatedPinSck::validate(sck, &spi0).unwrap_or_else(|_| {
        panic!("PIN_SCREEN_SCK (GP{}) n'est pas une broche Sck valide pour SPI0", PIN_SCREEN_SCK)
    });

    // Turbofish DS=8 (taille de trame en bits) : plusieurs impls existent
    // (4/5/8...), rien ne force le choix sans cette annotation explicite.
    let spi =
        Spi::<_, _, _, 8>::new(spi0, (tx, sck)).init(resets, peripheral_freq, SCREEN_SPI_HZ.Hz(), MODE_0);

    // CS/DC/RESET ne passent pas par le périphérique SPI : ce sont de
    // simples GPIO, `ExclusiveDevice` pilotant CS autour de chaque
    // transaction. Force de commande par défaut (4 mA) — ils n'attaquent
    // que des entrées CMOS, cf. `board::configure_output_pin`.
    let cs = board::configure_output_pin(PIN_SCREEN_CS);
    let dc = board::configure_output_pin(PIN_SCREEN_DC);
    let rst = board::configure_output_pin(PIN_SCREEN_RESET);

    // CS::Error = Infallible (GPIO simple) : ne peut pas échouer en pratique.
    let device = ExclusiveDevice::new_no_delay(spi, cs).expect("CS ne peut pas echouer");
    let iface = SPIInterface::new(device, dc);

    defmt::info!(
        "ecran : SCK=GP{} MOSI=GP{} CS=GP{} DC=GP{} RESET=GP{} a {} Hz",
        PIN_SCREEN_SCK,
        PIN_SCREEN_MOSI,
        PIN_SCREEN_CS,
        PIN_SCREEN_DC,
        PIN_SCREEN_RESET,
        SCREEN_SPI_HZ,
    );

    let ili = match Ili9341::new(iface, rst, delay, Orientation::Landscape, DisplaySize240x320) {
        Ok(ili) => ili,
        Err(e) => {
            defmt::error!("echec init ecran : {}", defmt::Debug2Format(&e));
            panic!("ecran indisponible");
        }
    };

    let framebuffer = display::take_framebuffer().expect("le framebuffer n'est reclame qu'ici");
    FramebufferedDisplay::new(ili, framebuffer)
}

/// Arme la scrutation de l'encodeur : configure A/B/SW en entrée pull-up,
/// planifie l'alarme 0 et démasque `TIMER_IRQ_0`.
///
/// À partir de cet appel, les événements s'accumulent dans la file, que
/// l'appelant vide avec [`next_event`] (ou laisse [`Console::pump`] vider).
///
/// # Panics
///
/// Panique si l'alarme 0 est déjà prise — elle appartient à ce module.
pub fn start_encoder(timer: &mut hal::Timer) {
    let encoder = RotaryEncoder::new(
        board::configure_input_pin(PIN_ENCODER_A),
        board::configure_input_pin(PIN_ENCODER_B),
        board::configure_input_pin(PIN_ENCODER_SW),
    );

    let mut alarm = timer.alarm_0().expect("alarme 0 disponible au premier appel");
    alarm.schedule(ENCODER_POLL_MS.millis()).expect("planification initiale valide");
    alarm.enable_interrupt();

    critical_section::with(|cs| {
        ENCODER.borrow(cs).replace(Some(encoder));
        ALARM.borrow(cs).replace(Some(alarm));
    });

    defmt::info!(
        "encodeur : A=GP{} B=GP{} SW=GP{}, scrutation toutes les {} ms",
        PIN_ENCODER_A,
        PIN_ENCODER_B,
        PIN_ENCODER_SW,
        ENCODER_POLL_MS,
    );

    // Sûr : ENCODER et ALARM sont déposés juste au-dessus, avant que
    // l'interruption ne puisse se déclencher.
    unsafe {
        pac::NVIC::unmask(pac::Interrupt::TIMER_IRQ_0);
    }
}

// ─── Consommation des événements ─────────────────────────────────────────────

/// Dépile le plus ancien événement encodeur en attente.
pub fn next_event() -> Option<EncoderEvent> {
    critical_section::with(|cs| EVENTS.borrow(cs).borrow_mut().pop())
}

/// Relève le nombre d'événements perdus par débordement de la file depuis le
/// dernier appel, et le remet à zéro.
pub fn take_dropped_events() -> u32 {
    critical_section::with(|cs| EVENTS.borrow(cs).borrow_mut().take_dropped())
}

// ─── La console complète ─────────────────────────────────────────────────────

/// Écran + encodeur, montés et prêts. Cf. [`init`].
pub struct Console {
    display: OperatorDisplay,
    timer: hal::Timer,
}

/// Monte la console entière — écran puis encodeur.
///
/// L'ordre compte : l'écran d'abord, pour que le premier rendu puisse
/// partir avant que l'encodeur ne commence à produire des événements.
pub fn init(
    spi0: pac::SPI0,
    resets: &mut pac::RESETS,
    peripheral_freq: HertzU32,
    timer: &mut hal::Timer,
) -> Console {
    let display = init_display(spi0, resets, peripheral_freq, timer);
    start_encoder(timer);
    // `hal::Timer` est `Copy` — juste un accès aux registres matériels.
    Console { display, timer: *timer }
}

impl Console {
    /// Applique à `app` tous les événements empilés par l'interruption, et
    /// journalise les pertes éventuelles.
    ///
    /// Rend `true` si au moins un événement a été appliqué — c'est-à-dire
    /// si l'opérateur vient de toucher la molette. `main.rs` s'en sert pour
    /// remettre à zéro le délai de mise en veille ; un appelant qui n'a pas
    /// de veille l'ignore.
    ///
    /// L'état courant est relu à chaque événement : entre deux, le cœur 1
    /// peut avoir fait avancer la machine, et c'est lui qui décide si un
    /// démarrage est encore légitime. Une demande de l'opérateur (clic sur
    /// le premier item du menu) est appliquée ici — `UiApp` ne fait que la
    /// formuler.
    pub fn pump(&mut self, app: &mut UiApp) -> bool {
        let mut any = false;
        while let Some(event) = next_event() {
            any = true;
            let current = critical_section::with(|cs| SHARED_STATE.borrow_ref(cs).task);
            if let Some(task) = app.handle_event(event, current) {
                critical_section::with(|cs| SHARED_STATE.borrow_ref_mut(cs).task = task);
                defmt::info!("demande operateur : nouvel etat systeme");
            }
        }

        let dropped = take_dropped_events();
        if dropped > 0 {
            defmt::warn!("{} evenement(s) encodeur perdus : file pleine", dropped);
        }

        any
    }

    /// Redessine l'écran courant à partir d'un instantané déjà copié.
    ///
    /// **Ne prend aucune section critique** : `app` appartient à
    /// l'appelant et `state` est une copie. C'est ce qui laisse
    /// `TIMER_IRQ_0` scruter l'encodeur pendant tout le rendu — dessin des
    /// bandes compris, pas seulement le transfert SPI — et, sur la machine
    /// complète, ce qui évite de bloquer le cœur 1 sur le spinlock.
    pub fn redraw(&mut self, app: &UiApp, state: &SharedState) {
        let start = self.timer.get_counter();
        let _ = self.display.render(|target| app.draw(target, state));
        let elapsed = self.timer.get_counter() - start;
        defmt::debug!("redraw termine en {} ms", elapsed.to_millis());
    }

    /// L'écran nu, pour les usages qui ne passent pas par [`UiApp`] —
    /// `bin/screen_test.rs` y dessine sa mire de câblage.
    pub fn display(&mut self) -> &mut OperatorDisplay {
        &mut self.display
    }
}
