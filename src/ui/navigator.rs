//! Pile de navigation entre écrans.
//!
//! # Pattern pile (stack)
//!
//! La navigation fonctionne comme un navigateur web : pousser un nouvel écran,
//! revenir en arrière (pop). La profondeur maximale est fixée à la compilation
//! via const generic `DEPTH` — pas d'allocation heap.
//!
//! # `InputEvent`
//!
//! Abstraction des entrées physiques (encodeur rotatif, bouton) en événements
//! de navigation logique. Découple l'UI de la source d'entrée.

/// Écrans disponibles dans l'interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Status,
    MainMenu,
    TemperatureDetail,
    VoltageDetail,
    Settings,
}

/// Événement de navigation produit par l'encodeur ou d'autres entrées.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Up,
    Down,
    Select,
    Back,
}

/// Pile de navigation à profondeur fixe.
///
/// `DEPTH` est un const generic : `Navigator<8>` alloue exactement 8 slots.
pub struct Navigator<const DEPTH: usize> {
    stack: [Screen; DEPTH],
    top: usize,
}

impl<const DEPTH: usize> Navigator<DEPTH> {
    pub fn new(initial: Screen) -> Self {
        let mut stack = [Screen::Status; DEPTH];
        stack[0] = initial;
        Self { stack, top: 0 }
    }

    /// Retourne l'écran actuellement affiché.
    pub fn current(&self) -> Screen {
        self.stack[self.top]
    }

    /// Pousse un nouvel écran sur la pile. Ignore si la pile est pleine.
    pub fn push(&mut self, screen: Screen) {
        if self.top + 1 < DEPTH {
            self.top += 1;
            self.stack[self.top] = screen;
        }
    }

    /// Dépile l'écran courant. Ignore si déjà au niveau racine.
    pub fn pop(&mut self) {
        if self.top > 0 {
            self.top -= 1;
        }
    }

    /// Retourne `true` si on est à l'écran racine (pas de retour possible).
    pub fn is_at_root(&self) -> bool {
        self.top == 0
    }

    /// Traite un événement d'entrée. Retourne `true` si l'écran a changé.
    pub fn handle_back(&mut self) -> bool {
        if !self.is_at_root() {
            self.pop();
            true
        } else {
            false
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_screen_is_correct() {
        let nav: Navigator<8> = Navigator::new(Screen::Status);
        assert_eq!(nav.current(), Screen::Status);
    }

    #[test]
    fn push_changes_current_screen() {
        let mut nav: Navigator<8> = Navigator::new(Screen::Status);
        nav.push(Screen::MainMenu);
        assert_eq!(nav.current(), Screen::MainMenu);
    }

    #[test]
    fn pop_returns_to_previous() {
        let mut nav: Navigator<8> = Navigator::new(Screen::Status);
        nav.push(Screen::MainMenu);
        nav.pop();
        assert_eq!(nav.current(), Screen::Status);
    }

    #[test]
    fn pop_at_root_stays_at_root() {
        let mut nav: Navigator<8> = Navigator::new(Screen::Status);
        nav.pop();
        assert_eq!(nav.current(), Screen::Status);
    }

    #[test]
    fn is_at_root_initial() {
        let nav: Navigator<8> = Navigator::new(Screen::Status);
        assert!(nav.is_at_root());
    }

    #[test]
    fn is_at_root_after_push() {
        let mut nav: Navigator<8> = Navigator::new(Screen::Status);
        nav.push(Screen::MainMenu);
        assert!(!nav.is_at_root());
    }

    #[test]
    fn push_beyond_depth_is_ignored() {
        let mut nav: Navigator<2> = Navigator::new(Screen::Status);
        nav.push(Screen::MainMenu);
        nav.push(Screen::Settings); // profondeur 2 = max, ignoré
        assert_eq!(nav.current(), Screen::MainMenu);
    }
}
