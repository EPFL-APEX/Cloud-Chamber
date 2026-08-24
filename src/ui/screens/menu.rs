//! Écran de menu principal style Prusa.
//!
//! # Navigation
//!
//! - `right_turn()` / `left_turn()` (trait `Rotary`) : déplacent la sélection
//!   dans la liste.
//! - `click()` (trait `Click`) : ouvre l'écran correspondant. Le premier
//!   item (`START`) fait en plus démarrer la machine — cf.
//!   [`MainMenuScreen::take_task_request`].
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

use crate::logic::cooling::CoolingPhase;
use crate::shared::data::SystemTask;
use crate::ui::{
    interactions::{Click, NavAction, Rotary},
    navigator::Screen,
    theme, utils,
};

/// Entrées du menu principal.
#[repr(u8)]
#[derive(TryFromPrimitive, IntoPrimitive)]
pub enum MainMenuItem {
    /// Démarre un cycle de refroidissement et ouvre l'écran de suivi.
    START,
    STATS,
    SETTINGS,
    COOLDOWN,
    DATA,
    INFO,
}

const MAIN_MENU_SIZE: u8 = 6; // core::mem::variant_count::<MainMenuItem>() as u8;

/// Première phase du cycle : celle par laquelle `logic::cooling` commence.
/// Écrire cette valeur dans `SHARED_STATE.task`, c'est tout ce qu'il faut
/// pour lancer la machine — `control_loop::tick()` adopte l'écriture au
/// tour suivant et enchaîne les phases lui-même.
const FIRST_COOLING_PHASE: SystemTask = SystemTask::Cooling(CoolingPhase::SensorCheck);

/// Écran de menu principal.
pub struct MainMenuScreen {
    pub selected: u8,
    /// Changement d'état demandé par l'opérateur, en attente d'être
    /// récupéré. Cf. [`MainMenuScreen::take_task_request`].
    task_requested: Option<SystemTask>,
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
    /// Le premier item démarre en plus un cycle : il lève une demande d'état
    /// (récupérée par la boucle principale via
    /// [`MainMenuScreen::take_task_request`]) *et* ouvre l'écran de suivi.
    /// L'écran décide, il n'agit pas — même séparation que
    /// `ActuatorPlan`/`Actuators::apply` : c'est ce qui permet de tester le
    /// démarrage sans toucher au `static` `SHARED_STATE`.
    fn click(&mut self) -> Option<NavAction> {
        let screen = match MainMenuItem::try_from(self.selected).ok()? {
            MainMenuItem::START => {
                self.task_requested = Some(FIRST_COOLING_PHASE);
                Screen::CurrentTask
            }
            MainMenuItem::STATS => Screen::Stats,
            MainMenuItem::SETTINGS => Screen::Settings,
            // Même écran que START, mais sans rien démarrer : consultation
            // du cycle en cours (ou de son absence).
            MainMenuItem::COOLDOWN => Screen::CurrentTask,
            MainMenuItem::DATA => Screen::Data,
            MainMenuItem::INFO => Screen::Info,
        };
        Some(NavAction::Push(screen))
    }
}

impl MainMenuScreen {
    pub fn new() -> Self {
        Self { selected: 0, task_requested: None }
    }

    /// Récupère un changement d'état demandé par l'opérateur, et le consomme.
    ///
    /// L'écran n'écrit pas dans `SHARED_STATE` lui-même : c'est un `static`
    /// partagé avec la boucle de contrôle, dont les écritures se
    /// réconcilient par comparaison-échange (cf.
    /// `logic::control_loop::tick`). Le faire depuis une ISR d'UI marcherait,
    /// mais mettrait la politique de démarrage dans un écran — ici l'écran
    /// se contente de lever le drapeau, l'appelant l'applique.
    pub fn take_task_request(&mut self) -> Option<SystemTask> {
        self.task_requested.take()
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

    use embedded_graphics::{geometry::Size, pixelcolor::Rgb565};

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
    fn click_on_first_item_opens_the_running_screen() {
        let mut menu = MainMenuScreen::new();
        assert_eq!(menu.click(), Some(NavAction::Push(Screen::CurrentTask)));
    }

    #[test]
    fn click_on_first_item_requests_the_start_of_a_cooling_cycle() {
        let mut menu = MainMenuScreen::new();
        assert_eq!(menu.take_task_request(), None, "rien de demande avant le clic");

        menu.click();
        assert_eq!(
            menu.take_task_request(),
            Some(SystemTask::Cooling(CoolingPhase::SensorCheck)),
        );
    }

    #[test]
    fn a_start_request_is_only_delivered_once() {
        let mut menu = MainMenuScreen::new();
        menu.click();
        assert!(menu.take_task_request().is_some());
        // Sinon la boucle principale relancerait un cycle à chaque tour.
        assert_eq!(menu.take_task_request(), None);
    }

    /// Ouvrir l'écran de suivi depuis l'item COOLDOWN ne doit rien démarrer :
    /// c'est une consultation, pas une commande.
    #[test]
    fn click_on_cooldown_opens_the_same_screen_without_starting_anything() {
        let mut menu = MainMenuScreen::new();
        for _ in 0..MainMenuItem::COOLDOWN as u8 {
            menu.right_turn();
        }
        assert_eq!(menu.click(), Some(NavAction::Push(Screen::CurrentTask)));
        assert_eq!(menu.take_task_request(), None);
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

    // Le harnais interactif SDL2 vit maintenant dans `ui::router` : il
    // pilote le routeur complet (menu, réglages, stats, suivi de cycle) au
    // lieu de ce seul écran, et applique réellement la navigation — ici,
    // la décision renvoyée par `click()` était ignorée.

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
