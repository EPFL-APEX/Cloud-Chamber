//! Écran de menu principal style Prusa.
//!
//! # Navigation
//!
//! - `right_turn()` / `left_turn()` (trait `Rotary`) : déplacent la sélection
//!   dans la liste.
//!
//! La liste est statique (`MAIN_MENU_SIZE`) — pas d'allocation heap.

use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    image::Image,
    mono_font::{MonoTextStyle, ascii::FONT_6X13},
    pixelcolor::Rgb565,
    primitives::{Line, Primitive, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle},
    text::{Baseline, LineHeight, Text, TextStyle, TextStyleBuilder},
};

use num_enum::{IntoPrimitive, TryFromPrimitive};
use tinybmp::Bmp;

use crate::ui::{
    interactions::{Click, NavAction, Rotary},
    navigator::Screen,
    theme, utils,
};

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

const MAIN_MENU_SIZE: u8 = 6; // core::mem::variant_count::<MainMenuItem>() as u8;

/// Écran de menu principal.
pub struct MainMenuScreen {
    pub selected: u8,
}

impl Rotary for MainMenuScreen {
    fn right_turn(&mut self) {
        if self.selected + 1 < MAIN_MENU_SIZE {
            self.selected += 1;
        }
    }

    fn left_turn(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }
}

impl Click for MainMenuScreen {
    fn click(&mut self) -> Option<NavAction> {
        let screen = match MainMenuItem::try_from(self.selected).ok()? {
            MainMenuItem::CONTROL => Screen::ManualControl,
            MainMenuItem::STATS => Screen::Stats,
            MainMenuItem::SETTINGS => Screen::Settings,
            MainMenuItem::COOLDOWN => Screen::CurrentTask,
            MainMenuItem::DATA => Screen::Data,
            MainMenuItem::INFO => Screen::Info,
        };
        Some(NavAction::Push(screen))
    }
}

impl MainMenuScreen {
    pub fn new() -> Self {
        Self { selected: 0 }
    }

