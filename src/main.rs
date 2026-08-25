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
//! - **Réarmement sécurité.** `SafetyMonitor::reset()` n'a toujours aucun
//!   appelant : après un déclenchement, seul un reflash repart. Aucun
//!   écran ne l'expose encore.
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
    Clock, I2C, Sio, Watchdog, self as hal,
    clocks::init_clocks_and_plls,
    fugit::{ExtU32, RateExtU32},
    gpio::{
        DynBankId, DynPinId, DynPullType, FunctionI2c, FunctionSio, FunctionSpi,
        OutputDriveStrength, Pin, Pins, PullUp, SioInput, SioOutput, new_pin,
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

use cloud_chamber_firmware::cloud_chamber_hal::actuators::Actuators;
use cloud_chamber_firmware::cloud_chamber_hal::sensors::{IndependentSensors, Sensors};
use cloud_chamber_firmware::cloud_chamber_hal::units::Celsius;
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

/// Fréquence du cristal externe du Pico — cf. `hal::clocks::init_clocks_and_plls`.
const XOSC_CRYSTAL_FREQ: u32 = 12_000_000;

/// Vitesse du bus SPI de l'écran — cf. `bin/ui_test.rs` pour le
/// raisonnement sur cette valeur et le symptôme d'un réglage trop haut.
const SCREEN_SPI_HZ: u32 = 32_000_000;

/// Force de commande des sorties pilotant les optocoupleurs MOC3043.
///
/// **Le RP2040 démarre à 4 mA** (champ `DRIVE` de `PADS_BANK0.GPIO`, valeur
/// de reset `0x56`), ce qui est insuffisant ici : il faut au moins 5 mA
/// dans la LED du MOC3043 pour garantir l'amorçage (`IFT` max), et on vise
/// en pratique ~10 mA de marge.
///
/// Attention au sens de ce réglage : ce n'est pas une limite de courant,
/// c'est la capacité de la sortie. Le courant réel est fixé par la
/// résistance série ; ce champ décide seulement à partir de quel courant la
/// tension de sortie s'effondre. À 4 mA, tirer ~10 mA fait chuter `VOH`
/// assez pour que l'amorçage devienne aléatoire — d'où ce passage à 8 mA.
///
/// Si la résistance série vise franchement plus de 8 mA, passer à
/// [`OutputDriveStrength::TwelveMilliAmps`] : c'est le seul changement à
/// faire, toutes les sorties de puissance passent par cette constante.
const RELAY_DRIVE_STRENGTH: OutputDriveStrength = OutputDriveStrength::EightMilliAmps;

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

#[unsafe(link_section = ".boot2")]
#[used]
static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

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

    // `Timer` est `Copy` : la même horloge sert de source monotone à la
    // boucle de contrôle, aux horodatages de mesure, et de source de délai
    // au bit-banging 1-Wire — sans avoir à la partager derrière un mutex.
    let mut timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

    let mut sio = Sio::new(pac.SIO);
    // `Pins::new` reste nécessaire même si son API typée n'est pas utilisée
    // ensuite : c'est cet appel qui sort IO_BANK0/PADS_BANK0 de reset.
    let _pins = Pins::new(pac.IO_BANK0, pac.PADS_BANK0, sio.gpio_bank0, &mut pac.RESETS);

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

    let tx = ValidatedPinTx::validate(tx, &pac.SPI0).unwrap_or_else(|_| {
        panic!("PIN_SCREEN_MOSI (GP{}) n'est pas une broche Tx/MOSI valide pour SPI0", PIN_SCREEN_MOSI)
    });
    let sck = ValidatedPinSck::validate(sck, &pac.SPI0).unwrap_or_else(|_| {
        panic!("PIN_SCREEN_SCK (GP{}) n'est pas une broche Sck valide pour SPI0", PIN_SCREEN_SCK)
    });

    // Turbofish DS=8 (taille de trame) : plusieurs impls existent (4/5/8...).
    let spi = Spi::<_, _, _, 8>::new(pac.SPI0, (tx, sck)).init(
        &mut pac.RESETS,
        clocks.peripheral_clock.freq(),
        SCREEN_SPI_HZ.Hz(),
        MODE_0,
    );

    let cs_pin = configure_output_pin(PIN_SCREEN_CS);
    let dc = configure_output_pin(PIN_SCREEN_DC);
    let rst = configure_output_pin(PIN_SCREEN_RESET);

    // CS::Error = Infallible (GPIO simple) : ne peut pas échouer en pratique.
    let spi_device = ExclusiveDevice::new_no_delay(spi, cs_pin).unwrap();
    let iface = SPIInterface::new(spi_device, dc);

    let ili = match Ili9341::new(iface, rst, &mut timer, Orientation::Landscape, DisplaySize240x320)
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
        configure_input_pin(PIN_ENCODER_A),
        configure_input_pin(PIN_ENCODER_B),
        configure_input_pin(PIN_ENCODER_SW),
    );

    let mut alarm = timer.alarm_0().expect("alarme 0 disponible au premier appel");
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
    redraw(&mut display, &app, &initial_state, timer);
    app.take_redraw_request();

    // ─── Températures : DS18B20 sur 1-Wire ─────────────────────────────────
    configure_onewire_pin(PIN_ONEWIRE);
    let mut bus = Ds18b20Bus::new(Rp2040OpenDrain::new(1u32 << PIN_ONEWIRE));
    let discovered = bus.discover(&mut timer);
    defmt::info!(
        "1-Wire GP{} : {} capteur(s) decouvert(s) (attendu : {})",
        PIN_ONEWIRE,
        discovered,
        cloud_chamber_firmware::cloud_chamber_hal::config::NUMBER_OF_TEMP_SENSOR,
    );

    // `Ds18b20Sensors::new` configure la résolution de chaque capteur
    // découvert ; un échec ici veut dire que le bus ne répond pas comme
    // attendu, ce qui rend toute lecture de température douteuse.
    let temperature_source = match Ds18b20Sensors::new(bus, timer, timer, TEMP_RESOLUTION) {
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

    let sda = ValidatedPinSda::validate(sda, &pac.I2C0).unwrap_or_else(|_| {
        panic!("PIN_I2C_SDA (GP{}) n'est pas une broche SDA valide pour I2C0", PIN_I2C_SDA)
    });
    let scl = ValidatedPinScl::validate(scl, &pac.I2C0).unwrap_or_else(|_| {
        panic!("PIN_I2C_SCL (GP{}) n'est pas une broche SCL valide pour I2C0", PIN_I2C_SCL)
    });

    let i2c = I2C::new_controller(pac.I2C0, sda, scl, 400.kHz(), &mut pac.RESETS, clocks.system_clock.freq());

    // Source de pression : BME280, pas ABP2. C'est ce qui est câblé sur ce
    // montage — l'ABP2 (pression d'un circuit de la chambre, 0–1 bar) n'y
    // est pas monté. Le BME280 mesure la pression **atmosphérique absolue**
    // (~1013 hPa) : il remplit le créneau `press` mais ne décrit pas la même
    // grandeur, et une sécurité pression ajoutée un jour devra en tenir
    // compte. Le driver ABP2 reste dans l'arbre, prêt à reprendre ce rôle.
    let mut bme = Bme280Sensor::new(Bme280Driver::new(i2c), timer, timer);
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
        high_voltage: GpioBreaker::new(configure_relay_pin(PIN_HV_RELAY), true),
        cooling: Compressor::new(
            configure_relay_pin(PIN_COMPRESSOR_RELAY),
            Celsius(REGULATION_BAND_C),
        ),
        iso_heater: Heater::new(
            configure_relay_pin(PIN_ISO_HEATER_RELAY),
            Celsius(REGULATION_BAND_C),
        ),
        iso_pump: Pump::new(configure_relay_pin(PIN_PUMP_RELAY)),
        lights: Lights::new(configure_relay_pin(PIN_LIGHTS_RELAY)),
        glass_heater: WindowHeater::new(configure_relay_pin(PIN_WINDOW_HEATER_RELAY)),
    };

    // ─── Cœur 1 : la boucle de contrôle ───────────────────────────────────
    //
    // Capteurs et actionneurs sont *déplacés* sur le cœur 1, qui en devient
    // seul propriétaire : aucun partage, donc aucun verrou sur le chemin
    // chaud du contrôle. Tout ce qui traverse est dans `SHARED_STATE`.
    let mut multicore = Multicore::new(&mut pac.PSM, &mut pac.PPB, &mut sio.fifo);
    let core1 = &mut multicore.cores()[1];
    let stack = CORE1_STACK
        .take()
        .expect("la pile du coeur 1 n'est reclamee qu'ici");

    if let Err(e) = core1.spawn(stack, move || {
        control_loop::run(sensors, actuators, timer);
    }) {
        defmt::error!("echec lancement coeur 1 : {}", defmt::Debug2Format(&e));
        panic!("boucle de controle indisponible");
    }

    defmt::info!("cloud-chamber : coeur 1 lance, UI sur coeur 0");

    // ─── Cœur 0 : la boucle d'interface ───────────────────────────────────
    //
    // Rien ici ne dessine sous section critique : on prend une copie de
    // l'état, puis on travaille dessus verrou relâché (cf. doc de module).
    let mut last_task = SystemTask::Idle;

    loop {
        // Applique les événements empilés par l'interruption. L'état
        // courant est relu à chaque fois : entre deux événements, le cœur 1
        // peut avoir fait avancer la machine, et c'est lui qui décide si un
        // démarrage est encore légitime.
        while let Some(event) = critical_section::with(|cs| EVENTS.borrow(cs).borrow_mut().pop()) {
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

        // Les mesures fraîches ne justifient un redessin que sur les écrans
        // qui les affichent — redessiner le menu à chaque cycle de sondage
        // ne ferait que consommer le cœur 0 pour rien.
        if state.new_data && matches!(app.current_screen(), Screen::Stats | Screen::CurrentTask) {
            app.mark_dirty();
        }

        // Faute d'implémentation flash pour le RP2040, une demande de
        // sauvegarde est consommée et journalisée — sinon elle resterait en
        // attente indéfiniment. Cf. doc de module.
        if app.take_save_request().is_some() {
            defmt::warn!("sauvegarde des reglages demandee mais pas encore implementee");
        }

        if app.take_redraw_request() {
            redraw(&mut display, &app, &state, timer);
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

/// Configure GP`pin` en sortie de puissance : comme
/// [`configure_output_pin`], mais à [`RELAY_DRIVE_STRENGTH`] au lieu des
/// 4 mA par défaut du RP2040 — cf. la doc de cette constante pour le
/// pourquoi (amorçage des MOC3043).
///
/// # Safety
/// Même raisonnement que [`configure_output_pin`].
fn configure_relay_pin(pin: u8) -> Pin<DynPinId, FunctionSio<SioOutput>, DynPullType> {
    let mut out = configure_output_pin(pin);
    out.set_drive_strength(RELAY_DRIVE_STRENGTH);

    // Relecture du registre : le seul moyen de vérifier sur la puce réelle
    // que le champ `DRIVE` a bien pris, plutôt que de le supposer. Un
    // `warn` plutôt qu'un `panic` — une force de commande inattendue rend
    // l'amorçage douteux, pas le démarrage impossible, et l'opérateur doit
    // pouvoir voir l'anomalie plutôt que de se retrouver devant une carte
    // muette.
    let readback = out.get_drive_strength();
    if readback == RELAY_DRIVE_STRENGTH {
        defmt::debug!("GP{} : force de commande {}", pin, defmt::Debug2Format(&readback));
    } else {
        defmt::warn!(
            "GP{} : force de commande {} au lieu de {} — amorcage MOC3043 incertain",
            pin,
            defmt::Debug2Format(&readback),
            defmt::Debug2Format(&RELAY_DRIVE_STRENGTH),
        );
    }

    out
}

/// Configure GP`pin` en sortie push-pull logicielle, démarrée à l'état bas.
///
/// Garde la force de commande par défaut (4 mA). Convient aux broches
/// CS/DC/RESET de l'écran, qui n'attaquent que des entrées CMOS : y
/// augmenter la force ne servirait à rien et aggraverait les rebonds et le
/// rayonnement sur des signaux voisins d'un bus SPI à 32 MHz. Les sorties
/// de puissance passent par [`configure_relay_pin`].
///
/// # Safety
/// `new_pin` exige qu'aucune autre instance de `Pin` pour cette broche
/// n'existe en parallèle. `Pins::new(...)` (appelé plus haut pour ses effets
/// de bord de sortie de reset) réserve bien un champ typé `pins.gpio<N>`,
/// mais aucun de ces champs n'est lu ni écrit dans ce fichier : aucun accès
/// concurrent réel aux registres n'en résulte. L'unicité des numéros est
/// elle-même garantie à la compilation par `config::wiring`.
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

/// Configure GP`pin` en entrée avec pull-up interne (broches encodeur).
///
/// # Safety
/// Même raisonnement que [`configure_output_pin`].
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

/// Prépare GP`pin` pour l'usage 1-Wire : sortie niveau bas, puis entrée
/// flottante — c'est ensuite `Rp2040OpenDrain` qui pilote la direction
/// directement par les registres du SIO. Le pull-up 4.7 kΩ est externe.
///
/// # Safety
/// Même raisonnement que [`configure_output_pin`] ; la `Pin` typée est
/// abandonnée à la fin, seule la configuration matérielle persiste.
fn configure_onewire_pin(pin: u8) {
    let id = DynPinId { bank: DynBankId::Bank0, num: pin };
    let raw = unsafe { new_pin(id) };
    let mut out = raw
        .try_into_function::<FunctionSio<SioOutput>>()
        .ok()
        .expect("SIO est une fonction valide sur toute broche de Bank0");
    out.set_pull_type(DynPullType::None);
    let _ = out.set_low();

    let _floating = out
        .try_into_function::<FunctionSio<SioInput>>()
        .ok()
        .expect("SIO est une fonction valide sur toute broche de Bank0");
}
