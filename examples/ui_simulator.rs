//! Rendu de tous les états UI dans un framebuffer en mémoire (sans SDL2).
//!
//! Exécuter avec :
//! ```
//! cargo test --example ui_simulator --target x86_64-unknown-linux-gnu
//! ```
//!
//! Ce fichier ne lance pas de fenêtre graphique. Il vérifie uniquement
//! que chaque écran peut être rendu sans panique ni erreur.

use cloud_chamber::{
    shared::data::{SensorSnapshot, SystemState},
    ui::{
        navigator::{Navigator, Screen},
        screens::{menu::MainMenuScreen, status::StatusScreen},
    },
};
use embedded_graphics::geometry::Size;
use embedded_graphics_simulator::SimulatorDisplay;
use embedded_graphics::pixelcolor::Rgb565;

fn make_display() -> SimulatorDisplay<Rgb565> {
    SimulatorDisplay::new(Size::new(320, 240))
}

fn render_status(state: SystemState, temp: f32) {
    let mut display = make_display();
    let mut snap = SensorSnapshot::default();
    snap.temps[0] = temp;
    snap.volts[0] = 24.0;
    snap.amps[0] = 2.5;
    snap.is_closed = true;
    StatusScreen { snapshot: &snap, system_state: state }
        .draw(&mut display)
        .expect("status screen render failed");
}

fn render_menu(selected: usize) {
    let mut display = make_display();
    let mut menu = MainMenuScreen::new();
    for _ in 0..selected { menu.select_down(); }
    menu.draw(&mut display).expect("menu render failed");
}

fn main() {
    // Rend tous les états de l'écran de statut
    render_status(SystemState::Normal,    25.0);
    render_status(SystemState::Warning,   50.0);
    render_status(SystemState::Alarm,     65.0);
    render_status(SystemState::Emergency, 70.0);

    // Rend le menu avec chaque item sélectionné
    for i in 0..5 { render_menu(i); }

    // Teste la navigation
    let mut nav: Navigator<8> = Navigator::new(Screen::Status);
    nav.push(Screen::MainMenu);
    assert_eq!(nav.current(), Screen::MainMenu);
    nav.pop();
    assert_eq!(nav.current(), Screen::Status);

    println!("Tous les rendus UI ont réussi.");
}
