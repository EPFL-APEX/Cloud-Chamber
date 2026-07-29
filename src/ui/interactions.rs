//! Traits d'interaction physique (encodeur rotatif, bouton) implémentés
//! par les écrans qui en ont besoin.

use super::navigator::Screen;

pub trait Rotary {
    fn right_turn(&mut self);
    fn left_turn(&mut self);
}

/// Décision de navigation renvoyée par [`Click::click`].
///
/// L'écran décide (quel écran ouvrir, ou "retour"), il ne touche jamais
/// `Navigator` lui-même — même séparation décision/application que
/// `ActuatorPlan`/`Actuators::apply` dans `logic/` : ça permet de tester un
/// écran sans construire de pile de navigation, et ça évite qu'un écran
/// (censé être une feuille) dépende du type qui le possède.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavAction {
    Push(Screen),
    Back,
}

pub trait Click {
    fn click(&mut self) -> Option<NavAction>;
}
