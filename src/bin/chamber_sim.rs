//! Simulateur de la chambre à brouillard — la **vraie** boucle de
//! contrôle, la **vraie** UI et la **vraie** logique de sécurité,
//! alimentées par un modèle thermique, dans une fenêtre SDL2.
//!
//! ```text
//! cargo run --release --target x86_64-unknown-linux-gnu \
//!     --features sim --bin chamber_sim
//! ```
//!
//! # Ce qu'on y voit
//!
//! - l'UI **réelle** de la machine dans une fenêtre SDL2 : le même
//!   rendu que `cargo test-live-ui-linux`, mais piloté par la physique
//!   et la boucle de contrôle au lieu d'un scénario clavier ;
//! - la température **descendre pour de vrai** : le compresseur tire la
//!   plaque vers son plancher, les sondes suivent par conduction, le
//!   régulateur à hystérésis coupe et reprend — c'est la physique qui
//!   répond au contrôle, pas un scénario ;
//! - la machine à états **passer d'une phase à l'autre** sur les
//!   conditions réelles du modèle (seuils, fenêtres de stabilité,
//!   timeouts) ;
//! - la sécurité **déclencher** si on injecte une panne (`P`) ;
//! - l'écran de veille s'afficher après 2 min de temps virtuel sans
//!   geste ;
//! - l'`ActuatorPlan` de chaque phase appliqué aux relais simulés.
//!
//! Le terminal reste une console de télémétrie : températures réelles
//! et lues, actionneurs, mini-graphe de la plaque.
//!
//! # Clavier (fenêtre)
//!
//! ```text
//! D/G ou flèches       tourner l'encodeur (droite / gauche)
//! Entrée (ou E)        clic
//! P                    panne de la sonde compresseur
//! S                    surchauffe compresseur (130 °C)
//! Espace               pause / reprise du temps virtuel
//! + / -                vitesse d'écoulement du temps
//! Q / Échap            quitter
//! ```
//!
//! Le temps virtuel s'écoule par défaut ×60 : un cycle complet de
//! refroidissement (~45 min) se joue en ~45 s. Les durées affichées
//! sont du temps virtuel.

use cloud_chamber_firmware::cloud_chamber_hal::actuators::Actuators;
use cloud_chamber_firmware::cloud_chamber_hal::config::{PLATE_MID_IDX, COMPRESSOR_IDX};
use cloud_chamber_firmware::cloud_chamber_hal::sensors::Sensors;
use cloud_chamber_firmware::cloud_chamber_hal::timer::{Instant, MonotonicTimer};
use cloud_chamber_firmware::drivers::encoder::EncoderEvent;
use cloud_chamber_firmware::logic::control_loop::tick;
use cloud_chamber_firmware::logic::phase_clock::PhaseClock;
use cloud_chamber_firmware::logic::probing::{MeasurementHistory, ProbingPlan};
use cloud_chamber_firmware::logic::security::{SafetyConfig, SafetyMonitor};
use cloud_chamber_firmware::shared::data::{SHARED_STATE, SystemTask};
use cloud_chamber_firmware::simulator::actuators::SimActuators;
use cloud_chamber_firmware::simulator::clock::VirtualClock;
use cloud_chamber_firmware::simulator::physics::{ChamberParams, ChamberPhysics};
use cloud_chamber_firmware::simulator::sensors::{SimPressureSensor, SimTempSensors};
use cloud_chamber_firmware::ui::app::UiApp;
use cloud_chamber_firmware::ui::navigator::Screen;
use embedded_graphics::geometry::Size;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics_simulator::{
    OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window, sdl2::Keycode,
};

/// Vitesse par défaut : 1 s réelle = 60 s virtuelles. Un cycle de 45 min
/// se joue en 45 s — assez pour voir la physique, assez lent pour lire
/// l'écran.
const DEFAULT_SPEED: f32 = 60.0;
/// Pas de physique (s virtuelles) : fin assez pour résoudre les
/// constantes de temps les plus rapides (charge compresseur).
const PHYSICS_STEP_S: f32 = 0.5;
/// Période de sondage (ms virtuelles) : le temps d'une conversion
/// DS18B20 12 bits — cf. `SimTempSensors::conversion_time_ms`. La
/// boucle de contrôle tourne à chaque itération, mais ne sonde qu'à
/// cette cadence, comme sur la machine.
const PROBE_PERIOD_MS: u64 = 750;
/// Cadence de rendu du terminal (ms réelles). La fenêtre, elle, rend
/// à chaque tour de boucle — c'est elle l'affichage principal.
const RENDER_MS: u64 = 100;

