//! Écran de réglages, navigation à deux modes façon Prusa : tourner déplace
//! la sélection, cliquer entre en édition, tourner change la valeur, cliquer
//! valide.
//!
//! Deux destinations pour une modification, et il ne faut pas les
//! confondre. Chaque cran d'encodeur pousse la nouvelle valeur dans
//! [`crate::shared::settings`], d'où `logic/` la lit au tour suivant : c'est
//! ça, « les changements s'appliquent tout de suite ». La flash, elle, n'est
//! écrite que sur la ligne « Save », et pas par cet écran — il lève un
//! drapeau que la boucle principale récupère via
//! [`SettingsScreen::take_save_request`], parce que c'est elle qui possède
//! le `SettingsStore`. Même séparation que `ActuatorPlan`/`Actuators::apply`
//! : l'écran décide, il n'agit pas.

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

use crate::cloud_chamber_hal::units::Celsius;
use crate::config::settings::Settings;
use crate::shared::settings as shared_settings;
use crate::ui::interactions::{Click, NavAction, Rotary};
use crate::ui::theme;

use super::widgets::{SettingLine, SettingLines};

/// Bornes et pas d'un réglage. Le pas est aussi ce que vaut un cran
/// d'encodeur en mode édition.
struct SettingSpec {
    label: &'static str,
    get: fn(&Settings) -> Celsius,
    set: fn(&mut Settings, Celsius),
    min: f32,
    max: f32,
    step: f32,
}

const SETTINGS: [SettingSpec; 4] = [
    SettingSpec {
        label: "Chamber target",
        get: |s| s.chamber_target,
        set: |s, v| s.chamber_target = v,
        min: -50.0, max: 0.0, step: 0.5,
    },
    SettingSpec {
        label: "Pre-cool target",
        get: |s| s.precool_target,
        set: |s, v| s.precool_target = v,
        min: -50.0, max: 0.0, step: 0.5,
    },
    SettingSpec {
        label: "Saturation target",
        get: |s| s.saturation_target,
        set: |s, v| s.saturation_target = v,
        min: -50.0, max: 0.0, step: 0.5,
    },
    SettingSpec {
        label: "IPA heater target",
        get: |s| s.ipa_heater_target,
        set: |s, v| s.ipa_heater_target = v,
        min: 0.0, max: 60.0, step: 0.5,
    },
];

const SCREEN_WIDTH: u32 = 320;
const TOP_BAND_HEIGHT: u32 = 29;
const LIST_TOP: i32 = 34;

/// Les deux lignes d'action, à la suite des réglages.
const SAVE_ROW: usize = SETTINGS.len();
const BACK_ROW: usize = SETTINGS.len() + 1;
const N_ROWS: usize = SETTINGS.len() + 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Browsing,
    Editing,
}

pub struct SettingsScreen {
    selected: usize,
    mode: Mode,
    /// Copie de travail : c'est elle qu'on édite.
    working: Settings,
    /// Dernier état sauvegardé, pour savoir si quelque chose a bougé.
    saved: Settings,
    /// Sauvegarde demandée, en attente d'être récupérée par la boucle
    /// principale.
    save_requested: bool,
}

impl SettingsScreen {
    /// Repart de ce que `logic/` utilise déjà — pas de la valeur par
    /// défaut, pour ne pas donner l'impression de repartir de zéro si
    /// l'écran a été quitté puis rouvert en session.
    pub fn new() -> Self {
        let settings = shared_settings::get();
        Self {
            selected: 0,
            mode: Mode::Browsing,
            working: settings,
            saved: settings,
            save_requested: false,
        }
    }

    /// Récupère une demande de sauvegarde, et la consomme.
    ///
    /// L'écran ne peut pas écrire en flash lui-même : l'opération est
    /// lente, doit tourner depuis la RAM interruptions coupées, et le
    /// `SettingsStore` appartient à la boucle principale. L'écran se
    /// contente de lever le drapeau.
    pub fn take_save_request(&mut self) -> Option<Settings> {
        if !self.save_requested {
            return None;
        }
        self.save_requested = false;
        self.saved = self.working;
        Some(self.working)
    }

    /// Vrai si la copie de travail diffère du dernier état sauvegardé.
    fn is_dirty(&self) -> bool {
        self.working != self.saved
    }

