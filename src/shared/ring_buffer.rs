//! Buffer circulaire (LIFO) à taille fixe, sans allocation heap.
//! Porté tel quel depuis la branche add-phase-transition-logic (src/shared/ring_buffer.rs).
//!
//! # Const Generics
//!
//! `RingBuffer<T, const N: usize>` utilise un const generic `N` pour que
//! la taille du buffer soit fixe et connue à la compilation. Le tableau
//! `[T; N]` est alloué en mémoire statique (stack ou .bss) — aucun heap.
//!
//! # Sémantique de lecture
//!
//! `get(0)` retourne la valeur la plus récente, `get(1)` la précédente, etc.

use crate::shared::error::{Error, Result};

/// Buffer circulaire à taille fixe.
///
/// Les nouvelles valeurs écrasent les plus anciennes quand le buffer est plein.
/// `get(0)` retourne toujours la valeur la plus récente.
#[derive(Debug)]
pub struct RingBuffer<T: Copy, const N: usize> {
    data: [T; N],
    write_index: usize,
    is_full: bool,
}

impl<T: Copy + Default, const N: usize> RingBuffer<T, N> {
    /// Crée un buffer vide initialisé avec les valeurs par défaut de `T`.
    pub fn new() -> Self {
        Self {
            data: [T::default(); N],
            write_index: 0,
            is_full: false,
        }
    }
}

impl<T: Copy, const N: usize> RingBuffer<T, N> {
    pub fn filled(value: T) -> Self {
        Self {
            data: [value; N],
            write_index: 0,
            is_full: false,
        }
    }

    /// Ajoute une valeur. Si le buffer est plein, écrase la plus ancienne.
    pub fn push(&mut self, value: T) {
        self.data[self.write_index] = value;
        self.write_index = (self.write_index + 1) % N;
        if self.write_index == 0 {
            self.is_full = true;
        }
    }

    /// Retourne la valeur à l'index `index` depuis la plus récente.
    ///
    /// `index = 0` → la plus récente, `index = 1` → avant-dernière, etc.
    pub fn get(&self, index: usize) -> Result<T> {
        if index >= N {
            return Err(Error::IndexOutOfBounds { index, len: N });
        }
        if !self.is_full && index >= self.write_index {
            return Err(Error::IndexOutOfBounds { index, len: self.write_index });
        }
        let actual_index = (self.write_index + N - 1 - index) % N;
        Ok(self.data[actual_index])
    }

    /// Retourne `true` si aucune valeur n'a encore été poussée.
    pub fn is_empty(&self) -> bool {
        !self.is_full && self.write_index == 0
    }
}