fn main() {
    let clock = VirtualClock::new();
    let physics = std::rc::Rc::new(std::cell::RefCell::new(
        ChamberPhysics::new(ChamberParams::default()),
    ));

    // Les actionneurs du HAL, peuplés par les relais simulés — le
    // panneau `PowerState` partagé (Rc<RefCell>) reste accessible pour
    // que la physique lise ce que la boucle applique.
    let sim = SimActuators::new();
    let power_panel = sim.panel();
    let mut actuators = Actuators {
        high_voltage: sim.high_voltage,
        cooling: sim.cooling,
        iso_heater: sim.iso_heater,
        iso_pump: sim.iso_pump,
        lights: sim.lights,
        glass_heater: sim.glass_heater,
        fans: sim.fans,
    };

    use cloud_chamber_firmware::cloud_chamber_hal::sensors::IndependentSensors;
    let mut sensors = Sensors::new(
        SimTempSensors::new(std::rc::Rc::clone(&physics), &clock),
        IndependentSensors([SimPressureSensor::new(&clock)]),
    );

    // Le même état de départ que `run()` sur la machine.
    let mut phase = PhaseClock::new(&clock, SystemTask::default());
    let mut safety = SafetyMonitor::new(SafetyConfig::default(), phase.now());
    let mut history = MeasurementHistory::new();
    // Dernier sondage : démarre « jamais fait » pour que le premier
    // tour sonde immédiatement.
    let mut last_probe = Instant::ZERO;

    // L'écran de la machine, dans une fenêtre — même harnais que le
    // test `ui_live` (cargo test-live-ui-linux), même échelle.
    let mut display = SimulatorDisplay::<Rgb565>::new(Size::new(320, 240));
    let output_settings = OutputSettingsBuilder::new().scale(2).build();
    let mut window = Window::new("Cloud Chamber — simulateur", &output_settings);

    // `Window::events` panique si `update` n'a jamais été appelé :
    // rendre une fois avant d'entrer dans la boucle (l'écran réel est
    // dessiné au premier tour, `UiApp::new` démarrant avec un
    // redessin en attente).
    window.update(&display);

    let mut app = UiApp::new();
    let mut speed = DEFAULT_SPEED;
    let mut paused = false;
    let mut probe_failed = false;
    let mut last_activity = Instant::ZERO;
    let mut last_task = SystemTask::Idle;

    println!("=== Cloud Chamber — simulateur ===");
    println!("UI dans la fenêtre, télémétrie ici.");
    println!("(D/G encoder, Entree clic, P panne sonde, S surchauffe, Espace pause, +/- vitesse, Q quitter)");
    println!();

    let mut last_physics = std::time::Instant::now();
    let mut last_render = std::time::Instant::now();

    'running: loop {
        // ─── Fenêtre → événements encodeur / commandes ────────────
        // Contrairement au test `ui_live`, le temps passe tout seul :
        // les événements ne font qu'agir sur la machine en marche.
        for event in window.events() {
            last_activity = clock.now();
            let ev = match event {
                SimulatorEvent::Quit => break 'running,
                SimulatorEvent::KeyDown { keycode, .. } => match keycode {
                    Keycode::Escape | Keycode::Q => break 'running,
                    Keycode::Space => {
                        paused = !paused;
                        continue;
                    }
                    Keycode::Plus | Keycode::Equals | Keycode::KpPlus => {
                        speed = (speed * 2.0).min(3600.0);
                        continue;
                    }
                    Keycode::Minus | Keycode::KpMinus => {
                        speed = (speed / 2.0).max(1.0);
                        continue;
                    }
                    Keycode::P => {
                        probe_failed = !probe_failed;
                        eprintln!(
                            ">>> sonde compresseur : {}",
                            if probe_failed { "MUETTE" } else { "reparee" }
                        );
                        continue;
                    }
                    Keycode::S => {
                        physics.borrow_mut().overheat_compressor();
                        eprintln!(">>> surchauffe compresseur injectee (130 C)");
                        continue;
                    }
                    Keycode::Right | Keycode::Down | Keycode::D => {
                        EncoderEvent::RotateClockwise
                    }
                    Keycode::Left | Keycode::Up | Keycode::G => {
                        EncoderEvent::RotateCounterClockwise
                    }
                    Keycode::Return | Keycode::E => EncoderEvent::ButtonPressed,
                    _ => continue,
                },
                _ => continue,
            };
            if let Some(task) = app.handle_event(ev, read_task()) {
                write_task(task);
            }
        }

        // ─── La veille suit le temps virtuel, comme sur la machine ──
        let idle = clock.now().since(last_activity);
        app.poll_idle(idle);

        // Sans ce sleep, la boucle tourne à 100 % d'un cœur : le
        // simulateur est un outil de bureau, pas un benchmark.
        std::thread::sleep(std::time::Duration::from_millis(if paused { 50 } else { 10 }));

        // ─── Physique : le pas de temps virtuel écoulé ──────────────
        // `last_physics` avance même en pause : au réveil, le temps
        // virtuel reprend là où il s'était arrêté au lieu de sauter
        // toute la durée de la pause.
        let now = std::time::Instant::now();
        let real_dt = now.duration_since(last_physics).as_secs_f32();
        last_physics = now;

        if !paused {
            let mut remaining = real_dt * speed;
            while remaining > 0.0 {
                let step = remaining.min(PHYSICS_STEP_S);
                physics.borrow_mut().step(step);
                clock.advance_seconds(step);
                remaining -= step;
            }

            // La physique applique ce que la boucle a commandé (la panne
            // injectée voyage avec, cf. `PowerState::compressor_probe_failed`).
            let mut power = power_panel.borrow().0;
            power.compressor_probe_failed = probe_failed;
            physics.borrow_mut().apply_power(power);

            // ─── Un tour de la boucle de contrôle (la vraie) ────────
            // Sondage à la facture du DS18B20 : une conversion 12 bits
            // prend 750 ms sur la machine, et la boucle réelle vit à ce
            // rythme — un échantillon par conversion, pas un par tour.
            // Sans cette limite, le simulateur pousse un échantillon à
            // chaque tour de boucle : à vitesse ×60, les 90 cases de
            // l'historique couvrent moins de 60 s et la fenêtre de
            // stabilité de `high_voltage` (60 s) ne peut plus être
            // couverte — la phase HV ne passait jamais (vu en pratique :
            // blocage jusqu'au timeout de phase).
            let probe_due =
                clock.now().since(last_probe).as_millis() as u64 >= PROBE_PERIOD_MS;
            let plan = if probe_due {
                last_probe = clock.now();
                ProbingPlan::all()
            } else {
                ProbingPlan::new(false, false)
            };
            tick(
                &mut sensors,
                &mut actuators,
                &mut phase,
                &mut safety,
                &mut history,
                plan,
            );
        }

        // ─── Copie de l'état partagé, plomberie du cœur 0 ───────────
        // Même enchaînement que la boucle d'affichage de `main.rs` :
        // copie courte sous verrou (et acquittement de `new_data`),
        // puis décision de redessiner hors verrou.
        let state = critical_section::with(|cs| {
            let mut shared = SHARED_STATE.borrow_ref_mut(cs);
            let copy = *shared;
            shared.new_data = false;
            copy
        });

        // Un changement de phase décidé par la logique ne passe par
        // aucun événement d'encodeur : sans ça, l'écran de suivi
        // resterait figé jusqu'au prochain geste de l'opérateur.
        if state.task != last_task {
            last_task = state.task;
            app.mark_dirty();
        }

        // Le graphe de veille se remplit quel que soit l'écran affiché ;
        // le redessin ne concerne que les écrans qui montrent des
        // mesures (cf. boucle du cœur 0 dans `main.rs`).
        if state.new_data {
            app.sample(&state);
            if matches!(
                app.current_screen(),
                Screen::Stats | Screen::CurrentTask | Screen::MainMenu | Screen::Idle
            ) {
                app.mark_dirty();
            }
        }

        // ─── Rendu : l'écran réel dans la fenêtre ───────────────────
        if app.take_redraw_request() {
            app.draw(&mut display, &state)
                .expect("le rendu de l'UI ne doit pas echouer");
        }
        window.update(&display);

        // ─── Télémétrie terminal (secondaire) ───────────────────────
        if last_render.elapsed() >= std::time::Duration::from_millis(RENDER_MS) {
            render(&clock, &physics, &power_panel, &history, &app, speed, paused);
            last_render = std::time::Instant::now();
        }
    }
}

