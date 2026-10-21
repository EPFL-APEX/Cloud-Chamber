//! Aides partagées des modules de test de l'UI — l'écran factice de 320×240
//! et l'état partagé de base, recopiés sinon dans chaque `mod tests`.

#![cfg(test)]

use embedded_graphics::geometry::Size;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics_simulator::SimulatorDisplay;

use crate::shared::data::{SharedState, SystemTask};

/// L'écran factice, la même taille que l'ILI9341.
pub fn make_display() -> SimulatorDisplay<Rgb565> {
    SimulatorDisplay::new(Size::new(320, 240))
}

/// État partagé minimal : machine à `task`, capteurs jamais lus.
pub fn state_with(task: SystemTask) -> SharedState {
    SharedState { snapshot: Default::default(), task, new_data: false, core1_panicked: false }
}
