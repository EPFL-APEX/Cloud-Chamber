//! Boucle UI : ce qu'un événement d'encodeur déclenche, et quand redessiner.
//!
//! [`UiApp`] est le sommet de `ui/` : au-dessus de [`super::router::Screens`]
//! (qui route vers l'écran courant) mais toujours **sans rien connaître du
//! matériel**. Il tient les deux choses qu'une boucle UI doit tenir :
//!
//! - la traduction `EncoderEvent` → action sur les écrans, et la demande
//!   d'état qui en ressort (démarrage de cycle) ;
//! - le drapeau « l'affichage ne reflète plus l'état », pour ne redessiner
//!   que quand c'est nécessaire — un rendu plein écran coûte cher.
//!
//! # Pourquoi ce n'est pas ici que vit le `loop {}`
//!
//! Le vrai `loop {}` reste dans le binaire, et c'est volontaire : sur cible,
//! il est réparti entre deux contextes d'exécution que ce module ne peut pas
//! abstraire sans devenir spécifique à une puce.
//!
//! - [`UiApp::handle_event`] tourne dans une **interruption** matérielle
//!   (`TIMER_IRQ_0` sur RP2040), pour que l'encodeur reste lu même pendant
//!   un transfert SPI bloquant. Un `#[interrupt]` est lié à un vecteur
//!   d'interruption précis d'une puce précise ; il ne peut pas être écrit
//!   dans une bibliothèque qui compile aussi pour RP2350 ARM et RISC-V.
//! - [`UiApp::draw`] tourne dans la **boucle principale**, parce que le
//!   transfert SPI n'a rien à faire dans une routine d'interruption.
//!
//! `ui/` reste donc compilable et testable sur les trois cibles de la CI
//! comme sur l'hôte. Ce qui était réellement dupliqué d'un appelant à
//! l'autre — router l'événement, lever le drapeau, filtrer la demande
//! d'état — vit ici, en un seul exemplaire testé.
//!
//! # Usage attendu
//!
//! Côté interruption :
//!
//! ```ignore
//! let current = SHARED_STATE.borrow_ref(cs).task;
//! if let Some(task) = app.handle_event(encoder.poll(), current) {
//!     SHARED_STATE.borrow_ref_mut(cs).task = task;
//! }
//! ```
//!
//! Côté boucle principale :
//!
//! ```ignore
//! if app.take_redraw_request() {
//!     display.render(|target| app.draw(target, &state))?;
//! }
//! ```

use embedded_graphics::{draw_target::DrawTarget, geometry::OriginDimensions, pixelcolor::Rgb565};

use crate::config::settings::Settings;
use crate::drivers::encoder::EncoderEvent;
use crate::shared::data::{SharedState, SystemTask};

use super::navigator::Screen;
use super::router::Screens;

/// Sommet de l'interface : les écrans, plus l'état de la boucle.
pub struct UiApp {
    screens: Screens,
    needs_redraw: bool,
}

impl UiApp {
    /// Démarre avec un redessin en attente : rien n'a encore été affiché,
    /// le premier tour de boucle doit donc dessiner sans qu'aucun
    /// événement ne soit arrivé.
    pub fn new() -> Self {
        Self { screens: Screens::new(), needs_redraw: true }
    }

    /// Applique un événement d'encodeur, et renvoie l'éventuel changement
    /// d'état demandé par l'opérateur.
    ///
    /// `current` est l'état de la machine au moment de l'appel : il sert à
    /// refuser un démarrage quand un cycle tourne déjà (cf.
    /// [`Screens::take_task_request`]). L'écriture dans `SHARED_STATE` est
    /// laissée à l'appelant — même séparation décision/application que
    /// partout ailleurs, et c'est ce qui rend cette fonction testable sans
    /// le `static`.
    ///
    /// [`EncoderEvent::None`] ne fait rien et ne salit pas l'affichage :
    /// c'est le cas de très loin le plus fréquent, l'encodeur étant scruté
    /// à ~1 ms.
    pub fn handle_event(
        &mut self,
        event: EncoderEvent,
        current: SystemTask,
    ) -> Option<SystemTask> {
        match event {
            EncoderEvent::RotateClockwise => {
                self.screens.right_turn();
                self.needs_redraw = true;
                None
            }
            EncoderEvent::RotateCounterClockwise => {
                self.screens.left_turn();
                self.needs_redraw = true;
                None
            }
            EncoderEvent::ButtonPressed => {
                self.screens.click();
                self.needs_redraw = true;
                self.screens.take_task_request(current)
            }
            EncoderEvent::None => None,
        }
    }