    pub fn draw<D>(&self, display: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565> + OriginDimensions,
    {
        // BACKGROUND
        display.clear(theme::BACKGROUND_COLOR)?;

        // STRUCTURE UI
        const SCREEN_SIZE: (u32, u32) = (320, 240);

        const STROKE_WIDTH: u32 = 1;

        // TOP BAND
        const TOP_UI_STYLE: PrimitiveStyle<Rgb565> = PrimitiveStyleBuilder::new()
            .fill_color(theme::BACKGROUND_COLOR_DARKER)
            .stroke_color(theme::ACCENT_COLOR)
            .stroke_width(STROKE_WIDTH)
            .build();
        const TOP_UI_BACKGROUND: Rectangle = Rectangle::new(
            Point { x: -1, y: -1 },
            Size {
                width: SCREEN_SIZE.0 + 2,
                height: 29,
            },
        );

        TOP_UI_BACKGROUND.into_styled(TOP_UI_STYLE).draw(display)?;

        const TITLE_STYLE: TextStyle = TextStyleBuilder::new()
            .line_height(LineHeight::Pixels(14))
            .baseline(Baseline::Top)
            .build();
        const CHAR_STYLE: MonoTextStyle<Rgb565> =
            MonoTextStyle::new(&FONT_6X13, theme::HIGHLIGHT_COLOR);
        const TOP_LEFT_TITLE: Text<'static, MonoTextStyle<Rgb565>> =
            Text::with_text_style("Cloud Chamber", Point::new(4, 6), CHAR_STYLE, TITLE_STYLE);

        TOP_LEFT_TITLE.draw(display)?;

        // BOTTOM BAND
        const BOTTOM_UI_HEIGHT: u32 = 32;

        const BOTTOM_UI_BACKGROUND: Rectangle = Rectangle::new(
            Point {
                x: -1,
                y: (SCREEN_SIZE.1 - BOTTOM_UI_HEIGHT) as i32 + 1,
            },
            Size {
                width: SCREEN_SIZE.0 + 2,
                height: BOTTOM_UI_HEIGHT + 1,
            },
        );

        BOTTOM_UI_BACKGROUND
            .into_styled(TOP_UI_STYLE)
            .draw(display)?;

        const SEPARATION_STEP_SIZE: i32 = 80;
        const SEPARATION_STARTING_COORDS: (i32, i32) = (80, 208);
        const SEPARATION_HEIGHT: i32 = 32;

        const SEPARATION_STYLE: PrimitiveStyle<Rgb565> = PrimitiveStyleBuilder::new()
            .stroke_width(1)
            .stroke_color(theme::ACCENT_COLOR)
            .build();

        for i in 0..3 {
            let x_coord: i32 = SEPARATION_STARTING_COORDS.0 + i * SEPARATION_STEP_SIZE;
            let y_coord: i32 = SEPARATION_STARTING_COORDS.1;
            Line::new(
                Point {
                    x: x_coord,
                    y: y_coord,
                },
                Point {
                    x: x_coord,
                    y: y_coord + SEPARATION_HEIGHT,
                },
            )
            .into_styled(SEPARATION_STYLE)
            .draw(display)?;
        }

        let stats_icons_data = include_bytes!("../images/stats_icons.bmp");
        let stats_icons = utils::Icons::new(
            Bmp::<Rgb565>::from_slice(stats_icons_data).unwrap(),
            Size::new(18, 18),
        )
        .unwrap();

        const STATS_ICON_STARTING_COORDS: (i32, i32) = (6, 216);

        for i in 0..4 {
            let icon = stats_icons.get(i).unwrap();

            let icon_x = STATS_ICON_STARTING_COORDS.0 + i as i32 * SEPARATION_STEP_SIZE;
            let icon_y = STATS_ICON_STARTING_COORDS.1;

            Image::new(&icon, Point::new(icon_x, icon_y)).draw(display)?;
        }

        // ICONS
        let menu_icons_data = include_bytes!("../images/menu_icons.bmp");
        let menu_icons = utils::Icons::new(
            Bmp::<Rgb565>::from_slice(menu_icons_data).unwrap(),
            Size::new(64, 64),
        )
        .unwrap();

        const ICON_STARTING_COORDS: (i32, i32) = (32, 44);
        const ICON_STEP_SIZE: (i32, i32) = (96, 84);

        for i in 0..2 {
            for j in 0..3 {
                let id: usize = i * 3 + j;
                let selected_menu_shift: usize = if id == self.selected as usize { 6 } else { 0 };
                let icon = menu_icons.get(id + selected_menu_shift).unwrap();

                let icon_x = ICON_STARTING_COORDS.0 + j as i32 * ICON_STEP_SIZE.0;
                let icon_y = ICON_STARTING_COORDS.1 + i as i32 * ICON_STEP_SIZE.1;

                Image::new(&icon, Point::new(icon_x, icon_y)).draw(display)?;
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
        geometry::Size,
        mono_font::{MonoTextStyle, ascii::FONT_6X9},
        pixelcolor::Rgb565,
        primitives::{Circle, Line, PrimitiveStyle, Rectangle},
        text::Text,
    };

    use embedded_graphics_simulator::{OutputSettingsBuilder, SimulatorDisplay};

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
        menu.right_turn();
        assert_eq!(menu.selected, 1);
    }

    #[test]
    fn select_next_at_top_stays() {
        let mut menu = MainMenuScreen::new();
        menu.left_turn();
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn select_previous_at_bottom_stays() {
        let mut menu = MainMenuScreen::new();
        for _ in 0..20 {
            menu.right_turn();
        }
        assert_eq!(menu.selected, MAIN_MENU_SIZE - 1);
    }

    #[test]
    fn menu_draws_without_error() {
        let mut d = make_display();
        MainMenuScreen::new().draw(&mut d).unwrap();
    }

    #[test]
    fn click_on_first_item_pushes_control() {
        let mut menu = MainMenuScreen::new();
        assert_eq!(menu.click(), Some(NavAction::Push(Screen::ManualControl)));
    }

    #[test]
    fn click_on_stats_pushes_stats() {
        let mut menu = MainMenuScreen::new();
        menu.right_turn(); // CONTROL -> STATS
        assert_eq!(menu.click(), Some(NavAction::Push(Screen::Stats)));
    }

    #[test]
    fn click_on_last_item_pushes_info() {
        let mut menu = MainMenuScreen::new();
        for _ in 0..MAIN_MENU_SIZE {
            menu.right_turn();
        }
        assert_eq!(menu.click(), Some(NavAction::Push(Screen::Info)));
    }

    //#[test]
    //fn selected_item_returns_correct_label() {
    //    let mut menu = MainMenuScreen::new();
    //    menu.select_next();
    //    todo!()
    //}

    /// Fenêtre SDL2 interactive : flèches gauche/droite pour naviguer,
    /// Entrée/Espace pour cliquer, fermer la fenêtre pour quitter.
    ///
    /// `cargo test --features live-menu-test main_menu_live` (nécessite
    /// SDL2 installé et un affichage). `click()` ne pousse pas réellement
    /// sur une pile de navigation ici (pas de `Navigator` dans ce test) —
    /// la décision renvoyée est juste ignorée.
    #[cfg(feature = "live-menu-test")]
    #[test]
    fn main_menu_live() {
        use embedded_graphics_simulator::{SimulatorEvent, Window, sdl2::Keycode};

        let mut display = make_display();
        let mut menu = MainMenuScreen::new();
        menu.draw(&mut display).unwrap();

        let output_settings = OutputSettingsBuilder::new().scale(2).build();
        let mut window = Window::new("Cloud Chamber - menu (live)", &output_settings);

        'running: loop {
            window.update(&display);
            for event in window.events() {
                match event {
                    SimulatorEvent::Quit => break 'running,
                    SimulatorEvent::KeyDown { keycode, .. } => {
                        match keycode {
                            Keycode::Right | Keycode::Down => menu.right_turn(),
                            Keycode::Left | Keycode::Up => menu.left_turn(),
                            Keycode::Return | Keycode::Space => {
                                menu.click();
                            }
                            _ => continue,
                        }
                        menu.draw(&mut display).unwrap();
                    }
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn main_menu_screenshot() -> Result<(), core::convert::Infallible> {
        let mut display = SimulatorDisplay::<Rgb565>::new(Size::new(320, 240));

        let main_menu_screen = MainMenuScreen::new();

        main_menu_screen.draw(&mut display)?;

        // SAVE SCREENSHOT
        let output_settings = OutputSettingsBuilder::new().build();

        let path = std::env::args_os()
            .nth(1)
            .unwrap_or_else(|| "screenshots/MainMenu.png".into());
        display
            .to_rgb_output_image(&output_settings)
            .save_png(&path)
            .expect("failed to save screenshot");

        Ok(())
    }
}
