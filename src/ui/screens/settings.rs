//! Écran de réglages, navigation à deux modes façon Prusa : tourner déplace
//! la sélection, cliquer entre en édition, tourner change la valeur, cliquer
//! valide.
//!
//! Les valeurs vivent ici, initialisées depuis `crate::config` au démarrage.
//! Rien d'autre ne les lit encore : `logic/` continue de consulter les
//! constantes. Le jour où une struct `Settings` partagée existera, ce tableau
//! sera remplacé par un emprunt et le reste de l'écran ne bougera pas.

use core::fmt::Write;

use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{ascii::FONT_6X13, MonoTextStyle},
    pixelcolor::Rgb565,
    primitives::{Primitive, PrimitiveStyle, Rectangle},
    text::{Baseline, Text, TextStyleBuilder},
};
use heapless::String;

use crate::config::{IPA_HEATER_TARGET_C, PRECOOL_TARGET_C, SATURATION_TARGET_C, TARGET_CHAMBER_TEMP};
use crate::ui::interactions::{Click, NavAction, Rotary};
use crate::ui::theme;

use super::widgets::{SettingLine, SettingLines};

/// Bornes et pas d'un réglage. Le pas est aussi ce que vaut un cran
/// d'encodeur en mode édition.
struct SettingSpec {
    label: &'static str,
    min: f32,
    max: f32,
    step: f32,
}

const SETTINGS: [SettingSpec; 4] = [
    SettingSpec { label: "Chamber target", min: -50.0, max: 0.0, step: 0.5 },
    SettingSpec { label: "Pre-cool target", min: -50.0, max: 0.0, step: 0.5 },
    SettingSpec { label: "Saturation target", min: -50.0, max: 0.0, step: 0.5 },
    SettingSpec { label: "IPA heater target", min: 0.0, max: 60.0, step: 0.5 },
];

const SCREEN_WIDTH: u32 = 320;
const TOP_BAND_HEIGHT: u32 = 29;
const LIST_TOP: i32 = 34;

/// Dernière ligne de la liste : sortie de l'écran.
const BACK_ROW: usize = SETTINGS.len();
const N_ROWS: usize = SETTINGS.len() + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Browsing,
    Editing,
}

pub struct SettingsScreen {
    selected: usize,
    mode: Mode,
    values: [f32; SETTINGS.len()],
}

impl SettingsScreen {
    pub fn new() -> Self {
        Self {
            selected: 0,
            mode: Mode::Browsing,
            values: [
                TARGET_CHAMBER_TEMP,
                PRECOOL_TARGET_C,
                SATURATION_TARGET_C,
                IPA_HEATER_TARGET_C,
            ],
        }
    }

    /// Déplace la valeur du réglage sélectionné d'un cran, bornée par sa spec.
    fn nudge(&mut self, steps: f32) {
        if self.selected >= SETTINGS.len() {
            return;
        }
        let spec = &SETTINGS[self.selected];
        let value = self.values[self.selected] + steps * spec.step;
        self.values[self.selected] = value.clamp(spec.min, spec.max);
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
            "Settings",
            Point::new(10, 8),
            MonoTextStyle::new(&FONT_6X13, theme::HIGHLIGHT_COLOR),
            title_style,
        )
        .draw(display)?;

        // Les valeurs sont formatées ici et empruntées par le widget : les
        // tampons doivent donc vivre jusqu'à la fin de la fonction.
        let mut buffers: [String<12>; SETTINGS.len()] = Default::default();
        for (buffer, value) in buffers.iter_mut().zip(self.values.iter()) {
            let _ = write!(buffer, "{value:.1} C");
        }

        let mut lines = [SettingLine { label: "", value: "" }; N_ROWS];
        for (i, spec) in SETTINGS.iter().enumerate() {
            lines[i] = SettingLine { label: spec.label, value: buffers[i].as_str() };
        }
        lines[BACK_ROW] = SettingLine { label: "Back", value: "" };

        SettingLines::<N_ROWS> {
            lines,
            selected: self.selected,
            editing: self.mode == Mode::Editing,
            top: LIST_TOP,
        }
        .draw(display)
    }
}

