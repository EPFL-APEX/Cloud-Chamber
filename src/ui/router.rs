//! Route les entrées (encodeur) et le rendu vers l'écran actuellement affiché.
//!
//! Compose [`super::navigator::Navigator`] (pile générique, ne connaît aucun
//! écran concret) et `super::screens::*` (écrans concrets, ne connaissent
//! pas la pile) — aucun des deux ne dépend de l'autre : c'est ce module, le
//! parent commun, qui les assemble.

use embedded_graphics::{draw_target::DrawTarget, geometry::OriginDimensions, pixelcolor::Rgb565};

use crate::config::settings::Settings;
use crate::shared::data::{SharedState, SystemTask};

use super::interactions::{Click, NavAction, Rotary};
use super::navigator::{Navigator, Screen};
use super::screens::menu::MainMenuScreen;
use super::screens::running::RunningScreen;
use super::screens::settings::SettingsScreen;
use super::screens::stats::StatsScreen;

const NAV_DEPTH: usize = 8;

/// Possède la pile de navigation et les écrans à état persistant
/// (`MainMenuScreen`, `SettingsScreen`). `StatsScreen` et `RunningScreen`
/// n'ont pas d'état propre : elles empruntent `&SharedState` et sont
/// reconstruites à la volée dans [`Screens::draw`]. Point d'entrée public
/// unique de la navigation UI.
pub struct Screens {
    navigator: Navigator<NAV_DEPTH>,
    main_menu: MainMenuScreen,
    settings: SettingsScreen,
}

impl Screens {
    pub fn new() -> Self {
        Self {
            navigator: Navigator::new(Screen::MainMenu),
            main_menu: MainMenuScreen::new(),
            settings: SettingsScreen::new(),
        }
    }

    /// Route l'entrée vers l'écran actuellement affiché.
    pub fn right_turn(&mut self) {
        use Screen::*;
        match self.navigator.current() {
            MainMenu => self.main_menu.right_turn(),
            Settings => self.settings.right_turn(),
            // Écrans d'affichage seul : rien à faire défiler. Ignorer une
            // rotation est le bon comportement — `todo!()` faisait paniquer
            // la machine sur un simple geste.
            CurrentTask | Stats => {}
            Idle => todo!(),
            ManualControl => todo!(),
            Data => todo!(),
            Info => todo!(),
        }
    }

    /// Route l'entrée vers l'écran actuellement affiché.
    pub fn left_turn(&mut self) {
        use Screen::*;
        match self.navigator.current() {
            MainMenu => self.main_menu.left_turn(),
            Settings => self.settings.left_turn(),
            CurrentTask | Stats => {}
            Idle => todo!(),
            ManualControl => todo!(),
            Data => todo!(),
            Info => todo!(),
        }
    }

    /// Route le clic vers l'écran courant, puis applique la décision de
    /// navigation qu'il renvoie (cf. doc de [`NavAction`]).
    pub fn click(&mut self) {
        use Screen::*;
        let action = match self.navigator.current() {
            Screen::MainMenu => self.main_menu.click(),
            Settings => self.settings.click(),
            // Affichage seul : le clic ne peut que ressortir. Sans ça,
            // l'opérateur resterait coincé sur l'écran.
            CurrentTask | Stats => Some(NavAction::Back),
            Idle => todo!(),
            ManualControl => todo!(),
            Data => todo!(),
            Info => todo!(),
        };
        match action {
            // Pile pleine (`NAV_DEPTH` écrans empilés) : on reste où on est
            // plutôt que de paniquer. Un `.unwrap()` ici faisait tomber la
            // machine sur un excès de clics — jamais acceptable pendant un
            // cycle, et c'est déjà ce que `Navigator::push` documente.
            Some(NavAction::Push(screen)) => {
                let _ = self.navigator.push(screen);
            }
            Some(NavAction::Back) => {
                self.navigator.pop();
            }
            None => {}
        }
    }

    /// Écran actuellement affiché.
    ///
    /// Utile à l'appelant qui doit savoir *où* il est sans avoir à dessiner
    /// (journalisation sur cible, harnais interactif qui contourne les
    /// écrans encore en `todo!()`).
    pub fn current(&self) -> Screen {
        self.navigator.current()
    }