    /// Signale que l'affichage ne reflète plus l'état, sans qu'aucune
    /// entrée opérateur ne soit en cause : nouvelles mesures publiées,
    /// changement de phase décidé par `logic::control_loop`… Sans ça, un
    /// écran de suivi resterait figé tant que personne ne touche au bouton.
    pub fn mark_dirty(&mut self) {
        self.needs_redraw = true;
    }

    /// Vrai si l'affichage doit être refait — et remet le drapeau à zéro.
    ///
    /// Lire et effacer d'un seul coup, plutôt qu'en deux appels : sur
    /// cible, ce drapeau est levé depuis une interruption, et un événement
    /// arrivé entre la lecture et l'effacement serait perdu.
    pub fn take_redraw_request(&mut self) -> bool {
        core::mem::take(&mut self.needs_redraw)
    }

    /// Récupère une demande de sauvegarde en flash levée par l'écran de
    /// réglages, et la consomme — cf. [`Screens::take_save_request`].
    pub fn take_save_request(&mut self) -> Option<Settings> {
        self.screens.take_save_request()
    }

    /// Écran actuellement affiché.
    pub fn current_screen(&self) -> Screen {
        self.screens.current()
    }

    /// Dessine l'écran courant.
    ///
    /// Ne consulte pas le drapeau de redessin : l'appelant décide *quand*
    /// dessiner (cf. [`UiApp::take_redraw_request`]), parce que sur cible
    /// ce dessin est enveloppé dans un rendu à framebuffer dont seul le
    /// binaire connaît la forme.
    pub fn draw<D>(&self, display: &mut D, state: &SharedState) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        self.screens.draw(display, state)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_chamber_hal::config::CHAMBER_TEMP_IDX;
    use crate::cloud_chamber_hal::measurement::Measurement;
    use crate::cloud_chamber_hal::timer::Instant;
    use crate::cloud_chamber_hal::units::Celsius;
    use crate::logic::cooling::CoolingPhase;
    use crate::logic::stopping::StoppingPhase;
    use crate::shared::data::SensorSnapshot;
    use embedded_graphics::geometry::Size;
    use embedded_graphics_simulator::SimulatorDisplay;

    fn make_display() -> SimulatorDisplay<Rgb565> {
        SimulatorDisplay::new(Size::new(320, 240))
    }

    fn state_with(task: SystemTask) -> SharedState {
        SharedState { snapshot: SensorSnapshot::default(), task, new_data: false }
    }

    #[test]
    fn the_first_frame_is_drawn_without_any_input() {
        let mut app = UiApp::new();
        assert!(app.take_redraw_request(), "le premier ecran n'a jamais ete dessine");
        assert!(!app.take_redraw_request(), "et une seule fois");
    }

    #[test]
    fn a_rotation_asks_for_a_redraw() {
        let mut app = UiApp::new();
        app.take_redraw_request(); // consomme le redessin initial

        assert_eq!(
            app.handle_event(EncoderEvent::RotateClockwise, SystemTask::Idle),
            None,
        );
        assert!(app.take_redraw_request());
    }

    /// Le cas le plus fréquent de loin : scruté à ~1 ms, l'encodeur ne
    /// renvoie presque jamais autre chose. Redessiner là-dessus reviendrait
    /// à redessiner en permanence.
    #[test]
    fn an_idle_poll_does_not_ask_for_a_redraw() {
        let mut app = UiApp::new();
        app.take_redraw_request();

        for _ in 0..1_000 {
            assert_eq!(app.handle_event(EncoderEvent::None, SystemTask::Idle), None);
        }
        assert!(!app.take_redraw_request());
    }

