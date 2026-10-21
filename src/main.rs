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
//! - l'interruption encodeur ne fait qu'empiler l'événement dans la file de
//!   [`console`](cloud_chamber_firmware::ui::console) et rend la main ;
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

use defmt_rtt as _;

use rp2040_hal::{
    Clock, I2C, self as hal,
    fugit::{ExtU32, RateExtU32},
    gpio::{DynBankId, DynPinId, FunctionI2c, PullUp, new_pin},
    i2c::{ValidatedPinScl, ValidatedPinSda},
    multicore::{Multicore, Stack},
};

use cloud_chamber_firmware::drivers::safe_panic;
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
    PIN_COMPRESSOR_RELAY, PIN_HV_RELAY, PIN_I2C_SCL, PIN_I2C_SDA, PIN_ISO_HEATER_RELAY,
    PIN_FANS_RELAY, PIN_LED_STRIP, PIN_LIGHTS_RELAY, PIN_ONEWIRE, PIN_PUMP_RELAY,
    PIN_WINDOW_HEATER_RELAY,
};
use cloud_chamber_firmware::drivers::bme280::{Bme280Driver, Bme280Sensor};
use cloud_chamber_firmware::drivers::breaker::GpioBreaker;
use cloud_chamber_firmware::drivers::led_strip;
use cloud_chamber_firmware::drivers::compressor::Compressor;
use cloud_chamber_firmware::drivers::ds18b20::{
    Ds18b20Bus, Ds18b20Sensors, Resolution, rp2040_adapter::Rp2040OpenDrain,
};
use cloud_chamber_firmware::drivers::heater::Heater;
use cloud_chamber_firmware::drivers::lights::Lights;
use cloud_chamber_firmware::drivers::fans::Fans;
use cloud_chamber_firmware::drivers::pump::Pump;
use cloud_chamber_firmware::drivers::window_heater::WindowHeater;
use cloud_chamber_firmware::logic::control_loop;
use cloud_chamber_firmware::shared::data::{SHARED_STATE, SystemTask};
use cloud_chamber_firmware::ui::app::UiApp;
use cloud_chamber_firmware::ui::console;
use cloud_chamber_firmware::ui::navigator::Screen;

/// Taille de la pile du cœur 1, en mots de 32 bits (8192 mots = 32 Ko).
///
/// `control_loop::run()` vit presque tout sur sa pile : historique de
/// mesures (~13 Ko à 90 échantillons × 9 sondes × 16 octets), états de
/// phase et de sécurité, `tick` inliné — le cadre complet de `run()`
/// mesure ~28 Ko sur le binaire release (lu au désassemblage). La pile
/// doit contenir ce cadre, plus la profondeur d'appel sous `run()`
/// (`probe`, bit-banging 1-Wire) et une marge d'exception.
///
/// Une pile trop petite ne reste pas silencieuse : `CORE1_STACK` est un
/// tableau en `.bss`, pas une région protégée — le débordement écrase
/// les statics voisins (le bloc RTT, vu corrompu en pratique), puis
/// finit en HardFault dans le `memcpy` de `MeasurementHistory::new`.
/// C'est le crash vu au démarrage du cœur 1 avec les 8 Ko d'origine,
/// devenus insuffisants quand l'historique est passé de 10 à 90
/// échantillons.
const CORE1_STACK_WORDS: usize = 8192;

/// La pile du cœur 1 doit dépasser le cadre de `control_loop::run()`,
/// dont l'historique de mesures est la part qui grandit quand la boucle
/// garde plus d'échantillons. Le garde-fou ne peut pas mesurer le cadre
/// compilé (il n'existe qu'après codegen, ~28 Ko mesurés en release) ;
/// il borne au moins l'historique — le reste du cadre est couvert par
/// la marge ci-dessous (~15 Ko, mesurés : cadre moins historique).
const _: () = {
    let stack = CORE1_STACK_WORDS * 4;
    let history =
        core::mem::size_of::<cloud_chamber_firmware::logic::probing::MeasurementHistory>();
    assert!(
        stack > history + 15 * 1024,
        "pile du coeur 1 trop petite pour le cadre de control_loop::run"
    );
};

