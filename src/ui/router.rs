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
    ///
    /// `current` est l'état de la machine au moment de l'appel : **un
    /// démarrage n'est accordé que depuis `Idle`**. Rappuyer sur le bouton
    /// pendant qu'un cycle tourne ne le renvoie donc pas à sa première
    /// phase — le clic a déjà fait son travail en basculant sur l'écran de
    /// suivi. La garde vit ici et pas dans `logic/` : `control_loop::tick`
    /// adopte *par conception* ce que l'UI écrit (c'est son mécanisme de
    /// réconciliation), il ne peut pas distinguer un démarrage voulu d'un
    /// redémarrage accidentel.
    ///
    /// La demande est consommée dans tous les cas, accordée ou non — sinon
    /// elle se déclencherait plus tard, au premier retour à l'arrêt, sans
    /// que personne ne l'ait redemandée.
    pub fn take_task_request(&mut self, current: SystemTask) -> Option<SystemTask> {
        let requested = self.main_menu.take_task_request()?;
        match requested {
            // La garde ne porte que sur le démarrage. Elle est écrite en
            // fonction de la demande, pas appliquée à tout : un futur
            // bouton d'arrêt passera par ce même canal et ne doit
            // évidemment pas exiger d'être déjà à l'arrêt.
            SystemTask::Cooling(_) => (current == SystemTask::Idle).then_some(requested),
            other => Some(other),
        }
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
    use crate::logic::cooling::CoolingPhase;
    use crate::logic::security::SafetyCause;
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
        assert_eq!(screens.take_task_request(SystemTask::Idle), None);

        screens.click();

        assert_eq!(
            screens.take_task_request(SystemTask::Idle),
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

        // De retour au menu, le premier item peut relancer un cycle — la
        // machine est repassée à l'arrêt entre-temps.
        screens.click();
        assert_eq!(
            screens.take_task_request(SystemTask::Idle),
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
        assert_eq!(screens.take_task_request(SystemTask::Idle), None);
    }

    // ─── Garde « on ne redémarre pas un cycle en cours » ─────────────────

    /// Le cas qui motive la garde : rappuyer sur le bouton pendant que la
    /// machine tourne ne doit pas la renvoyer à la première phase.
    #[test]
    fn pressing_start_again_mid_cycle_does_not_restart_the_sequence() {
        let mut screens = Screens::new();
        screens.click(); // démarrage depuis l'arrêt
        assert_eq!(
            screens.take_task_request(SystemTask::Idle),
            Some(SystemTask::Cooling(CoolingPhase::SensorCheck)),
        );

        // La machine a avancé ; l'opérateur revient au menu et re-clique.
        let running = SystemTask::Cooling(CoolingPhase::HighVoltage);
        screens.click(); // suivi -> menu
        screens.click(); // menu -> suivi, avec demande de démarrage
        assert_eq!(screens.take_task_request(running), None, "pas de redemarrage");

        // …mais l'écran de suivi est bien affiché, et il se dessine.
        assert_eq!(screens.current(), Screen::CurrentTask);
        let mut d = make_display();
        screens.draw(&mut d, &state_with(running)).unwrap();
    }

    /// Aucun état autre qu'`Idle` n'autorise un démarrage.
    #[test]
    fn no_state_other_than_idle_grants_a_start() {
        for busy in [
            SystemTask::Cooling(CoolingPhase::SensorCheck),
            SystemTask::Cooling(CoolingPhase::FinalCheckBeforeStabilising),
            SystemTask::Stabilising,
            SystemTask::Stopping(StoppingPhase::CutHighVoltage),
            SystemTask::Tripped(SafetyCause::CompressorOverheat),
        ] {
            let mut screens = Screens::new();
            screens.click();
            assert_eq!(screens.take_task_request(busy), None, "{busy:?}");
        }
    }

    /// Une demande refusée est quand même consommée : sinon elle
    /// s'appliquerait toute seule au prochain retour à l'arrêt.
    #[test]
    fn a_refused_request_is_not_kept_for_later() {
        let mut screens = Screens::new();
        screens.click();
        let running = SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa);
        assert_eq!(screens.take_task_request(running), None);

        // Machine revenue à l'arrêt, sans nouveau clic : rien ne doit
        // démarrer.
        assert_eq!(screens.take_task_request(SystemTask::Idle), None);
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
