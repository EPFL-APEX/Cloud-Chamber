//! Buffer circulaire (LIFO) à taille fixe, sans allocation heap.
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
//! Cela permet d'accéder à l'historique des mesures par ancienneté relative.
//!
//! # Contrainte `T: Copy + Default`
//!
//! - `Copy` : les valeurs sont copiées dans et hors du buffer sans move.
//! - `Default` : le buffer est initialisé avec des valeurs "zéro" avant
//!   la première écriture (ex: `0.0` pour `f32`, `false` pour `bool`).

use crate::shared::error::{Error, Result};

/// Buffer circulaire à taille fixe.
///
/// Les nouvelles valeurs écrasent les plus anciennes quand le buffer est plein.
/// `get(0)` retourne toujours la valeur la plus récente.
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
    ///
    /// Retourne `Err` si :
    /// - `index >= N` (hors capacité du buffer)
    /// - `index >= write_index` et le buffer n'est pas encore plein
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

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_buffer_is_empty() {
        let buf: RingBuffer<u32, 4> = RingBuffer::new();
        assert!(buf.is_empty());
        assert!(buf.get(0).is_err());
    }

    #[test]
    fn push_one_and_get_returns_it() {
        let mut buf: RingBuffer<u32, 4> = RingBuffer::new();
        buf.push(42);
        assert_eq!(buf.get(0).unwrap(), 42);
    }

    #[test]
    fn get_index_zero_is_most_recent() {
        let mut buf: RingBuffer<u32, 4> = RingBuffer::new();
        buf.push(1); buf.push(2); buf.push(3);
        assert_eq!(buf.get(0).unwrap(), 3);
        assert_eq!(buf.get(1).unwrap(), 2);
        assert_eq!(buf.get(2).unwrap(), 1);
    }

    #[test]
    fn get_out_of_range_returns_err() {
        let mut buf: RingBuffer<u32, 4> = RingBuffer::new();
        buf.push(1); buf.push(2);
        assert!(buf.get(2).is_err());
    }

    #[test]
    fn push_wraps_around_correctly() {
        let mut buf: RingBuffer<u32, 3> = RingBuffer::new();
        buf.push(1); buf.push(2); buf.push(3); buf.push(4);
        assert_eq!(buf.get(0).unwrap(), 4);
        assert_eq!(buf.get(1).unwrap(), 3);
        assert_eq!(buf.get(2).unwrap(), 2);
    }

    #[test]
    fn full_buffer_overwrite_old_values() {
        let mut buf: RingBuffer<i32, 5> = RingBuffer::new();
        for i in 0..10 { buf.push(i); }
        assert_eq!(buf.get(0).unwrap(), 9);
        assert_eq!(buf.get(4).unwrap(), 5);
    }

    #[test]
    fn bool_ring_buffer_works() {
        let mut buf: RingBuffer<bool, 3> = RingBuffer::new();
        buf.push(true); buf.push(false);
        assert!(!buf.get(0).unwrap());
        assert!(buf.get(1).unwrap());
    }

    #[test]
    fn index_beyond_capacity_returns_err() {
        let mut buf: RingBuffer<f32, 4> = RingBuffer::new();
        for _ in 0..4 { buf.push(1.0); }
        assert!(buf.get(4).is_err());
    }
}
