//! Écran de remplacement pour les entrées de menu dont l'écran définitif
//! n'existe pas encore.
//!
//! # Pourquoi un écran plutôt qu'un `todo!()`
//!
//! `Screens::draw` répondait par `todo!("écran pas encore construit")` aux
//! trois écrans non implémentés. Deux d'entre eux — `Data` et `Info` — sont
//! poussés directement par le menu principal
//! (`screens::menu::MainMenuScreen::click`) : deux des six entrées faisaient
//! donc paniquer le cœur 0 en trois clics.
//!
//! Sur cette machine, une panique de l'interface n'arrête pas la chambre :
//! la boucle de contrôle vit sur le cœur 1 et poursuit son cycle, haute
//! tension comprise, pendant que l'opérateur n'a plus ni affichage ni
//! bouton pour l'interrompre. Un écran « pas encore disponible », lui,
//! laisse la machine pilotable.
//!
//! # Layout 320×240
//!
//! ```text
//! ┌────────────────────────────────┐ y=0
//! │                                │
//! │  TITRE                         │ y=100  (FONT_9X18_BOLD, WARNING)
//! │  Ecran pas encore disponible   │ y=126  (FONT_6X13, DIM)
//! │  Clic pour revenir             │ y=148  (FONT_6X13, DIM)
//! │                                │
//! └────────────────────────────────┘ y=240
//! ```
//!
//! À supprimer au fur et à mesure que les écrans réels arrivent : le jour
//! où `Screens::draw` n'a plus de bras qui pointe ici, ce fichier part avec.

use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{MonoTextStyle, ascii::{FONT_6X13, FONT_9X18_BOLD}},
    pixelcolor::Rgb565,
    primitives::{Primitive, PrimitiveStyleBuilder, Rectangle},
    text::Text,
};

use crate::ui::{navigator::Screen, theme};

/// Écran « pas encore disponible », identifié par le titre de l'entrée de
/// menu qui y mène.
pub struct PlaceholderScreen {
    title: &'static str,
}

impl PlaceholderScreen {
    /// L'écran de remplacement correspondant à `screen`.
    ///
    /// Renvoie `None` pour tout écran réellement implémenté : c'est le
    /// typage qui garde la liste à jour, plutôt qu'un libellé par défaut
    /// qui masquerait un oubli de câblage.
    pub fn for_screen(screen: Screen) -> Option<Self> {
        let title = match screen {
            Screen::ManualControl => "COMMANDE MANUELLE",
            Screen::Data => "DONNEES",
            Screen::Info => "INFOS",
            Screen::Idle
            | Screen::MainMenu
            | Screen::Settings
            | Screen::Stats
            | Screen::CurrentTask => return None,
        };
        Some(Self { title })
    }

    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        let bg = PrimitiveStyleBuilder::new().fill_color(theme::BACKGROUND_COLOR).build();
        Rectangle::new(Point::zero(), Size::new(320, 240)).into_styled(bg).draw(display)?;

        Text::new(
            self.title,
            Point::new(20, 100),
            MonoTextStyle::new(&FONT_9X18_BOLD, theme::WARNING_COLOR),
        )
        .draw(display)?;

        Text::new(
            "Ecran pas encore disponible",
            Point::new(20, 126),
            MonoTextStyle::new(&FONT_6X13, theme::DIM_COLOR),
        )
        .draw(display)?;

        Text::new(
            "Clic pour revenir",
            Point::new(20, 148),
            MonoTextStyle::new(&FONT_6X13, theme::DIM_COLOR),
        )
        .draw(display)?;

        Ok(())
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_graphics::pixelcolor::Rgb565;
    use embedded_graphics_simulator::SimulatorDisplay;

    fn display() -> SimulatorDisplay<Rgb565> {
        SimulatorDisplay::new(Size::new(320, 240))
    }

    #[test]
    fn unimplemented_screens_have_a_placeholder() {
        for screen in [Screen::ManualControl, Screen::Data, Screen::Info] {
            assert!(
                PlaceholderScreen::for_screen(screen).is_some(),
                "{screen:?} devrait avoir un ecran de remplacement"
            );
        }
    }

    /// Le jour où un de ces écrans est implémenté, `for_screen` doit cesser
    /// de le revendiquer — sinon le vrai écran ne s'afficherait jamais.
    #[test]
    fn implemented_screens_have_none() {
        for screen in [
            Screen::Idle,
            Screen::MainMenu,
            Screen::Settings,
            Screen::Stats,
            Screen::CurrentTask,
        ] {
            assert!(
                PlaceholderScreen::for_screen(screen).is_none(),
                "{screen:?} a un vrai ecran, pas de remplacement attendu"
            );
        }
    }

    #[test]
    fn draws_without_error() {
        let mut d = display();
        for screen in [Screen::ManualControl, Screen::Data, Screen::Info] {
            PlaceholderScreen::for_screen(screen).unwrap().draw(&mut d).unwrap();
        }
    }
}
