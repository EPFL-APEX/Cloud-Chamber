//! Route les entrées (encodeur) et le rendu vers l'écran actuellement affiché.
//!
//! Compose [`super::navigator::Navigator`] (pile générique, ne connaît aucun
//! écran concret) et `super::screens::*` (écrans concrets, ne connaissent
//! pas la pile) — aucun des deux ne dépend de l'autre : c'est ce module, le
//! parent commun, qui les assemble.

use embedded_graphics::{draw_target::DrawTarget, geometry::OriginDimensions, pixelcolor::Rgb565};

use crate::cloud_chamber_hal::config::CHAMBER_TEMP_IDX;
use crate::config::settings::Settings;
use crate::shared::data::{SharedState, SystemTask};

use super::interactions::{Click, NavAction, Rotary};
use super::navigator::{Navigator, Screen};
use super::screens::menu::MainMenuScreen;
use super::screens::placeholder::PlaceholderScreen;
use super::screens::running::RunningScreen;
use super::screens::settings::SettingsScreen;
use super::screens::stats::StatsScreen;
use super::screens::temp::TempGraphScreen;

const NAV_DEPTH: usize = 8;

/// Possède la pile de navigation et les écrans à état persistant
/// (`MainMenuScreen`, `SettingsScreen`, `TempGraphScreen` qui accumule son
/// historique). `StatsScreen` et `RunningScreen` n'ont pas d'état propre :
/// elles empruntent `&SharedState` et sont reconstruites à la volée dans
/// [`Screens::draw`]. Point d'entrée public unique de la navigation UI.
pub struct Screens {
    navigator: Navigator<NAV_DEPTH>,
    main_menu: MainMenuScreen,
    settings: SettingsScreen,
    temp_graph: TempGraphScreen,
    /// Un clic a eu lieu sur l'écran de suivi. Devient un acquittement de
    /// sécurité, ou rien du tout, selon l'état de la machine au moment où
    /// [`Screens::take_task_request`] le relit — cf. sa doc.
    acknowledge_requested: bool,
}

impl Screens {
    pub fn new() -> Self {
        Self {
            navigator: Navigator::new(Screen::MainMenu),
            main_menu: MainMenuScreen::new(),
            settings: SettingsScreen::new(),
            temp_graph: TempGraphScreen::new(),
            acknowledge_requested: false,
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
            // la machine sur un simple geste. La veille est du même genre,
            // `UiApp` consommant le geste de réveil avant d'arriver ici.
            // Les écrans encore absents (cf. `PlaceholderScreen`) sont dans
            // le même cas : rien à faire défiler sur un carton d'attente.
            CurrentTask | Stats | Idle | ManualControl | Data | Info => {}
        }
    }

    /// Route l'entrée vers l'écran actuellement affiché.
    pub fn left_turn(&mut self) {
        use Screen::*;
        match self.navigator.current() {
            MainMenu => self.main_menu.left_turn(),
            Settings => self.settings.left_turn(),
            CurrentTask | Stats | Idle | ManualControl | Data | Info => {}
        }
    }

