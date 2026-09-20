//! Point d'entrée du firmware : assemble le matériel réel et lance la
//! boucle de contrôle.
//!
//! Ce fichier est le **point de composition** du projet, et rien d'autre.
//! Il ne contient aucune règle métier : il construit les périphériques,
//! les branche sur les abstractions de `cloud_chamber_hal`, et passe la
//! main à [`logic::control_loop::run`]. Tout ce qui décide de quoi que ce
//! soit — phases, sécurité, régulation, navigation UI — vit ailleurs et
//! est testé sur hôte.
//!
//! # Répartition entre les deux cœurs
//!
//! - **Cœur 1 : la boucle de contrôle.** `control_loop::run()` y possède le
//!   cœur — sondage des capteurs, machine à états, sécurité, actionneurs.
//!   Sa cadence est dominée par le bit-banging 1-Wire des DS18B20.
//! - **Cœur 0 : l'interface.** Scrutation de l'encodeur sous interruption
//!   `TIMER_IRQ_0` (1 ms), puis boucle d'affichage. Son rythme est
//!   totalement découplé de celui du contrôle : l'écran ne ralentit plus
//!   quand une lecture de température s'éternise.
//!
//! Les deux cœurs ne se parlent qu'à travers `shared::data::SHARED_STATE` et
//! `shared::settings`, tous deux protégés par `critical_section`.
//!
//! ## Le piège à connaître : le spinlock est global
//!
//! Sur RP2040, `critical-section-impl` n'est pas un simple masquage
//! d'interruptions : c'est un **spinlock matériel partagé par les deux
//! cœurs**. Une section critique tenue longtemps sur un cœur met l'autre en
//! attente active. C'est pourquoi rien ici ne dessine ni ne fait d'E/S sous
//! verrou :
//!
//! - l'interruption encodeur ne fait qu'empiler l'événement dans [`EVENTS`]
//!   et rend la main ;
//! - la boucle d'affichage prend une **copie** de `SharedState` sous verrou
//!   court, puis dessine hors verrou ;
//! - [`UiApp`] n'est plus partagé du tout : il appartient à la boucle du
//!   cœur 0, qui est seule à le toucher.
//!
//! Tenir le verrou pendant un rendu plein écran (des dizaines de ms)
//! bloquerait le cœur 1 d'autant — y compris au milieu d'une séquence
//! 1-Wire, dont le décodage dépend d'un timing à la microseconde.
//!
//! # Ce qui n'est pas encore câblé ici
//!
//! - **Persistance des réglages.** L'écran de réglages lève bien une
//!   demande de sauvegarde ([`UiApp::take_save_request`]), mais aucune
//!   implémentation de `drivers::flash_store::FlashOps` n'existe pour le
//!   RP2040 (écriture flash depuis la RAM, interruptions coupées). La
//!   demande est donc journalisée puis abandonnée : les réglages modifiés
//!   s'appliquent immédiatement (via `shared::settings`) mais ne survivent
//!   pas à une coupure.
//!
//! # Construction
//!
//! RP2040 uniquement, comme les bins de bring-up, et derrière la feature
//! `bin-cloud-chamber` pour que les jobs CI `cargo check` sur les autres
//! cibles ne tentent pas de le construire :
//!
//! ```text
//! cargo run --release --target thumbv6m-none-eabi \
//!     --features bin-cloud-chamber --bin cloud_chamber
//! ```
#![no_std]
#![no_main]

use core::cell::RefCell;

use defmt_rtt as _;
use panic_probe as _;

use critical_section::Mutex;
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::MODE_0;
use rp2040_hal::{
    Clock, I2C, self as hal,
    fugit::{ExtU32, RateExtU32},
    gpio::{
        DynBankId, DynPinId, DynPullType, FunctionI2c, FunctionSio, FunctionSpi, Pin, PullUp, SioInput, new_pin,
    },
    i2c::{ValidatedPinScl, ValidatedPinSda},
    multicore::{Multicore, Stack},
    pac::{self, interrupt},
    spi::{Spi, ValidatedPinSck, ValidatedPinTx},
    timer::{Alarm, Alarm0},
};