    #[test]
    fn clicking_the_first_item_from_idle_asks_to_start_a_cycle() {
        let mut app = UiApp::new();

        let requested = app.handle_event(EncoderEvent::ButtonPressed, SystemTask::Idle);

        assert_eq!(requested, Some(SystemTask::Cooling(CoolingPhase::SensorCheck)));
        assert_eq!(app.current_screen(), Screen::CurrentTask);
        assert!(app.take_redraw_request());
    }

    /// La garde de `Screens` est bien traversée : re-cliquer en plein cycle
    /// ne redemande pas de démarrage, mais affiche quand même le suivi.
    #[test]
    fn clicking_again_mid_cycle_asks_for_nothing() {
        let mut app = UiApp::new();
        let running = SystemTask::Cooling(CoolingPhase::HighVoltage);

        app.handle_event(EncoderEvent::ButtonPressed, SystemTask::Idle);
        app.handle_event(EncoderEvent::ButtonPressed, running); // suivi -> menu
        let requested = app.handle_event(EncoderEvent::ButtonPressed, running);

        assert_eq!(requested, None);
        assert_eq!(app.current_screen(), Screen::CurrentTask);
    }

    /// Une avancée de phase décidée par `logic/` ne passe par aucun
    /// événement d'encodeur : sans `mark_dirty`, l'écran de suivi resterait
    /// figé sur la phase affichée au moment du clic.
    #[test]
    fn new_data_can_ask_for_a_redraw_without_any_input() {
        let mut app = UiApp::new();
        app.take_redraw_request();
        assert!(!app.take_redraw_request());

        app.mark_dirty();
        assert!(app.take_redraw_request());
    }

