//! Commande manuelle de la bande LED du compartiment machine.
//!
//! Deux lignes, la géométrie de l'écran de réglages : la première
//! bascule la bande (un clic = ON/OFF), la seconde quitte. Pas de mode
//! édition — il n'y a qu'un bit à commander.
//!
//! # L'écran décide, il n'agit pas
//!
//! Même séparation que partout ailleurs (`ActuatorPlan`/`apply`,
//! `SettingsScreen::take_save_request`) : l'écran bascule son état
//! d'affichage **et** lève une demande, récupérée par la boucle du cœur 0
//! via [`LightsScreen::take_strip_request`], qui seule possède le GPIO
//! (`drivers::led_strip::LedStrip`). C'est ce qui rend l'écran testable
//! sans broche, et ce qui garantit qu'un clic pendant un rendu ne pilote
//! jamais le matériel depuis deux endroits.
//!
//! L'état affiché bascule immédiatement au clic — la demande est consommée
//! dans le même tour de boucle, l'écran et la bande ne peuvent pas diverger
//! plus d'un instant.

use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{MonoTextStyle, ascii::FONT_6X13},
    pixelcolor::Rgb565,
    primitives::{Primitive, PrimitiveStyle, Rectangle},
    text::{Baseline, Text, TextStyleBuilder},
};

use crate::ui::interactions::{Click, NavAction, Rotary};
use crate::ui::theme;

use super::widgets::{SettingLine, SettingLines};

const SCREEN_WIDTH: u32 = 320;
const TOP_BAND_HEIGHT: u32 = 29;
const LIST_TOP: i32 = 34;

/// La ligne de bascule, avant celle de sortie.
const TOGGLE_ROW: usize = 0;
const BACK_ROW: usize = 1;
const N_ROWS: usize = 2;

/// Écran de commande de l'éclairage du compartiment machine.
pub struct LightsScreen {
    selected: usize,
    /// État affiché — bascule au clic, appliqué à la bande par la boucle
    /// principale via `take_strip_request`.
    on: bool,
    /// Demande en attente d'être récupérée par la boucle principale.
    strip_requested: Option<bool>,
}

impl LightsScreen {
    pub fn new() -> Self {
        // Éteinte, comme le driver au démarrage (`LedStrip::new` force
        // la broche bas) : l'écran et la bande partent d'accord sans
        // avoir à se parler.
        Self { selected: TOGGLE_ROW, on: false, strip_requested: None }
    }

    /// Récupère l'état demandé pour la bande, et le consomme.
    pub fn take_strip_request(&mut self) -> Option<bool> {
        self.strip_requested.take()
    }

    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        display.clear(theme::BACKGROUND_COLOR)?;

        Rectangle::new(Point::new(0, 0), Size::new(SCREEN_WIDTH, TOP_BAND_HEIGHT))
            .into_styled(PrimitiveStyle::with_fill(theme::BACKGROUND_COLOR_DARKER))
            .draw(display)?;

        let title_style = TextStyleBuilder::new().baseline(Baseline::Top).build();
        Text::with_text_style(
            "Lights",
            Point::new(10, 8),
            MonoTextStyle::new(&FONT_6X13, theme::HIGHLIGHT_COLOR),
            title_style,
        )
        .draw(display)?;

        let lines = [
            SettingLine { label: "Machine bay LEDs", value: if self.on { "ON" } else { "OFF" } },
            SettingLine { label: "Back", value: "" },
        ];

        SettingLines::<N_ROWS> {
            lines,
            selected: self.selected,
            editing: false,
            top: LIST_TOP,
        }
        .draw(display)
    }
}

impl Rotary for LightsScreen {
    fn right_turn(&mut self) {
        if self.selected + 1 < N_ROWS {
            self.selected += 1;
        }
    }

    fn left_turn(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }
}

impl Click for LightsScreen {
    /// La ligne de bascule inverse l'état et lève la demande ; la ligne
    /// Back quitte. Éteindre n'exige pas de confirmation — la bande n'est
    /// pas une sortie de puissance, se tromper se corrige d'un autre clic.
    fn click(&mut self) -> Option<NavAction> {
        if self.selected == TOGGLE_ROW {
            self.on = !self.on;
            self.strip_requested = Some(self.on);
            return None;
        }
        Some(NavAction::Back)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_graphics_simulator::SimulatorDisplay;

    fn make_display() -> SimulatorDisplay<Rgb565> {
        SimulatorDisplay::new(Size::new(SCREEN_WIDTH, 240))
    }

    #[test]
    fn starts_on_the_toggle_row_off() {
        let mut screen = LightsScreen::new();
        assert_eq!(screen.selected, TOGGLE_ROW);
        assert!(!screen.on);
        assert_eq!(screen.take_strip_request(), None);
    }

    #[test]
    fn a_click_on_the_toggle_row_inverts_and_requests() {
        let mut screen = LightsScreen::new();
        assert!(screen.click().is_none());
        assert!(screen.on);
        assert_eq!(screen.take_strip_request(), Some(true));
        assert_eq!(screen.take_strip_request(), None, "la demande ne vaut qu'une fois");
        assert!(screen.click().is_none());
        assert!(!screen.on);
        assert_eq!(screen.take_strip_request(), Some(false));
    }

    #[test]
    fn the_back_row_leaves_the_strip_untouched() {
        let mut screen = LightsScreen::new();
        screen.right_turn();
        assert_eq!(screen.click(), Some(NavAction::Back));
        assert!(!screen.on);
        assert_eq!(screen.take_strip_request(), None);
    }

    #[test]
    fn selection_stops_at_both_ends() {
        let mut screen = LightsScreen::new();
        screen.left_turn();
        assert_eq!(screen.selected, TOGGLE_ROW);
        screen.right_turn();
        screen.right_turn();
        assert_eq!(screen.selected, BACK_ROW);
    }

    #[test]
    fn draws_in_both_states() {
        let mut d = make_display();
        let mut screen = LightsScreen::new();
        screen.draw(&mut d).unwrap();
        screen.click(); // ON
        screen.draw(&mut d).unwrap();
    }
}
