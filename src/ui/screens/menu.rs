//! Écran de menu principal style Prusa.
//!
//! # Navigation
//!
//! - `select_next()` / `select_previous()` : déplacent la sélection dans la liste
//! - `selected_item()` : retourne l'élément actuellement sélectionné
//!
//! La liste est statique (`MAIN_MENU_ITEMS`) — pas d'allocation heap.

use embedded_graphics::{
    Drawable, draw_target::DrawTarget, geometry::{OriginDimensions, Point, Size}, image::{Image, ImageDrawable, ImageDrawableExt}, pixelcolor::{Rgb565, Rgb888}, primitives::{Primitive, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle},
};

use tinybmp::Bmp;
use num_enum::{TryFromPrimitive, IntoPrimitive};

use crate::ui::{navigator::Screen::MainMenu, theme, widgets::{MenuItem, StatusBar}};

/// Entrées du menu principal.
#[repr(u8)]
#[derive(TryFromPrimitive, IntoPrimitive)]
pub enum MainMenuItem {
    CONTROL,
    STATS,
    SETTINGS,
    COOLDOWN,
    DATA,
    INFO,
}

const MAIN_MENU_SIZE : u8 = 6; // core::mem::variant_count::<MainMenuItem>() as u8;

/// Écran de menu principal.
pub struct MainMenuScreen {
    pub selected: u8,
}

impl MainMenuScreen {
    pub fn new() -> Self {
        Self { selected: 0 }
    }

    pub fn select_next(&mut self) {
        if self.selected + 1 < MAIN_MENU_SIZE {
            self.selected += 1;
        }
    }

    pub fn select_previous(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn selected_item(&self) -> MainMenuItem {
        MainMenuItem::try_from(self.selected).unwrap()
    }

    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {

        // BACKGROUND
        const BACKGROUND_COLOR:Rgb565 = Rgb565::new(0, 5, 4);
        display.clear(BACKGROUND_COLOR);
        
        
        const HIGHLIGHT_COLOR: Rgb565 = Rgb565::new(1, 29, 23);

        // STRUCTURE UI
        const SCREEN_SIZE:(u32, u32) = (320, 240);

        const BACKGROUND_COLOR_DARKER:Rgb565 = Rgb565::new(1, 4, 3);
        
        const STROKE_COLOR:Rgb565 = Rgb565::new(1, 9, 8);
        const STROKE_WIDTH:u32 = 1;

        // TOP BAND
        const TOP_UI_STYLE:PrimitiveStyle<Rgb565> = PrimitiveStyleBuilder::new()
            .fill_color(BACKGROUND_COLOR_DARKER)
            .stroke_color(STROKE_COLOR)
            .stroke_width(STROKE_WIDTH)
            .build();
        const TOP_UI_BACKGROUND:Rectangle = Rectangle::new(
                Point { x: -1, y: -1 },
                Size { width: SCREEN_SIZE.0 + 2, height: 29 }
            );

        TOP_UI_BACKGROUND.into_styled(TOP_UI_STYLE).draw(display);

        // BOTTOM BAND
        const BOTTOM_UI_HEIGHT:u32 = 32;

        const BOTTOM_UI_BACKGROUND:Rectangle = Rectangle::new(
            Point { x: -1, y: (SCREEN_SIZE.1 - BOTTOM_UI_HEIGHT) as i32 + 1},
                    Size { width: SCREEN_SIZE.0 + 2, height: BOTTOM_UI_HEIGHT + 1}
            );

        BOTTOM_UI_BACKGROUND.into_styled(TOP_UI_STYLE).draw(display);


        // ICONS
        let icons_data = include_bytes!("../images/menu_icons.bmp");
        let icons: Bmp<'_, Rgb565> = Bmp::<Rgb565>::from_slice(icons_data).unwrap();
        const ICON_SIZE:u32 = 64;

        const ICON_STARTING_COORDS:(i32, i32) = (32, 44);
        const ICON_STEP_SIZE:(i32, i32) = (96, 84);

        for i in 0..2 {
            for j in 0..3 {
                let drawing_icon_id = i * 3 + j;
                let selected_y_shift = if drawing_icon_id == self.selected {ICON_SIZE} else {0};
                let top_left_of_texture = Point::new(drawing_icon_id as i32 * ICON_SIZE as i32 , selected_y_shift as i32);
                let icon = icons.sub_image(&Rectangle { top_left: top_left_of_texture, size: Size { width: ICON_SIZE, height: ICON_SIZE } });

                let icon_x = ICON_STARTING_COORDS.0 + j as i32 * ICON_STEP_SIZE.0;
                let icon_y = ICON_STARTING_COORDS.1 + i as i32 * ICON_STEP_SIZE.1;

                Image::new(&icon, Point::new(icon_x, icon_y))
                .draw(display);
            }
        }

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