/// Résolution des DS18B20. 12 bits est la valeur usine ; c'est aussi la
/// plus lente à convertir, mais `probe()` ne bloque pas dessus (conversion
/// lancée à un tour, résultat lu au suivant).
const TEMP_RESOLUTION: Resolution = Resolution::Bits12;

// ─── Pile du second cœur ───────────────────────────────────────────────────

/// Pile du cœur 1. Vit en `.bss`, donc prise sur la RAM restante — la pile
/// du cœur 0 garde tout le bas de la RAM, où loge le framebuffer de 150 Ko.
static CORE1_STACK: Stack<CORE1_STACK_WORDS> = Stack::new();

/// Délai du chien de garde pour un boot raté : assez court pour retenter
/// vite (la panne au boot est souvent transitoire — bus 1-Wire brouillé,
/// écran muet), assez long pour que l'opérateur lise l'écran d'erreur.
/// En microsecondes, l'unité du chien de garde du RP2040 — 10 s.
const BOOT_RETRY_DELAY_US: u32 = 10_000_000;

/// Échec d'initialisation : la machine ne peut pas piloter, mais elle ne
/// panique pas. L'écran d'erreur porte le message, le chien de garde
/// relance le boot au bout du délai — et un clic de l'opérateur force le
/// reset immédiat, même mécanisme que l'écran d'erreur de panique.
///
/// Tout ce qui échoue avant la console (l'écran lui-même) passe par un
/// chemin plus court : chien de garde + rotation, rien à afficher.
fn boot_fatal(
    console: &mut console::Console,
    watchdog: &mut hal::Watchdog,
    message: &str,
) -> ! {
    defmt::error!("erreur fatale au demarrage : {}", message);
    let screen = cloud_chamber_firmware::ui::screens::error::ErrorScreen;
    let _ = console.display().render(|target| screen.draw(target, message));
    watchdog.start(BOOT_RETRY_DELAY_US.micros());
    loop {
        if console::next_event().is_some() {
            defmt::error!("redemarrage demande par l'operateur");
            watchdog.disable();
            loop {
                core::hint::spin_loop();
            }
        }
        core::hint::spin_loop();
    }
}

/// Gestionnaire de panique du firmware complet (les deux cœurs).
///
/// Priorité absolue aux sorties de puissance : la coupure précède tout,
/// même la trace defmt — une écriture RTT n'aura jamais l'occasion de
/// déclencher quoi que ce soit sur les relais. Ensuite seulement, le
/// drapeau est publié pour l'écran (cœur 0) et la trace émise pour la
/// sonde ; l'un comme l'autre sont du meilleur effort, la coupure ne
/// l'est pas.
///
/// `panic_probe` faisait la trace et s'arrêtait là : les relais restaient
/// en l'état — HT comprise — et l'écran affichait une machine saine. Ce
/// gestionnaire inverse l'ordre des priorités.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    safe_panic::cut_power_outputs();
    safe_panic::mark_core1_panicked();
    defmt::error!("PANIQUE : {}", defmt::Debug2Format(&info));
    loop {
        core::hint::spin_loop();
    }
}

