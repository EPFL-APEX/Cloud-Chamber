//! Écran de menu principal style Prusa.
//!
//! # Navigation
//!
//! - `select_up()` / `select_down()` : déplacent la sélection dans la liste
//! - `selected_item()` : retourne l'élément actuellement sélectionné
//!
//! La liste est statique (`MAIN_MENU_ITEMS`) — pas d'allocation heap.

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::Rgb565,
    primitives::{Primitive, PrimitiveStyleBuilder, Rectangle},
    text::Text,
    Drawable,
};

use crate::ui::{navigator::Screen::MainMenu, theme, widgets::{MenuItem, StatusBar}};

/// Entrées du menu principal.
pub enum MainMenuItem {
    CONTROL,
    STATS,
    SETTINGS,
    COOLDOWN,
    DATA,
    INFO,
}

const MAIN_MENU_SIZE : u8 = 6;//MainMenuItem::into(MainMenuItem::INFO);

/// Écran de menu principal.
pub struct MainMenuScreen {
    pub selected: u8,
}

impl MainMenuScreen {
    pub fn new() -> Self {
        Self { selected: 0 }
    }

    pub fn select_next(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn select_previous(&mut self) {
        if self.selected + 1 < MAIN_MENU_SIZE {
            self.selected += 1;
        }
    }

    pub fn selected_item(&self) -> MainMenuItem {
        //MainMenuItem::from(self.selected);
        todo!()
    }

    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        todo!();
        // Fond
        let bg_style = PrimitiveStyleBuilder::new().fill_color(theme::BG).build();
        Rectangle::new(Point::zero(), Size::new(320, 240))
            .into_styled(bg_style)
            .draw(display)?;

        // En-tête
        StatusBar { title: "Menu principal", state_color: theme::ACCENT }.draw(display)?;

        // Liste des éléments
        //for (i, &label) in MAIN_MENU_SIZE {
        //    MenuItem {
        //        label,
        //        origin: Point::new(0, 24 + i as i32 * 18),
        //        selected: i == self.selected,
        //    }.draw(display)?;
        //}

        // Flèche indicatrice de sélection
        let arrow_style = MonoTextStyle::new(&FONT_6X10, theme::ACCENT);
        let arrow_y = 24 + self.selected as i32 * 18 + 12;
        Text::new(">", Point::new(300, arrow_y), arrow_style).draw(display)?;

        Ok(())
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, Line, Rectangle, PrimitiveStyle},
    mono_font::{ascii::FONT_6X9, MonoTextStyle},
    text::Text,
    };

    use embedded_graphics_simulator::{SimulatorDisplay, Window, OutputSettingsBuilder, BinaryColorTheme};

    fn make_display() -> SimulatorDisplay<Rgb565> {
        SimulatorDisplay::new(Size::new(320, 240))
    }

    #[test]
    fn initial_selection_is_first_item() {
        let menu = MainMenuScreen::new();
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn select_previous_increments() {
        let mut menu = MainMenuScreen::new();
        menu.select_previous();
        assert_eq!(menu.selected, 1);
    }

    #[test]
    fn select_next_at_top_stays() {
        let mut menu = MainMenuScreen::new();
        menu.select_next();
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn select_previous_at_bottom_stays() {
        let mut menu = MainMenuScreen::new();
        for _ in 0..20 { menu.select_previous(); }
        assert_eq!(menu.selected, MAIN_MENU_SIZE - 1);
    }

    #[test]
    fn menu_draws_without_error() {
        let mut d = make_display();
        MainMenuScreen::new().draw(&mut d).unwrap();
    }

    //#[test]
    //fn selected_item_returns_correct_label() {
    //    let mut menu = MainMenuScreen::new();
    //    menu.select_next();
    //    todo!()
    //}

    #[test]
    fn test_() {
        let mut display = SimulatorDisplay::<BinaryColor>::new(Size::new(128, 64));

        let line_style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
        let text_style = MonoTextStyle::new(&FONT_6X9, BinaryColor::On);

        Circle::new(Point::new(72, 8), 48)
            .into_styled(line_style)
            .draw(&mut display);

        Line::new(Point::new(48, 16), Point::new(8, 16))
            .into_styled(line_style)
            .draw(&mut display);

        Line::new(Point::new(48, 16), Point::new(64, 32))
            .into_styled(line_style)
            .draw(&mut display);

        Rectangle::new(Point::new(79, 15), Size::new(34, 34))
            .into_styled(line_style)
            .draw(&mut display);

        Text::new("Hello World!", Point::new(5, 5), text_style).draw(&mut display);

        let output_settings = OutputSettingsBuilder::new()
            .theme(BinaryColorTheme::OledBlue)
            .build();
        Window::new("Hello World", &output_settings).show_static(&display);
}

}
