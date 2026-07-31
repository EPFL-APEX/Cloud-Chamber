//! Mesure horodatée — forme commune à toutes les lectures de capteur.

use super::timer::Instant;

/// Mesure horodatée dans l'unité physique `Unit`.
#[derive(Clone, Copy, Debug)]
pub struct Measurement<Unit> {
    pub time: Instant,
    pub value: Unit,
}

impl<Unit> Measurement<Unit> {
    pub fn new(time: Instant, value: Unit) -> Self {
        Self { time, value }
    }

    /// `true` si cette mesure est plus récente que `other`.
    pub fn is_newer_than(&self, other: &Self) -> bool {
        self.time > other.time
    }

    /// `true` si cette mesure est plus ancienne que `other`.
    pub fn is_older_than(&self, other: &Self) -> bool {
        self.time < other.time
    }
}