#[hal::entry]
fn main() -> ! {
    let mut board = board::init();

    defmt::info!("cloud-chamber : demarrage");

    // Le chien de garde relance tout le boot si une initialisation
    // échoue : pris avant la console, il sert déjà aux erreurs fatales
    // de démarrage (cf. `boot_fatal`) — un boot raté se retente, il ne
    // s'arrête pas.
    let mut watchdog = board.watchdog;

    // ─── Console opérateur (écran + encodeur) ──────────────────────────────
    //
    // Tout le montage vit dans `ui::console` : même fréquence SPI, même
    // framebuffer, même interruption de scrutation que les bins de
    // bring-up. Cf. sa doc de module pour l'architecture ISR/boucle qu'il
    // impose — et pourquoi le rendu ne doit prendre aucune section
    // critique sur cette machine à deux cœurs.
    //
    // Un écran muet au boot n'a droit ni à une panique ni à un écran
    // d'erreur (rien pour l'afficher) : le chien de garde relance la
    // machine au bout du délai, le boot retente.
    let mut console = match console::init(
        board.spi0,
        &mut board.resets,
        board.clocks.peripheral_clock.freq(),
        &mut board.timer,
    ) {
        Ok(console) => console,
        Err(e) => {
            defmt::error!("ecran indisponible au boot : {}", defmt::Debug2Format(&e));
            watchdog.start(BOOT_RETRY_DELAY_US.micros());
            loop {
                core::hint::spin_loop();
            }
        }
    };

    // `UiApp` n'est volontairement pas un static : il appartient à la
    // boucle du cœur 0, seule à le toucher. L'interruption ne communique
    // avec lui que par la file d'événements de `console`.
    let mut app = UiApp::new();

    // ─── Bande LED du compartiment machine ────────────────────────────────
    //
    // Commande opérateur uniquement, depuis l'entrée de menu dédiée :
    // pas dans les `Actuators` du cœur 1, pas dans les plans de phase.
    // Le cœur 0 en est seul propriétaire — cf. `drivers::led_strip` pour
    // le pourquoi de cette séparation d'avec l'éclairage de chambre.
    let mut bay_lights = led_strip::LedStrip::new(board::configure_output_pin(PIN_LED_STRIP));

    // Premier rendu : l'opérateur voit le menu pendant la découverte des
    // capteurs, qui prend un instant.
    let initial_state = critical_section::with(|cs| *SHARED_STATE.borrow_ref(cs));
    console.redraw(&app, &initial_state);
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
    // attendu, ce qui rend toute lecture de température douteuse. Pas de
    // panique : l'écran d'erreur porte le message, et le chien de garde
    // relance le boot au bout du délai — le bus a une chance de revenir,
    // la machine ne reste pas morte sur une panne transitoire.
    let temperature_source = match Ds18b20Sensors::new(bus, board.timer, board.timer, TEMP_RESOLUTION) {
        Ok(sensors) => sensors,
        Err(e) => {
            defmt::error!("echec config DS18B20 : {}", defmt::Debug2Format(&e));
            boot_fatal(
                &mut console,
                &mut watchdog,
                "Bus 1-Wire inutilisable au demarrage.",
            );
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
        // laisser `control_loop` tourner sur un capteur muet. L'écran
        // d'erreur porte le message, le chien de garde relance le boot.
        defmt::error!("echec init BME280 (adresse 0x76) : {}", defmt::Debug2Format(&e));
        boot_fatal(
            &mut console,
            &mut watchdog,
            "Capteur de pression indisponible au demarrage.",
        );
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
        fans: Fans::new(board::configure_relay_pin(PIN_FANS_RELAY)),
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
            // La trace des valeurs elles-mêmes : si le contenu relu est
            // faux (mauvais secteur, corruption silencieuse), c'est ici
            // que ça se voit, avant que la boucle n'applique quoi que
            // ce soit.
            defmt::info!(
                "reglages relus depuis la flash : {}",
                defmt::Debug2Format(&saved)
            );
            settings::set(saved);
            // L'écran de réglages travaille sur une copie prise à sa
            // création — avant le lancement du cœur 1, donc avant cette
            // relecture : sans ce rappel, il afficherait (et
            // republierait au premier cran d'encodeur) les valeurs par
            // défaut, pendant que le reste de la machine vivrait sur
            // les réglages relus.
            app.reload_settings_from_shared();
        }
        None => defmt::info!("aucun reglage en flash, valeurs par defaut"),
    }

    // Demande de sauvegarde acceptée mais pas encore écrite : le secteur
    // est plein et la machine tourne, donc l'effacement attend l'arrêt. Le
    // réglage lui-même est déjà appliqué (`shared::settings`) — seule sa
    // survie à une coupure est différée.
    let mut pending_save: Option<Settings> = None;

    // `Some(())` une fois l'écran d'erreur affiché : plus rien ne
    // réalimente le chien de garde ensuite.
    let mut dead: Option<()> = None;

    // ─── Cœur 0 : la boucle d'interface ───────────────────────────────────
    //
    // Rien ici ne dessine sous section critique : on prend une copie de
    // l'état, puis on travaille dessus verrou relâché (cf. doc de module).
    let mut last_task = SystemTask::Idle;
    let mut last_activity = board.timer.get_counter();

    // Santé des sondes au dernier sondage (bit i = index i) — journalisée
    // à chaque changement, cf. la boucle.
    let mut last_mute_mask = 0u8;

    // Réinitialisation périodique de l'écran : la seule arme contre
    // l'écran blanc intermittent (contrôleur déréglé par un crêux
    // d'alim ou un parasite — il ne se répare que par sa séquence de
    // boot, un power-cycle logiciel quoi). Toutes les 5 minutes :
    // assez fréquent pour qu'une panne ne dure jamais plus que ça,
    // assez rare pour que le gel de ~130 ms passe inaperçu.
    const DISPLAY_REINIT_INTERVAL_US: u64 = 5 * 60 * 1_000_000;
    let mut last_display_reinit = board.timer.get_counter();

    loop {
        // Une fois le cœur 1 mort, le seul geste qui reste : un clic
        // (n'importe quel événement encodeur — le bouton est le seul qui
        // en produise) coupe le chien de garde et force le reset au plus
        // vite.
        //
        // Et rien d'autre : pas de `pump`, pas de redraw. Poursuivre le
        // corps normal de la boucle laissait un événement d'encodeur —
        // vraie rotation ou parasite EMI sur les contacts — redessiner le
        // menu PAR-DESSUS l'écran d'erreur, qui n'apparaissait donc
        // jamais à l'opérateur (vu sur le terrain). Le chien de garde
        // armé à l'affichage de l'erreur déclenche seul au délai.
        if dead.is_some() {
            if cloud_chamber_firmware::ui::console::next_event().is_some() {
                defmt::error!("redemarrage demande par l'operateur");
                watchdog.disable();
                loop {
                    core::hint::spin_loop();
                }
            }
            continue;
        }

        // Applique les événements empilés par l'interruption, journalise
        // les pertes, et dit si l'opérateur vient de se manifester.
        // L'uptime, lu sur le timer du cœur 0 : la boucle est seule à le
        // connaître, l'écran Info seule à l'afficher.
        app.set_uptime(Duration::from_micros(board.timer.get_counter().ticks()));

        if console.pump(&mut app) {
            last_activity = board.timer.get_counter();
        }

        // Copie de l'état sous verrou court, et acquittement des nouvelles
        // mesures dans le même geste — le dessin qui suit se fait dessus,
        // hors verrou.
        let state = critical_section::with(|cs| {
            let mut shared = SHARED_STATE.borrow_ref_mut(cs);
            let copy = *shared;
            shared.new_data = false;
            // Vu une fois suffit : plus personne ne peut re-lever ce
            // drapeau (le cœur 1 est mort), le garder rendrait l'écran
            // d'erreur permanent même après un hypothétique nouveau
            // démarrage du cœur.
            shared.core1_panicked = false;
            copy
        });

        // Le cœur de contrôle est mort : l'écran d'erreur prend la place,
        // et le chien de garde est armé — la boucle ne le réalimentera
        // plus jamais. Deux façons de repartir, la même pour l'opérateur
        // et la machine :
        //
        // - **clic sur REDEMARRER** : reset immédiat, en arrêtant
        //   d'alimenter le chien de garde (`disable` puis plus rien) ;
        // - **inaction** : le watchdog déclenche tout seul au bout du
        //   délai ci-dessous. Une chambre qui a perdu son contrôle ne
        //   doit pas rester muette indéfiniment.
        //
        // Les deux passent par le même reset matériel : au redémarrage,
        // `main()` reconfigure tout depuis zéro, relais forcés au niveau
        // bas par les constructeurs des drivers — l'état de panique ne
        // survit pas au reset.
        let error_pending = state.core1_panicked;
        if error_pending && dead.is_none() {
            defmt::error!("coeur 1 arrete : affichage de l'ecran d'erreur");
            let screen = cloud_chamber_firmware::ui::screens::error::ErrorScreen;
            let _ = console
                .display()
                .render(|target| screen.draw(target, "Boucle de controle arretee (panique)."));
            // 60 s de délai avant l'auto-reset : assez pour lire
            // l'écran, assez court pour ne pas laisser la chambre
            // sans supervision très longtemps. En microsecondes —
            // `60_000` valait 60 ms et l'écran disparaissait avant
            // d'avoir été vu.
            watchdog.start(60_000_000u32.micros());
            dead = Some(());
        }

        // ─── Santé des sondes : visibilité ────────────────────────────────
        // Le cœur 1 publie à chaque sondage les capteurs qui n'ont pas
        // répondu CE tour-là (bit i = index i). Un capteur brouillé ne
        // doit se voir ni trop tard (à l'arrêt brutal) ni jamais (valeur
        // figée à l'écran) : le journal dit qui et quand, l'écran Stats
        // affiche `---`.
        if state.snapshot.temp_mute_mask != last_mute_mask {
            last_mute_mask = state.snapshot.temp_mute_mask;
            if last_mute_mask != 0 {
                defmt::warn!("capteurs muets au dernier sondage : {:#b} sur 8", last_mute_mask);
            } else {
                defmt::info!("tous les capteurs repondent");
            }
        }

        // L'auto-guérison de l'écran : réinitialisation périodique du
        // contrôleur, image renvoyée telle quelle (cf. le commentaire
        // de `last_display_reinit`).
        if (board.timer.get_counter() - last_display_reinit).to_micros()
            >= DISPLAY_REINIT_INTERVAL_US
        {
            last_display_reinit = board.timer.get_counter();
            console.reinit_display();
            defmt::debug!("ecran reinitalise (entretien periodique)");
        }

        // Un changement de phase décidé par le cœur 1 ne passe par aucun
        // événement d'encodeur : sans ça l'écran de suivi resterait figé
        // jusqu'au prochain geste de l'opérateur.
        //
        // Le journal qui va avec : un retour à `Idle` en cours de cycle est
        // silencieux à l'écran (pas de `Tripped`, pas de bannière) — sans
        // cette trace, impossible de distinguer un abandon perte-sonde
        // (`SENSOR_LOSS`) d'un timeout de phase, ni même de savoir de
        // quelle phase la machine est tombée.
        if state.task != last_task {
            defmt::info!(
                "phase : {} -> {}",
                defmt::Debug2Format(&last_task),
                defmt::Debug2Format(&state.task)
            );
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
        // L'écran COURBE dessine le même graphe que la veille : sans lui,
        // la courbe ne s'anime qu'au prochain geste — figée, elle prêterait
        // à croire que la machine ne rafraîchit plus.
        if state.new_data {
            app.sample(&state);
            if matches!(
                app.current_screen(),
                Screen::Stats
                    | Screen::CurrentTask
                    | Screen::MainMenu
                    | Screen::Graph
                    | Screen::Idle
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

        // Bande LED du compartiment : la demande vient de l'écran dédié,
        // consommée ici — le cœur 0 est seul à toucher la broche. L'erreur
        // d'écriture suit la politique du driver : ignorée, la broche se
        // signalera si elle refuse (cf. `drivers::relay::Relay::new`).
        if let Some(on) = app.take_strip_request() {
            let _ = bay_lights.set(on);
        }

        // L'écran d'erreur vient d'être dessiné dans ce même tour (le
        // cœur 1 vient de mourir) : un redraw en attente — il peut
        // traîner depuis un geste d'encodeur pomplé plus haut dans ce
        // tour — l'écraserait avec le menu. Le garde de tête de boucle
        // couvre les tours suivants, celui-ci couvre le tour courant.
        if dead.is_none() && app.take_redraw_request() {
            console.redraw(&app, &state);
        }
    }
}