    #[test]
    fn drawing_the_running_screen_follows_the_shared_state() {
        let mut app = UiApp::new();
        app.handle_event(EncoderEvent::ButtonPressed, SystemTask::Idle);

        // Chaque état que `logic/` peut publier pendant qu'on est sur cet
        // écran doit se dessiner : une panique ici, c'est l'affichage mort
        // en plein cycle.
        for task in [
            SystemTask::Cooling(CoolingPhase::SensorCheck),
            SystemTask::Cooling(CoolingPhase::PreCoolingThePlate),
            SystemTask::Cooling(CoolingPhase::StartingIpaCirculation),
            SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa),
            SystemTask::Cooling(CoolingPhase::HighVoltage),
            SystemTask::Cooling(CoolingPhase::FinalCheckBeforeStabilising),
            SystemTask::Stabilising,
            SystemTask::Stopping(StoppingPhase::CutHighVoltage),
            SystemTask::Idle,
        ] {
            let mut state = state_with(task);
            state.snapshot.temps[CHAMBER_TEMP_IDX] =
                Some(Measurement::new(Instant::from_micros(1), Celsius(-20.0)));
            let mut d = make_display();
            app.draw(&mut d, &state).unwrap();
        }
    }

    // ─── Harnais interactif ──────────────────────────────────────────────
    //
    // `cargo test-live-ui-linux` ouvre une vraie fenêtre SDL2 et pilote le
    // routeur au clavier. Seule la boucle d'événements est derrière la
    // feature `live-menu-test` (SDL2 n'est pas une dépendance du projet) :
    // la traduction "action opérateur -> effet" vit dans `apply_live_action`
    // ci-dessous, une fonction ordinaire, testée comme le reste. Ce qui
    // n'est pas vérifiable sans SDL2 se limite donc au mapping touche ->
    // action et au dessin.

    /// Ce qu'un opérateur peut faire, indépendamment du clavier.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum LiveAction {
        RotateRight,
        RotateLeft,
        Click,
        /// Fait avancer la machine simulée d'un cran.
        ///
        /// Ce harnais ne fait pas tourner `logic::control_loop` (il n'a ni
        /// capteurs ni actionneurs) : sans ça, l'écran de suivi resterait
        /// figé sur la première phase et on ne verrait jamais la checklist
        /// se remplir. C'est une doublure de démonstration, pas une
        /// seconde source de vérité sur l'enchaînement réel des phases.
        AdvancePhase,
        /// Ramène la machine à l'arrêt.
        Reset,
    }

    /// État suivant de la doublure — suit l'ordre de `logic::cooling` et
    /// `logic::stopping`, y compris le retour à `Idle` en fin d'arrêt.
    fn next_task(task: SystemTask) -> SystemTask {
        use CoolingPhase::*;
        use StoppingPhase::*;
        match task {
            SystemTask::Idle => SystemTask::Cooling(SensorCheck),
            SystemTask::Cooling(SensorCheck) => SystemTask::Cooling(PreCoolingThePlate),
            SystemTask::Cooling(PreCoolingThePlate) => {
                SystemTask::Cooling(StartingIpaCirculation)
            }
            SystemTask::Cooling(StartingIpaCirculation) => {
                SystemTask::Cooling(SaturatingAirWithIpa)
            }
            SystemTask::Cooling(SaturatingAirWithIpa) => SystemTask::Cooling(HighVoltage),
            SystemTask::Cooling(HighVoltage) => {
                SystemTask::Cooling(FinalCheckBeforeStabilising)
            }
            SystemTask::Cooling(FinalCheckBeforeStabilising) => SystemTask::Stabilising,
            SystemTask::Stabilising => SystemTask::Stopping(CutHighVoltage),
            SystemTask::Stopping(CutHighVoltage) => SystemTask::Stopping(CutIsoprop),
            SystemTask::Stopping(CutIsoprop) => SystemTask::Stopping(CutCompressor),
            SystemTask::Stopping(CutCompressor) => {
                SystemTask::Stopping(WaitPressureEquilibrium)
            }
            SystemTask::Stopping(WaitPressureEquilibrium) => SystemTask::Idle,
            SystemTask::Tripped(_) => SystemTask::Idle,
        }
    }

    /// Température chambre plausible pour l'état courant, pour que le pied
    /// de l'écran de suivi affiche autre chose que `---`.
    fn simulated_chamber_temp(task: SystemTask) -> f32 {
        use CoolingPhase::*;
        match task {
            SystemTask::Idle => 20.0,
            SystemTask::Cooling(SensorCheck) => 19.0,
            SystemTask::Cooling(PreCoolingThePlate) => 2.0,
            SystemTask::Cooling(StartingIpaCirculation) => -12.0,
            SystemTask::Cooling(SaturatingAirWithIpa) => -26.0,
            SystemTask::Cooling(HighVoltage) => -31.0,
            SystemTask::Cooling(FinalCheckBeforeStabilising) => -32.0,
            SystemTask::Stabilising => -33.0,
            SystemTask::Stopping(_) => -20.0,
            SystemTask::Tripped(_) => 40.0,
        }
    }

    /// Applique une action au harnais.
    ///
    /// Les trois actions « encodeur » passent par le même
    /// [`UiApp::handle_event`] que sur cible, et la demande d'état qui en
    /// ressort est appliquée exactement comme le fait l'ISR de
    /// `src/bin/ui_test.rs`. C'est ce qui rend ce harnais représentatif :
    /// il exerce le chemin de production, il ne le réimplémente pas.
    fn apply_live_action(app: &mut UiApp, state: &mut SharedState, action: LiveAction) {
        let event = match action {
            LiveAction::RotateRight => EncoderEvent::RotateClockwise,
            LiveAction::RotateLeft => EncoderEvent::RotateCounterClockwise,
            LiveAction::Click => EncoderEvent::ButtonPressed,
            LiveAction::AdvancePhase => {
                state.task = next_task(state.task);
                app.mark_dirty();
                EncoderEvent::None
            }
            LiveAction::Reset => {
                state.task = SystemTask::Idle;
                app.mark_dirty();
                EncoderEvent::None
            }
        };

        if let Some(task) = app.handle_event(event, state.task) {
            state.task = task;
        }

        state.snapshot.temps[CHAMBER_TEMP_IDX] = Some(Measurement::new(
            Instant::from_micros(1),
            Celsius(simulated_chamber_temp(state.task)),
        ));
    }

    /// Le scénario que le harnais sert à voir à l'œil : cliquer sur le
    /// premier item démarre la machine *et* bascule sur l'écran de suivi.
    #[test]
    fn live_click_on_the_first_item_starts_the_machine() {
        let mut app = UiApp::new();
        let mut state = state_with(SystemTask::Idle);

        apply_live_action(&mut app, &mut state, LiveAction::Click);

        assert_eq!(state.task, SystemTask::Cooling(CoolingPhase::SensorCheck));
        assert_eq!(app.current_screen(), Screen::CurrentTask);
    }

    /// Le même scénario vu depuis le harnais : une fois la machine lancée,
    /// re-cliquer sur le premier item ne la ramène pas au début — c'est ce
    /// qu'on veut pouvoir constater à l'œil dans la fenêtre.
    #[test]
    fn live_pressing_start_again_does_not_rewind_the_machine() {
        let mut app = UiApp::new();
        let mut state = state_with(SystemTask::Idle);

        apply_live_action(&mut app, &mut state, LiveAction::Click);
        apply_live_action(&mut app, &mut state, LiveAction::AdvancePhase);
        apply_live_action(&mut app, &mut state, LiveAction::AdvancePhase);
        let advanced = state.task;
        assert_ne!(advanced, SystemTask::Cooling(CoolingPhase::SensorCheck));

        // Retour au menu, puis nouveau clic sur le premier item.
        apply_live_action(&mut app, &mut state, LiveAction::Click);
        apply_live_action(&mut app, &mut state, LiveAction::Click);

        assert_eq!(state.task, advanced, "la machine n'a pas ete rembobinee");
        assert_eq!(app.current_screen(), Screen::CurrentTask);
    }

    /// Et la doublure fait bien progresser la checklist, jusqu'au retour
    /// à l'arrêt — c'est tout l'intérêt de la touche dédiée.
    #[test]
    fn live_advance_walks_the_whole_sequence_back_to_idle() {
        let mut app = UiApp::new();
        let mut state = state_with(SystemTask::Idle);
        apply_live_action(&mut app, &mut state, LiveAction::Click);

        let mut seen = std::vec::Vec::new();
        for _ in 0..12 {
            seen.push(state.task);
            if state.task == SystemTask::Idle && seen.len() > 1 {
                break;
            }
            apply_live_action(&mut app, &mut state, LiveAction::AdvancePhase);
        }

        // Les 6 phases de refroidissement, puis Stabilising, puis l'arrêt.
        assert!(seen.contains(&SystemTask::Cooling(CoolingPhase::HighVoltage)));
        assert!(seen.contains(&SystemTask::Stabilising));
        assert!(seen.contains(&SystemTask::Stopping(StoppingPhase::CutCompressor)));
        assert_eq!(*seen.last().unwrap(), SystemTask::Idle, "revient a l'arret");
    }

    /// Chaque état atteignable au clavier doit se dessiner — sinon la
    /// fenêtre meurt en cours de démonstration.
    #[test]
    fn live_every_reachable_state_draws_on_the_running_screen() {
        let mut app = UiApp::new();
        let mut state = state_with(SystemTask::Idle);
        apply_live_action(&mut app, &mut state, LiveAction::Click);
        assert_eq!(app.current_screen(), Screen::CurrentTask);

        for _ in 0..11 {
            let mut d = make_display();
            app.draw(&mut d, &state).unwrap();
            apply_live_action(&mut app, &mut state, LiveAction::AdvancePhase);
        }
    }

    #[test]
    fn live_reset_brings_the_machine_back_to_idle() {
        let mut app = UiApp::new();
        let mut state = state_with(SystemTask::Idle);
        apply_live_action(&mut app, &mut state, LiveAction::Click);
        apply_live_action(&mut app, &mut state, LiveAction::AdvancePhase);
        assert_ne!(state.task, SystemTask::Idle);

        apply_live_action(&mut app, &mut state, LiveAction::Reset);
        assert_eq!(state.task, SystemTask::Idle);
    }

    /// Le pied de l'écran de suivi doit afficher une vraie mesure, pas
    /// `---` : toute action pose une lecture chambre cohérente.
    #[test]
    fn live_actions_always_leave_a_chamber_reading() {
        let mut app = UiApp::new();
        let mut state = state_with(SystemTask::Idle);
        for action in [
            LiveAction::RotateRight,
            LiveAction::Click,
            LiveAction::AdvancePhase,
            LiveAction::RotateLeft,
            LiveAction::Reset,
        ] {
            apply_live_action(&mut app, &mut state, action);
            assert!(state.snapshot.temps[CHAMBER_TEMP_IDX].is_some(), "{action:?}");
        }
    }

    /// Fenêtre SDL2 interactive sur le routeur complet (menu, réglages,
    /// stats, suivi de cycle) :
    ///
    /// - flèches gauche/droite (ou haut/bas) : tourner l'encodeur
    /// - **Entrée / Espace : cliquer** (depuis le menu, le premier item
    ///   démarre un cycle et ouvre son suivi)
    /// - `N` : faire avancer la machine simulée d'une phase
    /// - `R` : remettre la machine à l'arrêt
    /// - fermer la fenêtre ou `Échap` : quitter
    ///
    /// `cargo test-live-ui-linux` (nécessite SDL2 installé et un affichage).
    #[cfg(feature = "live-menu-test")]
    #[test]
    fn ui_live() {
        use embedded_graphics::{
            mono_font::{MonoTextStyle, ascii::FONT_6X13},
            text::Text,
            Drawable,
        };
        use embedded_graphics::geometry::Point;
        use embedded_graphics_simulator::{
            OutputSettingsBuilder, SimulatorEvent, Window, sdl2::Keycode,
        };

        let mut display = make_display();
        let mut app = UiApp::new();
        let mut state = state_with(SystemTask::Idle);

        // Écrans encore en `todo!()` dans `Screens::draw` : les dessiner
        // ferait paniquer la fenêtre en pleine démonstration. On affiche un
        // texte à la place — à supprimer au fur et à mesure qu'ils sont
        // implémentés.
        let draw = |display: &mut SimulatorDisplay<Rgb565>,
                    app: &UiApp,
                    state: &SharedState| {
            match app.current_screen() {
                Screen::Idle | Screen::ManualControl | Screen::Data | Screen::Info => {
                    display.clear(crate::ui::theme::BACKGROUND_COLOR).unwrap();
                    Text::new(
                        "Ecran pas encore implemente - clic pour revenir",
                        Point::new(10, 120),
                        MonoTextStyle::new(&FONT_6X13, crate::ui::theme::DIM_COLOR),
                    )
                    .draw(display)
                    .unwrap();
                }
                _ => app.draw(display, state).unwrap(),
            }
        };

        draw(&mut display, &app, &state);

        let output_settings = OutputSettingsBuilder::new().scale(2).build();
        let mut window = Window::new("Cloud Chamber - UI (live)", &output_settings);

        'running: loop {
            window.update(&display);
            for event in window.events() {
                let action = match event {
                    SimulatorEvent::Quit => break 'running,
                    SimulatorEvent::KeyDown { keycode, .. } => match keycode {
                        Keycode::Escape => break 'running,
                        Keycode::Right | Keycode::Down => LiveAction::RotateRight,
                        Keycode::Left | Keycode::Up => LiveAction::RotateLeft,
                        Keycode::Return | Keycode::Space => LiveAction::Click,
                        Keycode::N => LiveAction::AdvancePhase,
                        Keycode::R => LiveAction::Reset,
                        _ => continue,
                    },
                    _ => continue,
                };

                apply_live_action(&mut app, &mut state, action);
                std::println!("{action:?} -> ecran {:?}, etat {:?}", app.current_screen(), state.task);
                draw(&mut display, &app, &state);
            }
        }
    }

    /// Le parcours complet tel que la boucle le vit : rotation vers les
    /// réglages, entrée dedans, retour, puis démarrage.
    #[test]
    fn a_realistic_sequence_of_events_never_panics() {
        let mut app = UiApp::new();
        let state = state_with(SystemTask::Idle);

        for event in [
            EncoderEvent::RotateClockwise,  // -> Stats
            EncoderEvent::RotateClockwise,  // -> Settings
            EncoderEvent::ButtonPressed,    // entre dans Settings
            EncoderEvent::RotateClockwise,
            EncoderEvent::RotateCounterClockwise,
            EncoderEvent::None,
        ] {
            app.handle_event(event, state.task);
            if app.take_redraw_request() {
                let mut d = make_display();
                app.draw(&mut d, &state).unwrap();
            }
        }
    }
}