    /// Récupère une éventuelle demande de sauvegarde en flash levée par
    /// l'écran de réglages. `None` la plupart du temps — à consommer
    /// depuis la boucle principale (pas encore câblé : ce projet n'a pas
    /// encore de point d'entrée matériel/`main.rs`, cf. `SettingsStore`).
    pub fn take_save_request(&mut self) -> Option<Settings> {
        self.settings.take_save_request()
    }

    /// Récupère un changement d'état demandé par l'opérateur (démarrage de
    /// cycle depuis le menu principal), et le consomme.
    ///
    /// À appeler après [`Screens::click`] — l'appelant est seul à écrire
    /// dans `SHARED_STATE.task`, que `logic::control_loop::tick` adopte au
    /// tour suivant. Cf. [`super::screens::menu::MainMenuScreen::take_task_request`].
    pub fn take_task_request(&mut self) -> Option<SystemTask> {
        self.main_menu.take_task_request()
    }

    /// Dessine l'écran actuellement affiché. `state` sert aux écrans
    /// dérivés (ex. `Stats`, `CurrentTask`), construits ici plutôt que
    /// stockés.
    pub fn draw<D>(&self, display: &mut D, state: &SharedState) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        match self.navigator.current() {
            Screen::MainMenu => self.main_menu.draw(display),
            Screen::Settings => self.settings.draw(display),
            Screen::Stats => StatsScreen { state }.draw(display),
            Screen::CurrentTask => RunningScreen { state }.draw(display),
            Screen::Idle | Screen::ManualControl | Screen::Data | Screen::Info => {
                todo!("écran pas encore construit")
            }
        }
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

    /// Le parcours complet du premier bouton : depuis le menu, un clic ouvre
    /// l'écran de suivi *et* rend disponible la demande de démarrage.
    #[test]
    fn clicking_the_first_menu_item_starts_a_cycle_and_shows_it() {
        let mut screens = Screens::new();
        assert_eq!(screens.take_task_request(), None);

        screens.click();

        assert_eq!(
            screens.take_task_request(),
            Some(SystemTask::Cooling(CoolingPhase::SensorCheck)),
        );

        // Et l'écran affiché est bien celui du cycle : il se dessine sans
        // paniquer, y compris une fois la machine passée en Cooling.
        let mut d = make_display();
        let state = state_with(SystemTask::Cooling(CoolingPhase::PreCoolingThePlate));
        screens.draw(&mut d, &state).unwrap();
    }

    /// Une fois sur l'écran de suivi, tourner ne doit rien casser (c'est un
    /// affichage seul) et cliquer doit ramener au menu.
    #[test]
    fn the_running_screen_ignores_rotation_and_exits_on_click() {
        let mut screens = Screens::new();
        screens.click(); // menu -> écran de suivi

        screens.right_turn();
        screens.left_turn();

        let state = state_with(SystemTask::Cooling(CoolingPhase::HighVoltage));
        let mut d = make_display();
        screens.draw(&mut d, &state).unwrap();

        screens.click(); // retour au menu
        let mut d = make_display();
        screens.draw(&mut d, &state).unwrap();

        // De retour au menu, le premier item peut relancer un cycle.
        screens.click();
        assert_eq!(
            screens.take_task_request(),
            Some(SystemTask::Cooling(CoolingPhase::SensorCheck)),
        );
    }