    /// Route le clic vers l'écran courant, puis applique la décision de
    /// navigation qu'il renvoie (cf. doc de [`NavAction`]).
    pub fn click(&mut self) {
        use Screen::*;
        let action = match self.navigator.current() {
            Screen::MainMenu => self.main_menu.click(),
            Settings => self.settings.click(),
            // L'écran de suivi porte la bannière « ARRET SECURITE » : c'est
            // donc là que l'opérateur acquitte un déclenchement. Le clic
            // ressort au menu comme sur les autres écrans d'affichage seul,
            // et lève en plus ce drapeau — `take_task_request` décidera s'il
            // vaut acquittement, lui seul connaissant l'état de la machine.
            CurrentTask => {
                self.acknowledge_requested = true;
                Some(NavAction::Back)
            }
            // Affichage seul : le clic ne peut que ressortir. Sans ça,
            // l'opérateur resterait coincé sur l'écran. Les écrans encore
            // absents en font partie — c'est même leur seule sortie, et ce
            // que leur carton d'attente annonce à l'opérateur.
            Stats | ManualControl | Data | Info => Some(NavAction::Back),
            // La veille se quitte par `Screens::leave_idle`, pas par la
            // pile. Un `Back` ici dépilerait deux fois.
            Idle => None,
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
    /// Utile à l'appelant qui doit savoir *où* il est sans avoir à
    /// dessiner (journalisation sur cible, assertions de test).
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
    ///
    /// # Acquittement d'un déclenchement sécurité
    ///
    /// Un clic sur l'écran de suivi pendant que la machine est `Tripped`
    /// demande `Idle`. C'est tout ce qu'il faut : `control_loop::tick` voit
    /// passer cet `Idle` alors que `SafetyMonitor` est encore déclenché, et
    /// en déduit l'acquittement opérateur — le canal existant suffit, pas
    /// besoin d'un second drapeau partagé entre les cœurs.
    ///
    /// Hors `Tripped`, ce clic ne demande rien : il n'a servi qu'à
    /// retourner au menu. C'est la même forme de garde que pour le
    /// démarrage juste en dessous, et elle vit ici pour la même raison —
    /// `tick` adopte par conception ce que l'UI écrit et ne peut pas
    /// distinguer les intentions.
    pub fn take_task_request(&mut self, current: SystemTask) -> Option<SystemTask> {
        // Consommé dans tous les cas, accordé ou non — un drapeau qui
        // traîne se déclencherait au prochain déclenchement sécurité, sans
        // que personne n'ait cliqué. Et sans arrêter là : le clic qui n'a
        // pas valu acquittement ne doit pas manger une demande de
        // démarrage en attente.
        let acknowledged = core::mem::take(&mut self.acknowledge_requested);
        if acknowledged && matches!(current, SystemTask::Tripped(_)) {
            return Some(SystemTask::Idle);
        }

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

    /// Alimente le graphe de veille, quel que soit l'écran affiché. Sinon
    /// elle s'ouvrirait sur un cadre vide.
    pub fn sample(&mut self, state: &SharedState) {
        if let Some(m) = state.snapshot.temps[CHAMBER_TEMP_IDX] {
            self.temp_graph.sample(m);
        }
    }

    /// Empile `Screen::Idle`. Faux si on y est déjà ou si la pile est
    /// pleine, donc rien à redessiner.
    pub fn enter_idle(&mut self) -> bool {
        if self.navigator.current() == Screen::Idle {
            return false;
        }
        self.navigator.push(Screen::Idle).is_ok()
    }

    /// Dépile la veille, la pile retrouve l'écran d'avant toute seule.
    /// Faux si on n'y était pas.
    pub fn leave_idle(&mut self) -> bool {
        if self.navigator.current() != Screen::Idle {
            return false;
        }
        self.navigator.pop();
        true
    }

    /// Dessine l'écran actuellement affiché. `state` sert à tous ceux qui
    /// montrent des mesures, qu'ils soient construits ici (`Stats`,
    /// `CurrentTask`) ou stockés (`MainMenu`, `Idle`).
    pub fn draw<D>(&self, display: &mut D, state: &SharedState) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        match self.navigator.current() {
            Screen::MainMenu => self.main_menu.draw(display, state),
            Screen::Settings => self.settings.draw(display),
            Screen::Stats => StatsScreen { state }.draw(display),
            Screen::CurrentTask => RunningScreen { state }.draw(display),
            Screen::Idle => self.temp_graph.draw(display, state),
            // Pas encore construits : un carton d'attente plutôt qu'une
            // panique, cf. `screens::placeholder`.
            //
            // Variantes listées explicitement, pas de `_ =>` : on garde le
            // contrôle d'exhaustivité sur `Screen`, pour qu'un écran ajouté
            // plus tard fasse échouer la compilation ici au lieu de se
            // retrouver silencieusement sans rendu.
            //
            // `for_screen` ne peut pas rendre `None` sur ces trois-là, mais
            // on reste sur `if let` plutôt que `.unwrap()` : rien ne
            // justifie de réintroduire une panique dans la fonction dont on
            // vient tout juste de la retirer.
            screen @ (Screen::ManualControl | Screen::Data | Screen::Info) => {
                if let Some(placeholder) = PlaceholderScreen::for_screen(screen) {
                    placeholder.draw(display)?;
                }
                Ok(())
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

    // ─── Acquittement d'un déclenchement sécurité ───────────────────────

    /// Depuis l'écran de suivi — celui qui porte la bannière « ARRET
    /// SECURITE » — un clic pendant un déclenchement demande `Idle`, ce que
    /// `control_loop::tick` lit comme l'acquittement opérateur.
    #[test]
    fn clicking_the_running_screen_while_tripped_asks_for_idle() {
        let mut screens = Screens::new();
        screens.click(); // menu -> écran de suivi
        assert_eq!(screens.current(), Screen::CurrentTask);
        let _ = screens.take_task_request(SystemTask::Idle); // purge le démarrage

        screens.click();
        assert_eq!(
            screens.take_task_request(SystemTask::Tripped(SafetyCause::CompressorOverheat)),
            Some(SystemTask::Idle),
        );
        assert_eq!(screens.current(), Screen::MainMenu, "le clic ramene aussi au menu");
    }

    /// Hors déclenchement, ce même clic ne demande rien : il n'a servi qu'à
    /// revenir au menu. Sinon un opérateur qui consulte son cycle en cours
    /// l'arrêterait en ressortant.
    #[test]
    fn clicking_the_running_screen_while_running_asks_for_nothing() {
        let mut screens = Screens::new();
        screens.click();
        let _ = screens.take_task_request(SystemTask::Idle);

        screens.click();
        assert_eq!(
            screens.take_task_request(SystemTask::Cooling(CoolingPhase::HighVoltage)),
            None,
        );
        assert_eq!(screens.current(), Screen::MainMenu);
    }

    /// Un clic non accordé ne doit pas laisser de drapeau derrière lui :
    /// il serait accordé au prochain déclenchement, sans que personne
    /// n'ait cliqué — et il mangerait au passage la demande de démarrage
    /// en attente.
    #[test]
    fn an_ungranted_acknowledgement_leaves_nothing_behind() {
        let mut screens = Screens::new();
        screens.click();
        let _ = screens.take_task_request(SystemTask::Idle);

        // Clic sur l'écran de suivi alors que la machine tourne : refusé.
        screens.click();
        assert_eq!(
            screens.take_task_request(SystemTask::Cooling(CoolingPhase::HighVoltage)),
            None,
        );

        // Le démarrage demandé ensuite depuis le menu passe normalement.
        screens.click();
        assert_eq!(
            screens.take_task_request(SystemTask::Idle),
            Some(SystemTask::Cooling(CoolingPhase::SensorCheck)),
        );

        // Et plus tard, un déclenchement ne trouve aucun drapeau en attente.
        assert_eq!(
            screens.take_task_request(SystemTask::Tripped(SafetyCause::CompressorOverheat)),
            None,
        );
    }

    /// Le chemin qui faisait tomber le cœur 0 : deux des six entrées du
    /// menu (Données, Info) poussaient un écran dont `draw` répondait
    /// `todo!()`. On le parcourt ici depuis le menu, comme l'opérateur, et
    /// on vérifie que l'écran se dessine, encaisse les rotations, et rend
    /// la main au clic.
    #[test]
    fn unbuilt_menu_entries_show_a_placeholder_instead_of_panicking() {
        use crate::ui::screens::menu::MainMenuItem;

        let state = state_with(SystemTask::Idle);

        for (item, screen) in [
            (MainMenuItem::DATA, Screen::Data),
            (MainMenuItem::INFO, Screen::Info),
        ] {
            let mut screens = Screens::new();
            for _ in 0..item as u8 {
                screens.right_turn();
            }
            screens.click();
            assert_eq!(screens.current(), screen);

            let mut d = make_display();
            screens.draw(&mut d, &state).unwrap();

            // Affichage seul : rien à faire défiler, mais rien ne casse.
            screens.right_turn();
            screens.left_turn();
            screens.draw(&mut d, &state).unwrap();

            // Seule sortie, celle que le carton d'attente annonce.
            screens.click();
            assert_eq!(screens.current(), Screen::MainMenu);
        }
    }

    /// `ManualControl` n'a pas d'entrée de menu aujourd'hui, mais reste
    /// atteignable par la pile de navigation — son rendu ne doit pas plus
    /// paniquer que les autres.
    #[test]
    fn manual_control_also_draws_without_panicking() {
        let mut screens = Screens::new();
        let _ = screens.navigator.push(Screen::ManualControl);

        let mut d = make_display();
        screens.draw(&mut d, &state_with(SystemTask::Idle)).unwrap();
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

    /// La veille s'empile par-dessus l'écran courant et se dépile, sans
    /// qu'aucun champ ait eu à mémoriser où on était.
    #[test]
    fn idle_stacks_and_unstacks_over_the_current_screen() {
        let mut screens = Screens::new();
        screens.click(); // menu -> suivi de cycle

        assert!(screens.enter_idle());
        assert_eq!(screens.current(), Screen::Idle);
        assert!(!screens.enter_idle(), "deja en veille");

        let mut d = make_display();
        screens.draw(&mut d, &state_with(SystemTask::Idle)).unwrap();

        assert!(screens.leave_idle());
        assert_eq!(screens.current(), Screen::CurrentTask);
        assert!(!screens.leave_idle(), "plus en veille");
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
