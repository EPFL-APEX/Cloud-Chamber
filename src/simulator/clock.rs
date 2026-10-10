//! Horloge virtuelle : le temps du simulateur, accéléré à volonté.

use crate::cloud_chamber_hal::timer::{Instant, MonotonicTimer};

/// Horloge monotone pilotée par le simulateur.
///
/// La boucle de contrôle et les capteurs ne voient jamais la différence
/// avec `hal::Timer` : même trait, même sémantique. Seul le binaire du
/// simulateur avance le compteur — par grands pas (`advance_seconds`)
/// pour laisser la physique se dérouler, ou pas fins pour les tours de
/// boucle.
///
/// Le facteur d'accélération n'est pas dans l'horloge : c'est le
/// simulateur qui choisit combien de temps virtuel s'écoule par
/// milliseconde réelle. L'horloge, elle, ne connaît que le temps virtuel.
#[derive(Debug, Clone)]
pub struct VirtualClock {
    /// Temps virtuel écoulé, en microsecondes.
    now_us: core::cell::Cell<u64>,
}

impl VirtualClock {
    pub fn new() -> Self {
        Self { now_us: core::cell::Cell::new(0) }
    }

    /// Avance le temps virtuel de `us` microsecondes.
    pub fn advance_us(&self, us: u64) {
        self.now_us.set(self.now_us.get().saturating_add(us));
    }

    /// Avance le temps virtuel de `secs` secondes — le pas de physique.
    pub fn advance_seconds(&self, secs: f32) {
        self.advance_us((secs * 1_000_000.0) as u64);
    }
}

impl MonotonicTimer for VirtualClock {
    fn now(&self) -> Instant {
        Instant::from_micros(self.now_us.get())
    }
}

/// Par référence : le binaire du simulateur garde l'horloge et la prête
/// à `PhaseClock`/`tick`/capteurs par `&` — une seule instance, une
/// seule source de temps virtuel, personne ne la clone par accident.
impl MonotonicTimer for &VirtualClock {
    fn now(&self) -> Instant {
        (*self).now()
    }
}

impl Default for VirtualClock {
    fn default() -> Self {
        Self::new()
    }
}
