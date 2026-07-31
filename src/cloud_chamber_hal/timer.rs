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

/// Timer monotonique. Une seule méthode requise (`now`) — `elapsed_since`
/// est fournie par défaut, un nouvel impl matériel n'a rien de plus à
/// écrire.
pub trait MonotonicTimer {
    /// Horodatage courant. Monotone : ne revient jamais en arrière.
    fn now(&self) -> Instant;

    /// Temps écoulé depuis `earlier`.
    fn elapsed_since(&self, earlier: Instant) -> Duration {
        self.now().since(earlier)
    }
}

/// Alimentation (« nourrissage ») du watchdog matériel.
pub trait WatchdogFeed {
    /// Réinitialise le compteur du watchdog.
    ///
    /// Doit être appelé régulièrement (au moins une fois par période de
    /// sécurité) pour éviter un redémarrage forcé.
    fn feed(&mut self);
}

/// Instant monotone, en microsecondes depuis le démarrage.
///
/// `u64` plutôt qu'un `u32` : un `u32` de µs déborde après ~71 minutes,
/// trop court — `Stabilising` (régime permanent après un cycle de
/// refroidissement) peut durer des heures voire des jours en usage réel.
/// `u64` déborde après ~584 942 ans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instant(u64);

impl Instant {
    pub const fn from_micros(us: u64) -> Self {
        Self(us)
    }

    pub const fn as_millis(&self) -> u64 {
        self.0 / 1_000
    }

    pub const fn as_micros(&self) -> u64 {
        self.0
    }

    /// Durée écoulée entre `earlier` et `self`. Sature à 0 si `earlier`
    /// est postérieur (ne devrait pas arriver avec une horloge monotone,
    /// mais évite un panic sur soustraction débordante).
    pub fn since(&self, earlier: Instant) -> Duration {
        Duration::from_micros(self.0.saturating_sub(earlier.0))
    }
}

impl core::ops::Sub for Instant {
    type Output = Duration;
    fn sub(self, rhs: Instant) -> Duration {
        self.since(rhs)
    }
}

impl core::ops::Add<Duration> for Instant {
    type Output = Instant;
    fn add(self, rhs: Duration) -> Instant {
        Instant(self.0.saturating_add(rhs.as_micros()))
    }
}

/// Durée entre deux instants, ou limite temporelle (timeout, délai
/// minimal, temps de conversion capteur). Représentée en microsecondes
/// comme `Instant` — leur arithmétique reste directe, sans conversion.
/// Contrairement à `Instant`, jamais cumulative sur la durée de vie de
/// l'appareil : les valeurs réelles de ce projet (`config.rs`) restent
/// toutes très en dessous de la portée d'un `u64` en µs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Duration(u64);

impl Duration {
    pub const fn from_millis(ms: u64) -> Self {
        Self(ms * 1_000)
    }

    pub const fn from_micros(us: u64) -> Self {
        Self(us)
    }

    pub const fn as_millis(&self) -> u64 {
        self.0 / 1_000
    }

    pub const fn as_micros(&self) -> u64 {
        self.0
    }
}

impl core::ops::Add for Duration {
    type Output = Duration;
    fn add(self, rhs: Duration) -> Duration {
        Duration(self.0.saturating_add(rhs.0))
    }
}

// ─── Implémentations concrètes (matériel ARM uniquement) ─────────────────────

#[cfg(all(rp2040, target_arch = "arm"))]
impl MonotonicTimer for rp2040_hal::Timer {
    fn now(&self) -> Instant {
        Instant::from_micros(self.get_counter().ticks())
    }
}

#[cfg(all(rp2350, any(target_arch = "arm", target_arch = "riscv32")))]
impl MonotonicTimer for rp235x_hal::Timer<rp235x_hal::timer::CopyableTimer0> {
    fn now(&self) -> Instant {
        Instant::from_micros(self.get_counter().ticks())
    }
}
