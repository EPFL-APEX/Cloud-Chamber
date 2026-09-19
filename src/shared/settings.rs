//! Point de partage des réglages opérateur.
//!
//! Même rôle que `SHARED_STATE` dans `data.rs`, et rangé au même endroit
//! pour la même raison : `config::settings` définit le type `Settings`, il
//! ne possède aucun état. C'est `shared` qui détient ce qui est global.
//!
//! # Pourquoi un static et pas un paramètre
//!
//! Faire descendre un `&Settings` obligerait `time_limit()`,
//! `sensor_loss_abort()`, `advance()` et les trois `react_to()` de
//! `SystemTask`, `CoolingPhase` et `StoppingPhase` à le porter. Ces
//! valeurs ne sont pas des paramètres de ces fonctions, c'est justement
//! pour ça qu'elles étaient des constantes. Le static garde les signatures
//! intactes, et comme il démarre sur `Settings::defaults()`, les tests
//! existants de `logic/` continuent de passer sans être touchés.
//!
//! # Pourquoi `Cell` et pas `RefCell`
//!
//! `data.rs` utilise `Mutex<RefCell<SharedState>>` parce que `SharedState`
//! n'est pas `Copy` : il faut une `&mut` pour fusionner les mesures en
//! place. `Settings` est `Copy` et tient sur quelques octets — on le lit et
//! on l'écrit en entier, `Cell::get`/`set` suffisent.
//!
//! Ce n'est pas qu'une économie de compteur d'emprunt : `borrow_mut()`
//! panique si un emprunt est déjà actif, et une panique dans la boucle de
//! contrôle sur la puce, c'est un reset. Avec `Cell`, ce cas n'existe pas.

use core::cell::Cell;
use critical_section::Mutex;

use crate::config::settings::Settings;

/// Réglages courants. Écrits par l'écran de réglages, lus par `logic/`.
///
/// Passer par [`get`] et [`set`] plutôt que par ce static directement :
/// personne d'autre n'a besoin de savoir qu'il y a une section critique
/// dessous.
static SETTINGS: Mutex<Cell<Settings>> = Mutex::new(Cell::new(Settings::defaults()));

/// Copie des réglages courants.
pub fn get() -> Settings {
    critical_section::with(|cs| SETTINGS.borrow(cs).get())
}

/// Remplace les réglages courants.
///
/// L'écriture porte sur la struct entière : un lecteur voit soit l'ancienne
/// version, soit la nouvelle, jamais un mélange des deux.
pub fn set(settings: Settings) {
    critical_section::with(|cs| SETTINGS.borrow(cs).set(settings));
}

// Pas de tests dans CE module : la valeur initiale est une constante
// vérifiée à la compilation, et le câblage écran → static se constate sur
// la machine. Mais `logic::cooling`/`stopping`/`control_loop` lisent
// maintenant `get()` dans leur chemin normal, et `ui::screens::settings`
// écrit via `set()` — plusieurs modules de test partagent donc ce static.
// `cargo test` lance les tests en parallèle dans un même processus : sans
// verrou, un test qui pousse une valeur ailleurs ferait échouer par
// intermittence un test qui compare à `Settings::defaults()`. D'où
// `with_isolated_settings`, à utiliser par tout test qui touche ce static
// (lecture ou écriture) — même raison que
// `control_loop::tests::with_isolated_shared_state` sur `SHARED_STATE`,
// un static différent, donc un verrou différent.
#[cfg(test)]
static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Réinitialise `SETTINGS` à `Settings::defaults()` et exécute `body` sous
/// verrou exclusif — chaque test démarre donc d'un état connu, indépendant
/// de l'ordre ou du parallélisme d'exécution.
#[cfg(test)]
pub fn with_isolated_settings<T>(body: impl FnOnce() -> T) -> T {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set(Settings::defaults());
    body()
}