use display_interface_spi::SPIInterface;
use embedded_hal_bus::spi::ExclusiveDevice;
use ili9341::{DisplaySize240x320, Ili9341, Orientation};

use cloud_chamber_firmware::board;
use cloud_chamber_firmware::cloud_chamber_hal::timer::Duration;
use cloud_chamber_firmware::config::settings::{Settings, SettingsStore};
use cloud_chamber_firmware::drivers::flash_rp2040;
use cloud_chamber_firmware::drivers::flash_store::{self, FlashSettingsStore};
use cloud_chamber_firmware::logic::persistence;
use cloud_chamber_firmware::shared::settings;
use cloud_chamber_firmware::cloud_chamber_hal::actuators::Actuators;
use cloud_chamber_firmware::cloud_chamber_hal::sensors::{IndependentSensors, Sensors};
use cloud_chamber_firmware::config::operating::REGULATION_BAND_C;
use cloud_chamber_firmware::config::wiring::{
    PIN_COMPRESSOR_RELAY, PIN_ENCODER_A,
    PIN_ENCODER_B, PIN_ENCODER_SW, PIN_HV_RELAY, PIN_I2C_SCL, PIN_I2C_SDA, PIN_ISO_HEATER_RELAY,
    PIN_LIGHTS_RELAY, PIN_ONEWIRE, PIN_PUMP_RELAY, PIN_SCREEN_CS, PIN_SCREEN_DC, PIN_SCREEN_MOSI,
    PIN_SCREEN_RESET, PIN_SCREEN_SCK, PIN_WINDOW_HEATER_RELAY,
};
use cloud_chamber_firmware::drivers::bme280::{Bme280Driver, Bme280Sensor};
use cloud_chamber_firmware::drivers::breaker::GpioBreaker;
use cloud_chamber_firmware::drivers::compressor::Compressor;
use cloud_chamber_firmware::drivers::display::{self, FramebufferedDisplay};
use cloud_chamber_firmware::drivers::ds18b20::{
    Ds18b20Bus, Ds18b20Sensors, Resolution, rp2040_adapter::Rp2040OpenDrain,
};
use cloud_chamber_firmware::drivers::encoder::{EncoderEvent, RotaryEncoder};
use cloud_chamber_firmware::drivers::heater::Heater;
use cloud_chamber_firmware::drivers::lights::Lights;
use cloud_chamber_firmware::drivers::pump::Pump;
use cloud_chamber_firmware::drivers::window_heater::WindowHeater;
use cloud_chamber_firmware::logic::control_loop;
use cloud_chamber_firmware::shared::data::{SHARED_STATE, SharedState, SystemTask};
use cloud_chamber_firmware::ui::app::UiApp;
use cloud_chamber_firmware::ui::navigator::Screen;

/// Vitesse du bus SPI de l'écran — cf. `bin/ui_test.rs` pour le
/// raisonnement sur cette valeur et le symptôme d'un réglage trop haut.
const SCREEN_SPI_HZ: u32 = 32_000_000;

/// Profondeur de la file d'événements encodeur — cf. [`EventQueue`].
const EVENT_QUEUE_LEN: usize = 32;

/// Taille de la pile du cœur 1, en mots de 32 bits (soit 8 Ko).
///
/// La boucle de contrôle n'y alloue rien de volumineux : historique de
/// mesures et état de phase sont des structures de quelques centaines
/// d'octets, et l'appel le plus profond est le bit-banging 1-Wire. 8 Ko
/// laisse une marge confortable sans mordre inutilement sur la RAM.
const CORE1_STACK_WORDS: usize = 2048;