    /// Le menu principal ne doit pas démarrer de cycle par simple
    /// navigation — seul un clic sur le premier item compte.
    #[test]
    fn rotating_in_the_menu_never_requests_a_start() {
        let mut screens = Screens::new();
        for _ in 0..10 {
            screens.right_turn();
        }
        for _ in 0..10 {
            screens.left_turn();
        }
        assert_eq!(screens.take_task_request(), None);
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
            SystemTask::Stopping(CutHighVoltage) => SystemTask::Stopping(CutCompressor),
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

    /// Applique une action au routeur et à l'état simulé.
    ///
    /// Le traitement du clic reproduit exactement ce que fait l'ISR de
    /// `src/bin/ui_test.rs` sur matériel : router le clic, puis appliquer
    /// la demande d'état que l'écran a éventuellement levée. C'est ce qui
    /// rend ce harnais représentatif — sans cette ligne, le premier bouton
    /// changerait d'écran sans jamais rien démarrer.
    fn apply_live_action(screens: &mut Screens, state: &mut SharedState, action: LiveAction) {
        match action {
            LiveAction::RotateRight => screens.right_turn(),
            LiveAction::RotateLeft => screens.left_turn(),
            LiveAction::Click => {
                screens.click();
                if let Some(task) = screens.take_task_request() {
                    state.task = task;
                }
            }
            LiveAction::AdvancePhase => state.task = next_task(state.task),
            LiveAction::Reset => state.task = SystemTask::Idle,
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
        let mut screens = Screens::new();
        let mut state = state_with(SystemTask::Idle);

        apply_live_action(&mut screens, &mut state, LiveAction::Click);

        assert_eq!(state.task, SystemTask::Cooling(CoolingPhase::SensorCheck));
        assert_eq!(screens.current(), Screen::CurrentTask);
    }

    /// Et la doublure fait bien progresser la checklist, jusqu'au retour
    /// à l'arrêt — c'est tout l'intérêt de la touche dédiée.
    #[test]
    fn live_advance_walks_the_whole_sequence_back_to_idle() {
        let mut screens = Screens::new();
        let mut state = state_with(SystemTask::Idle);
        apply_live_action(&mut screens, &mut state, LiveAction::Click);

        let mut seen = std::vec::Vec::new();
        for _ in 0..12 {
            seen.push(state.task);
            if state.task == SystemTask::Idle && seen.len() > 1 {
                break;
            }
            apply_live_action(&mut screens, &mut state, LiveAction::AdvancePhase);
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
        let mut screens = Screens::new();
        let mut state = state_with(SystemTask::Idle);
        apply_live_action(&mut screens, &mut state, LiveAction::Click);
        assert_eq!(screens.current(), Screen::CurrentTask);

        for _ in 0..11 {
            let mut d = make_display();
            screens.draw(&mut d, &state).unwrap();
            apply_live_action(&mut screens, &mut state, LiveAction::AdvancePhase);
        }
    }

    #[test]
    fn live_reset_brings_the_machine_back_to_idle() {
        let mut screens = Screens::new();
        let mut state = state_with(SystemTask::Idle);
        apply_live_action(&mut screens, &mut state, LiveAction::Click);
        apply_live_action(&mut screens, &mut state, LiveAction::AdvancePhase);
        assert_ne!(state.task, SystemTask::Idle);

        apply_live_action(&mut screens, &mut state, LiveAction::Reset);
        assert_eq!(state.task, SystemTask::Idle);
    }

    /// Le pied de l'écran de suivi doit afficher une vraie mesure, pas
    /// `---` : toute action pose une lecture chambre cohérente.
    #[test]
    fn live_actions_always_leave_a_chamber_reading() {
        let mut screens = Screens::new();
        let mut state = state_with(SystemTask::Idle);
        for action in [
            LiveAction::RotateRight,
            LiveAction::Click,
            LiveAction::AdvancePhase,
            LiveAction::RotateLeft,
            LiveAction::Reset,
        ] {
            apply_live_action(&mut screens, &mut state, action);
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
        let mut screens = Screens::new();
        let mut state = state_with(SystemTask::Idle);

        // Écrans encore en `todo!()` dans `Screens::draw` : les dessiner
        // ferait paniquer la fenêtre en pleine démonstration. On affiche un
        // texte à la place — à supprimer au fur et à mesure qu'ils sont
        // implémentés.
        let draw = |display: &mut SimulatorDisplay<Rgb565>,
                    screens: &Screens,
                    state: &SharedState| {
            match screens.current() {
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
                _ => screens.draw(display, state).unwrap(),
            }
        };

        draw(&mut display, &screens, &state);

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

                apply_live_action(&mut screens, &mut state, action);
                std::println!("{action:?} -> ecran {:?}, etat {:?}", screens.current(), state.task);
                draw(&mut display, &screens, &state);
            }
        }
    }

    /// Empiler plus que `NAV_DEPTH` écrans ne doit pas paniquer.
    #[test]
    fn clicking_far_beyond_the_stack_depth_does_not_panic() {
        let mut screens = Screens::new();
        // Alterne menu -> suivi -> menu…, bien au-delà de NAV_DEPTH.
        for _ in 0..(NAV_DEPTH * 4) {
            screens.click();
        }
        let state = state_with(SystemTask::Idle);
        let mut d = make_display();
        screens.draw(&mut d, &state).unwrap();
    }
}