    /// Déplace la valeur du réglage sélectionné d'un cran, bornée par sa spec.
    fn nudge(&mut self, steps: f32) {
        if self.selected >= SETTINGS.len() {
            return;
        }
        let spec = &SETTINGS[self.selected];
        let value = ((spec.get)(&self.working).0 + steps * spec.step).clamp(spec.min, spec.max);
        (spec.set)(&mut self.working, Celsius(value));

        // Publié tout de suite, pas à la sauvegarde : `logic/` reprendra la
        // nouvelle valeur au tour suivant, y compris en plein cycle.
        shared_settings::set(self.working);
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
        for (buffer, spec) in buffers.iter_mut().zip(SETTINGS.iter()) {
            let _ = write!(buffer, "{:.1} C", (spec.get)(&self.working).0);
        }

        let mut lines = [SettingLine { label: "", value: "" }; N_ROWS];
        for (i, spec) in SETTINGS.iter().enumerate() {
            lines[i] = SettingLine { label: spec.label, value: buffers[i].as_str() };
        }
        lines[SAVE_ROW] = SettingLine {
            label: "Save to flash",
            value: if self.is_dirty() { "*" } else { "" },
        };
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
    /// Sur la ligne Back, quitte l'écran. Sur la ligne Save, lève le
    /// drapeau de sauvegarde sans changer de mode. Ailleurs, bascule entre
    /// navigation et édition sans naviguer.
    fn click(&mut self) -> Option<NavAction> {
        if self.selected == BACK_ROW {
            return Some(NavAction::Back);
        }
        if self.selected == SAVE_ROW {
            self.save_requested = true;
            return None;
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
    use crate::shared::settings::with_isolated_settings;

    fn make_display() -> SimulatorDisplay<Rgb565> {
        SimulatorDisplay::new(Size::new(SCREEN_WIDTH, 240))
    }

    // `SettingsScreen::new()` lit `shared_settings::get()`, et `nudge()`
    // y écrit — un static partagé par tout le binaire de test. Chaque test
    // ci-dessous passe par `with_isolated_settings` (verrou + retour aux
    // défauts), sinon des tests parallèles (défaut de `cargo test`)
    // pourraient se voir mutuellement une valeur laissée par l'autre. Même
    // raison que `logic::control_loop::tests::with_isolated_shared_state`.

    #[test]
    fn browsing_moves_selection_without_changing_values() {
        with_isolated_settings(|| {
            let mut screen = SettingsScreen::new();
            let before = screen.working;
            screen.right_turn();
            assert_eq!(screen.selected, 1);
            assert_eq!(screen.working, before);
        });
    }

    #[test]
    fn selection_stops_at_both_ends() {
        with_isolated_settings(|| {
            let mut screen = SettingsScreen::new();
            screen.left_turn();
            assert_eq!(screen.selected, 0);
            for _ in 0..20 {
                screen.right_turn();
            }
            assert_eq!(screen.selected, BACK_ROW);
        });
    }

    #[test]
    fn click_enters_and_leaves_edit_mode() {
        with_isolated_settings(|| {
            let mut screen = SettingsScreen::new();
            assert!(screen.click().is_none());
            assert_eq!(screen.mode, Mode::Editing);
            assert!(screen.click().is_none());
            assert_eq!(screen.mode, Mode::Browsing);
        });
    }

    #[test]
    fn editing_changes_the_selected_value_only() {
        with_isolated_settings(|| {
            let mut screen = SettingsScreen::new();
            let before = screen.working;
            let _ = screen.click();
            screen.right_turn();
            assert_eq!(screen.working.chamber_target, Celsius(before.chamber_target.0 + SETTINGS[0].step));
            assert_eq!(screen.working.precool_target, before.precool_target);
            assert_eq!(screen.working.saturation_target, before.saturation_target);
            assert_eq!(screen.working.ipa_heater_target, before.ipa_heater_target);
        });
    }

    #[test]
    fn value_stays_within_its_bounds() {
        with_isolated_settings(|| {
            let mut screen = SettingsScreen::new();
            let _ = screen.click();
            for _ in 0..500 {
                screen.right_turn();
            }
            assert_eq!(screen.working.chamber_target, Celsius(SETTINGS[0].max));
            for _ in 0..500 {
                screen.left_turn();
            }
            assert_eq!(screen.working.chamber_target, Celsius(SETTINGS[0].min));
        });
    }

    #[test]
    fn nudging_publishes_to_shared_settings() {
        with_isolated_settings(|| {
            let mut screen = SettingsScreen::new();
            let _ = screen.click();
            screen.right_turn();
            assert_eq!(shared_settings::get().chamber_target, screen.working.chamber_target);
        });
    }

    #[test]
    fn back_row_leaves_the_screen() {
        with_isolated_settings(|| {
            let mut screen = SettingsScreen::new();
            screen.selected = BACK_ROW;
            assert_eq!(screen.click(), Some(NavAction::Back));
        });
    }

    #[test]
    fn save_row_flags_a_request_without_changing_mode() {
        with_isolated_settings(|| {
            let mut screen = SettingsScreen::new();
            screen.selected = SAVE_ROW;
            assert_eq!(screen.click(), None);
            assert_eq!(screen.mode, Mode::Browsing);
            assert_eq!(screen.take_save_request(), Some(screen.working));
        });
    }

    #[test]
    fn save_request_is_consumed_once() {
        with_isolated_settings(|| {
            let mut screen = SettingsScreen::new();
            screen.selected = SAVE_ROW;
            let _ = screen.click();
            assert!(screen.take_save_request().is_some());
            assert_eq!(screen.take_save_request(), None);
        });
    }

    #[test]
    fn dirty_only_after_an_edit() {
        with_isolated_settings(|| {
            let mut screen = SettingsScreen::new();
            assert!(!screen.is_dirty());
            let _ = screen.click();
            screen.right_turn();
            assert!(screen.is_dirty());
            screen.selected = SAVE_ROW;
            let _ = screen.click();
            let _ = screen.take_save_request();
            assert!(!screen.is_dirty());
        });
    }

    #[test]
    fn draws_in_both_modes() {
        with_isolated_settings(|| {
            let mut d = make_display();
            let mut screen = SettingsScreen::new();
            screen.draw(&mut d).unwrap();
            let _ = screen.click();
            screen.draw(&mut d).unwrap();
        });
    }

    #[test]
    fn main_menu_screenshot() -> Result<(), core::convert::Infallible> {
        with_isolated_settings(|| {
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
        })
    }

}