/// Résolution des DS18B20. 12 bits est la valeur usine ; c'est aussi la
/// plus lente à convertir, mais `probe()` ne bloque pas dessus (conversion
/// lancée à un tour, résultat lu au suivant).
const TEMP_RESOLUTION: Resolution = Resolution::Bits12;

// ─── État partagé avec l'interruption TIMER_IRQ_0 ──────────────────────────

type EncPin = Pin<DynPinId, FunctionSio<SioInput>, DynPullType>;
type Encoder = RotaryEncoder<EncPin, EncPin, EncPin>;

/// `None` jusqu'à ce que `main()` y dépose l'encodeur — l'interruption
/// n'étant démasquée qu'après, l'ISR ne peut pas observer ce `None`.
static ENCODER: Mutex<RefCell<Option<Encoder>>> = Mutex::new(RefCell::new(None));
static ALARM: Mutex<RefCell<Option<Alarm0>>> = Mutex::new(RefCell::new(None));

/// Événements d'encodeur en attente de traitement par la boucle du cœur 0.
///
/// L'interruption ne fait qu'empiler ici ; c'est la boucle qui les applique
/// à `UiApp`. Ce découplage est ce qui permet à `UiApp` de n'être partagé
/// avec personne (donc de se dessiner hors section critique) tout en
/// gardant une scrutation à 1 ms qui ne rate jamais un cran.
static EVENTS: Mutex<RefCell<EventQueue>> = Mutex::new(RefCell::new(EventQueue::new()));

/// Pile du cœur 1. Vit en `.bss`, donc prise sur la RAM restante — la pile
/// du cœur 0 garde tout le bas de la RAM, où loge le framebuffer de 150 Ko.
static CORE1_STACK: Stack<CORE1_STACK_WORDS> = Stack::new();

/// File circulaire d'événements encodeur, à taille fixe.
///
/// 32 places : à 1 ms de scrutation et un rendu de quelques dizaines de ms,
/// une rotation même rapide en produit une poignée entre deux passages de
/// la boucle. Le débordement est compté et journalisé plutôt que silencieux
/// — perdre un cran doit se voir.
struct EventQueue {
    buffer: [EncoderEvent; EVENT_QUEUE_LEN],
    head: usize,
    len: usize,
    dropped: u32,
}

impl EventQueue {
    const fn new() -> Self {
        Self {
            buffer: [EncoderEvent::None; EVENT_QUEUE_LEN],
            head: 0,
            len: 0,
            dropped: 0,
        }
    }

    /// Empile un événement. Sur file pleine, le nouvel événement est
    /// abandonné (plutôt que d'écraser le plus ancien) : réordonner les
    /// entrées serait pire que d'en perdre une, un clic ne doit jamais
    /// doubler une rotation qui l'a précédé.
    fn push(&mut self, event: EncoderEvent) {
        if self.len == EVENT_QUEUE_LEN {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        let tail = (self.head + self.len) % EVENT_QUEUE_LEN;
        self.buffer[tail] = event;
        self.len += 1;
    }

    fn pop(&mut self) -> Option<EncoderEvent> {
        if self.len == 0 {
            return None;
        }
        let event = self.buffer[self.head];
        self.head = (self.head + 1) % EVENT_QUEUE_LEN;
        self.len -= 1;
        Some(event)
    }

    /// Relève le compteur de débordements et le remet à zéro.
    fn take_dropped(&mut self) -> u32 {
        core::mem::take(&mut self.dropped)
    }
}

/// Scrutation de l'encodeur, toutes les 1 ms sur le cœur 0. Ne fait
/// qu'empiler : aucune section critique longue, donc aucun risque de faire
/// attendre le cœur 1 sur le spinlock (cf. doc de module).
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

        // `None` est le cas de très loin le plus fréquent (1000 scrutations
        // par seconde) : ne rien empiler évite de saturer la file pour rien.
        if event != EncoderEvent::None {
            EVENTS.borrow(cs).borrow_mut().push(event);
        }
    });
}