impl Rotary for SettingsScreen {
    fn right_turn(&mut self) {
        match self.mode {
            Mode::Browsing if self.selected + 1 < N_ROWS => self.selected += 1,
            Mode::Editing => self.nudge(1.0),
            Mode::Browsing => {}
        }
    }

    fn left_turn(&mut self) {
        match self.mode {
            Mode::Browsing if self.selected > 0 => self.selected -= 1,
            Mode::Editing => self.nudge(-1.0),
            Mode::Browsing => {}
        }
    }
}

impl Click for SettingsScreen {
    /// Sur la ligne Back, quitte l'écran. Ailleurs, bascule entre navigation
    /// et édition sans naviguer.
    fn click(&mut self) -> Option<NavAction> {
        if self.selected == BACK_ROW {
            return Some(NavAction::Back);
        }
        self.mode = match self.mode {
            Mode::Browsing => Mode::Editing,
            Mode::Editing => Mode::Browsing,
        };
        None
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_graphics_simulator::{SimulatorDisplay, OutputSettingsBuilder};

    fn make_display() -> SimulatorDisplay<Rgb565> {
        SimulatorDisplay::new(Size::new(SCREEN_WIDTH, 240))
    }

    #[test]
    fn browsing_moves_selection_without_changing_values() {
        let mut screen = SettingsScreen::new();
        let before = screen.values;
        screen.right_turn();
        assert_eq!(screen.selected, 1);
        assert_eq!(screen.values, before);
    }

    #[test]
    fn selection_stops_at_both_ends() {
        let mut screen = SettingsScreen::new();
        screen.left_turn();
        assert_eq!(screen.selected, 0);
        for _ in 0..20 {
            screen.right_turn();
        }
        assert_eq!(screen.selected, BACK_ROW);
    }

    #[test]
    fn click_enters_and_leaves_edit_mode() {
        let mut screen = SettingsScreen::new();
        assert!(screen.click().is_none());
        assert_eq!(screen.mode, Mode::Editing);
        assert!(screen.click().is_none());
        assert_eq!(screen.mode, Mode::Browsing);
    }

    #[test]
    fn editing_changes_the_selected_value_only() {
        let mut screen = SettingsScreen::new();
        let others = screen.values[1..].to_vec();
        let _ = screen.click();
        screen.right_turn();
        assert_eq!(screen.values[0], TARGET_CHAMBER_TEMP + SETTINGS[0].step);
        assert_eq!(screen.values[1..].to_vec(), others);
    }

    #[test]
    fn value_stays_within_its_bounds() {
        let mut screen = SettingsScreen::new();
        let _ = screen.click();
        for _ in 0..500 {
            screen.right_turn();
        }
        assert_eq!(screen.values[0], SETTINGS[0].max);
        for _ in 0..500 {
            screen.left_turn();
        }
        assert_eq!(screen.values[0], SETTINGS[0].min);
    }

    #[test]
    fn back_row_leaves_the_screen() {
        let mut screen = SettingsScreen::new();
        screen.selected = BACK_ROW;
        assert_eq!(screen.click(), Some(NavAction::Back));
    }

    #[test]
    fn draws_in_both_modes() {
        let mut d = make_display();
        let mut screen = SettingsScreen::new();
        screen.draw(&mut d).unwrap();
        let _ = screen.click();
        screen.draw(&mut d).unwrap();
    }


    #[test]
    fn main_menu_screenshot() -> Result<(), core::convert::Infallible> {
        let mut display = make_display();

        let main_menu_screen = SettingsScreen::new();

        main_menu_screen.draw(&mut display)?;

        // SAVE SCREENSHOT
        let output_settings = OutputSettingsBuilder::new()
            .build();

        let path = std::env::args_os()
            .nth(1)
            .unwrap_or_else(|| "screenshots/SettingsMenu.png".into());
        display
            .to_rgb_output_image(&output_settings)
            .save_png(&path)
            .expect("failed to save screenshot");

        Ok(())
    }

}
