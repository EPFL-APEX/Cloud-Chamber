//! Écran d'erreur fatale : le cœur de contrôle a paniqué.
//!
//! Dessiné par la boucle du cœur 0 quand [`SharedState::core1_panicked`]
//! a été vu — c'est le pendant écran du gestionnaire de panique de
//! `drivers::safe_panic` : les sorties sont déjà coupées (la coupure
//! précède la publication du drapeau, cf. l'ordre dans le gestionnaire),
//! il ne reste qu'à le dire à l'opérateur. Sans cet écran, le cœur 0
//! continuerait d'afficher une machine normale pendant que plus rien ne
//! pilote les relais.
//!
//! Un seul geste disponible, affiché comme tel : **clic = redémarrage
//! logiciel** (reset watchdog, cf. la boucle du cœur 0 dans `main.rs`).
//! C'est un vrai redémarrage — la reprise n'existe pas, la panique a tué
//! la boucle de contrôle et rien ne la ressuscite : le reset est le seul
//! chemin de retour, autant l'offrir à l'opérateur plutôt que de lui
//! demander de couper l'alimentation.

use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{MonoTextStyle, ascii::{FONT_6X13, FONT_9X18_BOLD}},
    pixelcolor::Rgb565,
    primitives::{Primitive, PrimitiveStyleBuilder, Rectangle},
    text::Text,
};

use crate::ui::theme;

/// Écran d'erreur fatale — cœur de contrôle mort ou initialisation
/// impossible.
///
/// `message` porte la cause en toutes lettres : c'est la seule information
/// que l'opérateur n'a pas déjà, tout le reste est identique d'une
/// erreur à l'autre. Sans accents ni glyphe exotique — les fontes
/// `ascii`.
pub struct ErrorScreen;

impl ErrorScreen {
    pub fn draw<D>(&self, display: &mut D, message: &str) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        let bg = PrimitiveStyleBuilder::new().fill_color(theme::BACKGROUND_COLOR).build();
        Rectangle::new(Point::zero(), Size::new(320, 240))
            .into_styled(bg)
            .draw(display)?;

        Text::new(
            "ERREUR FATALE",
            Point::new(20, 40),
            MonoTextStyle::new(&FONT_9X18_BOLD, theme::DANGER_COLOR),
        )
        .draw(display)?;

        Text::new(
            message,
            Point::new(20, 80),
            MonoTextStyle::new(&FONT_6X13, theme::TEXT_COLOR),
        )
        .draw(display)?;

        Text::new(
            "Sorties de puissance coupees.",
            Point::new(20, 100),
            MonoTextStyle::new(&FONT_6X13, theme::TEXT_COLOR),
        )
        .draw(display)?;

        Text::new(
            "Clic pour redemarrer la machine.",
            Point::new(20, 130),
            MonoTextStyle::new(&FONT_6X13, theme::WARNING_COLOR),
        )
        .draw(display)?;

        // Le même bouton physique que partout ailleurs, dessiné comme le
        // bouton de l'écran de suivi : le clic est l'action, il n'y a pas
        // d'autre geste possible ici.
        Rectangle::new(Point::new(190, 180), Size::new(120, 40))
            .into_styled(embedded_graphics::primitives::PrimitiveStyleBuilder::new()
                .fill_color(theme::SUCCESS_COLOR)
                .stroke_color(theme::ACCENT_COLOR)
                .stroke_width(1)
                .build())
            .draw(display)?;
        Text::new(
            "REDEMARRER",
            Point::new(250 - (10 * 9) / 2, 196),
            MonoTextStyle::new(&FONT_9X18_BOLD, theme::BACKGROUND_COLOR),
        )
        .draw(display)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_graphics::pixelcolor::Rgb565;
    use embedded_graphics_simulator::SimulatorDisplay;

    #[test]
    fn draws_without_error() {
        let mut d = SimulatorDisplay::<Rgb565>::new(Size::new(320, 240));
        ErrorScreen.draw(&mut d, "Bus 1-Wire inutilisable.").unwrap();
    }
}