fn read_task() -> SystemTask {
    critical_section::with(|cs| SHARED_STATE.borrow_ref(cs).task)
}

fn write_task(task: SystemTask) {
    critical_section::with(|cs| SHARED_STATE.borrow_ref_mut(cs).task = task);
}

/// Rendu terminal : télémétrie + mini-graphe. La fenêtre SDL2 affiche
/// l'UI ; ici, c'est le dessous des cartes — températures réelles du
/// modèle contre températures lues par les sondes, sorties, graphe.
fn render(
    clock: &VirtualClock,
    physics: &std::rc::Rc<std::cell::RefCell<ChamberPhysics>>,
    power_panel: &std::rc::Rc<std::cell::RefCell<cloud_chamber_firmware::simulator::actuators::PowerStateShared>>,
    history: &MeasurementHistory,
    app: &UiApp,
    speed: f32,
    paused: bool,
) {
    let now_ms = clock.now().since(Instant::ZERO).as_millis() as u64;
    let task = read_task();
    let power = power_panel.borrow().0;

    // Effacement sans scroll : curseur en haut, zone fixe.
    println!("\x1b[H\x1b[2J");

    // ─── État machine ───────────────────────────────────────────────
    let phase_name = format!("{:?}", task);
    println!("\x1b[1m{: <34}\x1b[0m {:>6.1} min virtuelles  ×{:.0}{}",
        phase_name,
        now_ms as f32 / 60_000.0,
        speed,
        if paused { "  [PAUSE]" } else { "" },
    );

    // ─── Sorties (ce que la physique applique) ─────────────────────
    println!("  froid[{}] IPA[{}] HV[{}] pompe[{}] vitre[{}] ventilo[{}]",
        on(power.cooling), on(power.iso_heater), on(power.high_voltage),
        on(power.iso_pump), on(power.glass_heater), on(power.fans),
    );

    // ─── Températures : réelles (physique) et lues (avec bruit) ────
    // La plaque et la corps compresseur : les deux grandeurs pilotes.
    let (plate_real, comp_real) = {
        let p = physics.borrow();
        (p.real_temp(PLATE_MID_IDX), p.real_temp(COMPRESSOR_IDX))
    };
    let plate_read = history.newest_valid(PLATE_MID_IDX).map(|c| c.0);
    println!("  plaque   réelle {:6.1} °C   lue {}", plate_real,
        match plate_read { Some(t) => format!("{:6.1} °C", t), None => "  ---".into() });
    println!("  comp.out réelle {:6.1} °C", comp_real);

    // ─── Mini-graphe de la plaque (60 dernières lectures) ──────────
    // Une lecture par pas de boucle : ~1/s virtuelle. 60 colonnes =
    // la dernière minute virtuelle de refroidissement.
    print!("  [");
    let mut temps = [f32::NAN; 60];
    let mut count = 0usize;
    for i in 0..60 {
        if let Ok(m) = history.temps[PLATE_MID_IDX].get(i) {
            if !m.value.is_nan() {
                temps[count] = m.value.0;
                count += 1;
            }
        }
    }
    // Échelle : 25..-45 °C du haut vers le bas, 8 rangées. Chaque
    // colonne trace un `│` discret et la valeur y pose son `█` au bon
    // rang — le profil de refroidissement se lit d'un coup d'œil.
    let row_of = |t: f32| ((25.0 - t) / 70.0 * 7.0).round().clamp(0.0, 7.0) as usize;
    let rows: Vec<usize> = temps[..count]
        .iter()
        .map(|t| if t.is_nan() { usize::MAX } else { row_of(*t) })
        .collect();
    for row in 0..8 {
        println!();
        print!("  ");
        for &r in &rows {
            if r == usize::MAX {
                print!(" ");
            } else if r == row {
                print!("\x1b[36m█\x1b[0m");
            } else {
                print!("\x1b[90m·\x1b[0m");
            }
        }
    }
    println!("]  {:>4} lectures", count);

    // ─── UI ─────────────────────────────────────────────────────────
    println!("  écran : {:?}   (rendu dans la fenêtre)", app.current_screen());

    println!("\n  (D/G tourner · Entrée clic · P panne sonde · S surchauffe · Espace pause · Q quitter)");
}

fn on(b: bool) -> &'static str {
    if b { "\x1b[32mON \x1b[0m" } else { "OFF" }
}
