//! Traits pour le timer monotonique et le watchdog.
//!
//! # Timer monotonique
//!
//! Un timer monotonique ne revient jamais en arrière et n'est pas affecté
//! par les ajustements d'horloge système. Il est idéal pour mesurer des
//! durées et implémenter des boucles à période fixe.
//!
//! # Watchdog
//!
//! Le watchdog est un timer matériel qui redémarre le microcontrôleur si
//! la fonction `feed()` n'est pas appelée périodiquement. Cela garantit
//! qu'un blocage logiciel ne laisse pas le système dans un état indéfini.

/// Timer monotonique exprimé en microsecondes.
pub trait MonotonicTimer {
    /// Retourne le temps écoulé depuis le démarrage, en µs.
    ///
    /// La valeur est monotone : elle ne décroît jamais.
    fn get_counter_us(&self) -> Instant;
}

/// Alimentation (« nourrissage ») du watchdog matériel.
pub trait WatchdogFeed {
    /// Réinitialise le compteur du watchdog.
    ///
    /// Doit être appelé régulièrement (au moins une fois par période de
    /// sécurité) pour éviter un redémarrage forcé.
    fn feed(&mut self);
}

#[derive(Debug, Clone, Copy)]
pub struct Instant {
    time:u64
}

impl Instant {
    pub fn new(time: u64) -> Self {
        Self { time }
    }
    pub fn is_newer_than(&self, other: &Instant) -> bool {
        self.time > other.time
    }
}

// ─── Implémentations concrètes (matériel ARM uniquement) ─────────────────────

#[cfg(all(rp2040, target_arch = "arm"))]
impl MonotonicTimer for rp2040_hal::Timer {
    fn get_counter_us(&self) -> Instant {
        self.get_counter().ticks()
    }
}

#[cfg(all(rp2350, any(target_arch = "arm", target_arch = "riscv32")))]
impl MonotonicTimer for rp235x_hal::Timer<rp235x_hal::timer::CopyableTimer0> {
    fn get_counter_us(&self) -> Instant {
        self.get_counter().ticks()
    }
}
