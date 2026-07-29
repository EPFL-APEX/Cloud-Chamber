//! Route les entrées (encodeur) et le rendu vers l'écran actuellement affiché.
//!
//! Compose [`super::navigator::Navigator`] (pile générique, ne connaît aucun
//! écran concret) et `super::screens::*` (écrans concrets, ne connaissent
//! pas la pile) — aucun des deux ne dépend de l'autre : c'est ce module, le
//! parent commun, qui les assemble.

use embedded_graphics::{draw_target::DrawTarget, geometry::OriginDimensions, pixelcolor::Rgb565};

use crate::shared::data::SharedState;

use super::interactions::{Click, NavAction, Rotary};
use super::navigator::{Navigator, Screen};
use super::screens::menu::MainMenuScreen;
use super::screens::stats::StatsScreen;

const NAV_DEPTH: usize = 8;

/// Possède la pile de navigation et les écrans à état persistant (seul
/// `MainMenuScreen` pour l'instant — `StatsScreen` n'a pas d'état propre,
/// elle emprunte `&SharedState` et est reconstruite à la volée dans
/// [`Screens::draw`]). Point d'entrée public unique de la navigation UI.
pub struct Screens {
    navigator: Navigator<NAV_DEPTH>,
    main_menu: MainMenuScreen,
}

impl Screens {
    pub fn new() -> Self {
        Self {
            navigator: Navigator::new(Screen::MainMenu),
            main_menu: MainMenuScreen::new(),
        }
    }

    /// Route l'entrée vers l'écran actuellement affiché.
    pub fn right_turn(&mut self) {
        match self.navigator.current() {
            Screen::MainMenu => self.main_menu.right_turn(),
        }
    }

    /// Route l'entrée vers l'écran actuellement affiché.
    pub fn left_turn(&mut self) {
        match self.navigator.current() {
            Screen::MainMenu => self.main_menu.left_turn(),
        }
    }

    /// Route le clic vers l'écran courant, puis applique la décision de
    /// navigation qu'il renvoie (cf. doc de [`NavAction`]).
    pub fn click(&mut self) {
        let action = match self.navigator.current() {
            Screen::MainMenu => self.main_menu.click(),
        };
        match action {
            Some(NavAction::Push(screen)) => self.navigator.push(screen),
            Some(NavAction::Back) => {
                self.navigator.handle_back();
            }
            None => {}
        }
    }

    /// Dessine l'écran actuellement affiché. `state` sert aux écrans
    /// dérivés (ex. `Stats`), construits ici plutôt que stockés.
    pub fn draw<D>(&self, display: &mut D, state: &SharedState) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        match self.navigator.current() {
            Screen::MainMenu => self.main_menu.draw(display),
            Screen::Stats => StatsScreen { state }.draw(display),
            Screen::Idle
            | Screen::Settings
            | Screen::Control
            | Screen::Cooldown
            | Screen::Data
            | Screen::Info => todo!("écran pas encore construit"),
        }
    }
}