#[hal::entry]
fn main() -> ! {
    let mut board = board::init();

    defmt::info!("cloud-chamber : demarrage");

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

    // Turbofish DS=8 (taille de trame) : plusieurs impls existent (4/5/8...).
    let spi = Spi::<_, _, _, 8>::new(board.spi0, (tx, sck)).init(
        &mut board.resets,
        board.clocks.peripheral_clock.freq(),
        SCREEN_SPI_HZ.Hz(),
        MODE_0,
    );

    let cs_pin = board::configure_output_pin(PIN_SCREEN_CS);
    let dc = board::configure_output_pin(PIN_SCREEN_DC);
    let rst = board::configure_output_pin(PIN_SCREEN_RESET);

    // CS::Error = Infallible (GPIO simple) : ne peut pas échouer en pratique.
    let spi_device = ExclusiveDevice::new_no_delay(spi, cs_pin).unwrap();
    let iface = SPIInterface::new(spi_device, dc);

    let ili = match Ili9341::new(iface, rst, &mut board.timer, Orientation::Landscape, DisplaySize240x320)
    {
        Ok(display) => display,
        Err(e) => {
            // L'écran est le seul canal d'information de l'opérateur : sans
            // lui, démarrer un cycle serait piloter à l'aveugle. On s'arrête
            // plutôt que de continuer en silence.
            defmt::error!("echec init ecran : {}", defmt::Debug2Format(&e));
            panic!("ecran indisponible");
        }
    };
    let framebuffer = display::take_framebuffer().expect("le framebuffer n'est reclame qu'ici");
    let mut display = FramebufferedDisplay::new(ili, framebuffer);

    // ─── Encodeur (A/B/SW, pull-up interne) — piloté par interruption ──────
    let encoder = RotaryEncoder::new(
        board::configure_input_pin(PIN_ENCODER_A),
        board::configure_input_pin(PIN_ENCODER_B),
        board::configure_input_pin(PIN_ENCODER_SW),
    );

    let mut alarm = board.timer.alarm_0().expect("alarme 0 disponible au premier appel");
    alarm.schedule(1_u32.millis()).expect("planification initiale valide");
    alarm.enable_interrupt();

    critical_section::with(|cs| {
        ENCODER.borrow(cs).replace(Some(encoder));
        ALARM.borrow(cs).replace(Some(alarm));
    });

    // `UiApp` n'est volontairement pas un static : il appartient à la
    // boucle du cœur 0, seule à le toucher. L'interruption ne communique
    // avec lui que par la file d'événements.
    let mut app = UiApp::new();

    // Sûr : ENCODER/ALARM sont déposés juste au-dessus, avant que
    // l'interruption ne puisse se déclencher.
    unsafe {
        pac::NVIC::unmask(pac::Interrupt::TIMER_IRQ_0);
    }

    // Premier rendu : l'opérateur voit le menu pendant la découverte des
    // capteurs, qui prend un instant.
    let initial_state = critical_section::with(|cs| *SHARED_STATE.borrow_ref(cs));
    redraw(&mut display, &app, &initial_state, board.timer);
    app.take_redraw_request();

    // ─── Températures : DS18B20 sur 1-Wire ─────────────────────────────────
    board::configure_onewire_pin(PIN_ONEWIRE);
    let mut bus = Ds18b20Bus::new(Rp2040OpenDrain::new(1u32 << PIN_ONEWIRE));
    let discovered = bus.discover(&mut board.timer);
    defmt::info!(
        "1-Wire GP{} : {} capteur(s) decouvert(s) (attendu : {})",
        PIN_ONEWIRE,
        discovered,
        cloud_chamber_firmware::cloud_chamber_hal::config::NUMBER_OF_TEMP_SENSOR,
    );

    // `Ds18b20Sensors::new` configure la résolution de chaque capteur
    // découvert ; un échec ici veut dire que le bus ne répond pas comme
    // attendu, ce qui rend toute lecture de température douteuse.
    let temperature_source = match Ds18b20Sensors::new(bus, board.timer, board.timer, TEMP_RESOLUTION) {
        Ok(sensors) => sensors,
        Err(e) => {
            defmt::error!("echec config DS18B20 : {}", defmt::Debug2Format(&e));
            panic!("bus 1-Wire inutilisable");
        }
    };

    // ─── Pression : BME280 sur I²C0 ────────────────────────────────────────
    let sda = unsafe { new_pin(DynPinId { bank: DynBankId::Bank0, num: PIN_I2C_SDA }) }
        .try_into_function::<FunctionI2c>()
        .ok()
        .expect("I2C est une fonction valide sur cette broche")
        .into_pull_type::<PullUp>();
    let scl = unsafe { new_pin(DynPinId { bank: DynBankId::Bank0, num: PIN_I2C_SCL }) }
        .try_into_function::<FunctionI2c>()
        .ok()
        .expect("I2C est une fonction valide sur cette broche")
        .into_pull_type::<PullUp>();

    let sda = ValidatedPinSda::validate(sda, &board.i2c0).unwrap_or_else(|_| {
        panic!("PIN_I2C_SDA (GP{}) n'est pas une broche SDA valide pour I2C0", PIN_I2C_SDA)
    });
    let scl = ValidatedPinScl::validate(scl, &board.i2c0).unwrap_or_else(|_| {
        panic!("PIN_I2C_SCL (GP{}) n'est pas une broche SCL valide pour I2C0", PIN_I2C_SCL)
    });

    let i2c = I2C::new_controller(board.i2c0, sda, scl, 400.kHz(), &mut board.resets, board.clocks.system_clock.freq());

    // Source de pression : BME280, pas ABP2. C'est ce qui est câblé sur ce
    // montage — l'ABP2 (pression d'un circuit de la chambre, 0–1 bar) n'y
    // est pas monté. Le BME280 mesure la pression **atmosphérique absolue**
    // (~1013 hPa) : il remplit le créneau `press` mais ne décrit pas la même
    // grandeur, et une sécurité pression ajoutée un jour devra en tenir
    // compte. Le driver ABP2 reste dans l'arbre, prêt à reprendre ce rôle.
    let mut bme = Bme280Sensor::new(Bme280Driver::new(i2c), board.timer, board.timer);
    if let Err(e) = bme.init() {
        // Sans init, les coefficients de compensation ne sont pas chargés et
        // toutes les lectures seraient fausses — mieux vaut le dire ici que
        // laisser `control_loop` paniquer sur un capteur muet.
        defmt::error!("echec init BME280 (adresse 0x76) : {}", defmt::Debug2Format(&e));
        panic!("capteur de pression indisponible");
    }
    defmt::info!("BME280 initialise — source de pression");

    let pressure_source = IndependentSensors([bme]);

    let sensors = Sensors::new(temperature_source, pressure_source);

    // ─── Actionneurs ───────────────────────────────────────────────────────
    //
    // Chaque relais démarre à l'arrêt : les constructeurs forcent la broche
    // au niveau bas avant tout. C'est ce qui garantit qu'un reset en plein
    // cycle ne laisse pas la haute tension ou le compresseur collés.
    let actuators = Actuators {
        high_voltage: GpioBreaker::new(board::configure_relay_pin(PIN_HV_RELAY), true),
        cooling: Compressor::new(
            board::configure_relay_pin(PIN_COMPRESSOR_RELAY),
            REGULATION_BAND_C,
        ),
        iso_heater: Heater::new(
            board::configure_relay_pin(PIN_ISO_HEATER_RELAY),
            REGULATION_BAND_C,
        ),
        iso_pump: Pump::new(board::configure_relay_pin(PIN_PUMP_RELAY)),
        lights: Lights::new(board::configure_relay_pin(PIN_LIGHTS_RELAY)),
        glass_heater: WindowHeater::new(board::configure_relay_pin(PIN_WINDOW_HEATER_RELAY)),
    };

    // ─── Cœur 1 : la boucle de contrôle ───────────────────────────────────
    //
    // Capteurs et actionneurs sont *déplacés* sur le cœur 1, qui en devient
    // seul propriétaire : aucun partage, donc aucun verrou sur le chemin
    // chaud du contrôle. Tout ce qui traverse est dans `SHARED_STATE`.
    // Le bloc libère `board.fifo` : `Multicore` ne l'emprunte que le temps
    // du lancement, et le cœur 0 en a besoin ensuite pour garer le cœur 1
    // pendant les écritures flash.
    {
        let mut multicore = Multicore::new(&mut board.psm, &mut board.ppb, &mut board.fifo);
        let core1 = &mut multicore.cores()[1];
        let stack = CORE1_STACK
            .take()
            .expect("la pile du coeur 1 n'est reclamee qu'ici");

        if let Err(e) = core1.spawn(stack, move || {
            // Le point où le cœur 0 peut réclamer les deux cœurs, le temps
            // d'une écriture flash — cf. `drivers::flash_rp2040`.
            control_loop::run(sensors, actuators, board.timer, flash_rp2040::park_if_requested);
        }) {
            defmt::error!("echec lancement coeur 1 : {}", defmt::Debug2Format(&e));
            panic!("boucle de controle indisponible");
        }
    }

    defmt::info!("cloud-chamber : coeur 1 lance, UI sur coeur 0");

    // ─── Réglages persistants ─────────────────────────────────────────────
    //
    // Après le lancement du cœur 1, parce que le store a besoin de la FIFO.
    // Sans conséquence : la boucle de contrôle démarre sur `Idle`, toutes
    // sorties coupées, et relit `shared::settings` à chaque tour — elle
    // prendra les valeurs relues dès le tour suivant.
    let settings_offset = flash_rp2040::settings_offset();
    debug_assert_eq!(
        flash_rp2040::settings_len() as usize,
        flash_store::SECTOR_SIZE,
        "rp2040.x doit reserver exactement un secteur"
    );
    defmt::info!("secteur reglages a {:#x}", settings_offset);

    let mut store =
        FlashSettingsStore::new(flash_rp2040::Rp2040Flash::new(board.fifo), settings_offset);
    match store.load() {
        Some(saved) => {
            settings::set(saved);
            defmt::info!("reglages relus depuis la flash");
        }
        None => defmt::info!("aucun reglage en flash, valeurs par defaut"),
    }

    // Demande de sauvegarde acceptée mais pas encore écrite : le secteur
    // est plein et la machine tourne, donc l'effacement attend l'arrêt. Le
    // réglage lui-même est déjà appliqué (`shared::settings`) — seule sa
    // survie à une coupure est différée.
    let mut pending_save: Option<Settings> = None;

    // ─── Cœur 0 : la boucle d'interface ───────────────────────────────────
    //
    // Rien ici ne dessine sous section critique : on prend une copie de
    // l'état, puis on travaille dessus verrou relâché (cf. doc de module).
    let mut last_task = SystemTask::Idle;
    let mut last_activity = board.timer.get_counter();

    loop {
        // Applique les événements empilés par l'interruption. L'état
        // courant est relu à chaque fois : entre deux événements, le cœur 1
        // peut avoir fait avancer la machine, et c'est lui qui décide si un
        // démarrage est encore légitime.
        while let Some(event) = critical_section::with(|cs| EVENTS.borrow(cs).borrow_mut().pop()) {
            last_activity = board.timer.get_counter();
            let current = critical_section::with(|cs| SHARED_STATE.borrow_ref(cs).task);
            if let Some(task) = app.handle_event(event, current) {
                critical_section::with(|cs| SHARED_STATE.borrow_ref_mut(cs).task = task);
                defmt::info!("demande operateur : nouvel etat systeme");
            }
        }

        let dropped = critical_section::with(|cs| EVENTS.borrow(cs).borrow_mut().take_dropped());
        if dropped > 0 {
            defmt::warn!("{} evenement(s) encodeur perdus : file pleine", dropped);
        }

        // Copie de l'état sous verrou court, et acquittement des nouvelles
        // mesures dans le même geste — le dessin qui suit se fait dessus,
        // hors verrou.
        let state = critical_section::with(|cs| {
            let mut shared = SHARED_STATE.borrow_ref_mut(cs);
            let copy = *shared;
            shared.new_data = false;
            copy
        });

        // Un changement de phase décidé par le cœur 1 ne passe par aucun
        // événement d'encodeur : sans ça l'écran de suivi resterait figé
        // jusqu'au prochain geste de l'opérateur.
        if state.task != last_task {
            last_task = state.task;
            app.mark_dirty();
        }

        // Le graphe de veille se remplit quel que soit l'écran affiché,
        // sinon la veille s'ouvrirait sur un cadre vide.
        //
        // Le redessin, lui, ne concerne que les écrans qui montrent des
        // mesures. Le menu était exclu tant que sa bande du bas était vide ;
        // elle porte maintenant les températures et l'état des actionneurs,
        // qui resteraient figés jusqu'au prochain geste de l'opérateur.
        if state.new_data {
            app.sample(&state);
            if matches!(
                app.current_screen(),
                Screen::Stats | Screen::CurrentTask | Screen::MainMenu | Screen::Idle
            ) {
                app.mark_dirty();
            }
        }

        app.poll_idle(Duration::from_micros(
            (board.timer.get_counter() - last_activity).to_micros(),
        ));

        // Une nouvelle demande remplace celle qui attendait : c'est la
        // dernière valeur voulue par l'opérateur qui compte, pas la
        // première.
        if let Some(wanted) = app.take_save_request() {
            pending_save = Some(wanted);
        }

        if let Some(wanted) = pending_save {
            let task = critical_section::with(|cs| SHARED_STATE.borrow_ref(cs).task);
            match persistence::decide(store.next_save_cost(), task) {
                persistence::SaveDecision::Now => {
                    match store.save(&wanted) {
                        Ok(()) => defmt::info!("reglages sauvegardes"),
                        Err(e) => {
                            defmt::error!("sauvegarde impossible : {}", defmt::Debug2Format(&e))
                        }
                    }
                    // Consommée dans les deux cas : réessayer en boucle
                    // userait la flash sans rien changer au défaut.
                    pending_save = None;
                }
                persistence::SaveDecision::Defer => {}
            }
        }
        app.set_save_pending(pending_save.is_some());

        if app.take_redraw_request() {
            redraw(&mut display, &app, &state, board.timer);
        }
    }
}

/// Redessine l'écran courant, à partir d'un instantané déjà copié.
///
/// Ne prend aucune section critique : `app` appartient au cœur 0 et `state`
/// est une copie. C'est ce qui garantit qu'un rendu plein écran — des
/// dizaines de millisecondes — ne fait jamais attendre le cœur 1 sur le
/// spinlock global (cf. doc de module).
fn redraw<IFACE, RESET>(
    display: &mut FramebufferedDisplay<IFACE, RESET>,
    app: &UiApp,
    state: &SharedState,
    timer: hal::Timer,
) where
    IFACE: display_interface::WriteOnlyDataCommand,
    RESET: OutputPin,
{
    let start = timer.get_counter();
    let _ = display.render(|target| app.draw(target, state));
    let elapsed = timer.get_counter() - start;
    defmt::debug!("redraw termine en {} ms", elapsed.to_millis());
}

