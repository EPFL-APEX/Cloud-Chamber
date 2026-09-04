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
    /// Origine de l'horloge monotone : le démarrage de l'appareil.
    pub const ZERO: Instant = Instant(0);

    /// Instant le plus tardif représentable (~584 942 ans d'uptime).
    pub const MAX: Instant = Instant(u64::MAX);

    pub const fn from_micros(us: u64) -> Self {
        Self(us)
    }

    /// Sature à [`Instant::MAX`] — cf. [`Duration::from_millis`], même
    /// raisonnement.
    pub const fn from_millis(ms: u64) -> Self {
        Self(ms.saturating_mul(1_000))
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

    /// Instant antérieur de `earlier`, le bord d'une fenêtre glissante.
    /// Sature à l'instant 0 (le démarrage), pas de temps négatif.
    ///
    /// À ne pas confondre avec [`Instant::since`] : ici on recule d'une
    /// durée et on obtient un instant, là on mesure l'écart entre deux
    /// instants et on obtient une durée. Même distinction que
    /// `std::time::Instant::checked_sub` face à `duration_since`.
    pub const fn saturating_sub(self, earlier: Duration) -> Instant {
        Instant(self.0.saturating_sub(earlier.as_micros()))
    }

    /// Comme [`Instant::saturating_sub`], mais `None` si le résultat
    /// passerait avant le démarrage.
    pub const fn checked_sub(self, earlier: Duration) -> Option<Instant> {
        match self.0.checked_sub(earlier.as_micros()) {
            Some(us) => Some(Instant(us)),
            None => None,
        }
    }
}

impl core::ops::Sub<Duration> for Instant {
    type Output = Instant;
    fn sub(self, rhs: Duration) -> Instant {
        self.saturating_sub(rhs)
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
/// minimal, temps de conversion capteur).
///
/// # Pourquoi pas `core::time::Duration`
///
/// `core::time::Duration` se représente en `u64` secondes + `u32`
/// nanosecondes, ce qui force ses accesseurs `as_millis`/`as_micros`/
/// `as_nanos` à renvoyer un `u128`. Sur le Cortex-M0+ du RP2040 il n'existe
/// pas de division 128 bits dans `compiler-rt` : LLVM la déroule à chaque
/// site d'appel. Mesuré en `-O` sur `thumbv6m-none-eabi`, une division par
/// 1 000 coûte 7 instructions en `u64` contre 366 en `u128`. La boucle de
/// contrôle appelle `as_millis()` à chaque tour.
///
/// D'où cette représentation : un simple `u64` de microsecondes, la même
/// que [`Instant`], leur arithmétique reste directe, sans conversion.
/// Contrairement à `Instant`, jamais cumulative sur la durée de vie de
/// l'appareil : les valeurs réelles de ce projet (`config.rs`) restent
/// toutes très en dessous de la portée d'un `u64` en µs.
///
/// # Écarts assumés vis-à-vis de `core::time::Duration`
///
/// L'API reprend celle de `core` — mêmes noms, mêmes signatures — avec
/// trois différences délibérées :
///
/// 1. **Les opérateurs `+`, `-`, `*`, `/` saturent au lieu de paniquer.**
///    `core` panique en cas de débordement ; ici un `panic!` sur une puce
///    sans opérateur n'a nulle part où être signalé et coupe la
///    surveillance de sécurité. Les variantes `checked_*` restent
///    disponibles pour les appelants qui veulent détecter le cas.
/// 2. **La granularité est la microseconde, pas la nanoseconde.**
///    [`Duration::from_nanos`] tronque, et [`Duration::as_nanos`] renvoie un
///    `u64` (sature à ~584 ans) plutôt qu'un `u128` — c'est tout l'intérêt
///    de l'exercice.
/// 3. **Pas de variantes `f64`** (`as_secs_f64`, `from_secs_f64`,
///    `mul_f64`...) : le M0+ n'a pas de FPU et la double précision logicielle
///    est nettement plus lourde que le simple. Aucun appelant n'en a besoin.
///    Les variantes `f32` sont là.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Duration(u64);

/// Microsecondes par seconde.
const MICROS_PER_SEC: u64 = 1_000_000;
/// Microsecondes par milliseconde.
const MICROS_PER_MILLI: u64 = 1_000;
/// Nanosecondes par microseconde, pour `from_nanos`/`as_nanos`.
const NANOS_PER_MICRO: u64 = 1_000;

impl Duration {
    /// Durée nulle.
    pub const ZERO: Duration = Duration(0);

    /// Durée maximale représentable (~584 942 ans).
    pub const MAX: Duration = Duration(u64::MAX);

    /// Une seconde. (`core` a le même, encore instable côté `std`.)
    pub const SECOND: Duration = Duration(MICROS_PER_SEC);

    /// Une milliseconde.
    pub const MILLISECOND: Duration = Duration(MICROS_PER_MILLI);

    /// Une microseconde — le quantum de cette représentation.
    pub const MICROSECOND: Duration = Duration(1);

    // ─── Constructeurs ──────────────────────────────────────────────────

    /// Durée de `secs` secondes plus `micros` microsecondes.
    ///
    /// Contrairement à `core::time::Duration::new`, `micros` n'a pas besoin
    /// d'être inférieur à une seconde : le surplus est simplement reporté.
    /// Sature à [`Duration::MAX`] au lieu de paniquer.
    pub const fn new(secs: u64, micros: u32) -> Self {
        Self(secs.saturating_mul(MICROS_PER_SEC).saturating_add(micros as u64))
    }

    /// Durée de `secs` secondes. Sature à [`Duration::MAX`].
    pub const fn from_secs(secs: u64) -> Self {
        Self(secs.saturating_mul(MICROS_PER_SEC))
    }

    /// Durée de `ms` millisecondes. Sature à [`Duration::MAX`].
    pub const fn from_millis(ms: u64) -> Self {
        Self(ms.saturating_mul(MICROS_PER_MILLI))
    }

    /// Durée de `us` microsecondes, conversion exacte, c'est la
    /// représentation interne.
    pub const fn from_micros(us: u64) -> Self {
        Self(us)
    }

    /// Durée de `ns` nanosecondes, **tronquée** à la microseconde
    /// inférieure : `from_nanos(1_999)` vaut 1 µs. La granularité de ce
    /// type est la microseconde.
    pub const fn from_nanos(ns: u64) -> Self {
        Self(ns / NANOS_PER_MICRO)
    }

    // ─── Accesseurs ─────────────────────────────────────────────────────

    /// `true` si la durée est nulle.
    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// Nombre entier de secondes (partie fractionnaire tronquée).
    pub const fn as_secs(&self) -> u64 {
        self.0 / MICROS_PER_SEC
    }

    /// Millisecondes au-delà de [`Duration::as_secs`] — toujours < 1 000.
    pub const fn subsec_millis(&self) -> u32 {
        ((self.0 % MICROS_PER_SEC) / MICROS_PER_MILLI) as u32
    }

    /// Microsecondes au-delà de [`Duration::as_secs`] — toujours < 1 000 000.
    pub const fn subsec_micros(&self) -> u32 {
        (self.0 % MICROS_PER_SEC) as u32
    }

    /// Durée totale en millisecondes (tronquée).
    ///
    /// `u64`, là où `core::time::Duration::as_millis` renvoie un `u128` —
    /// cf. la doc du type.
    pub const fn as_millis(&self) -> u64 {
        self.0 / MICROS_PER_MILLI
    }

    /// Durée totale en microsecondes — accès direct à la représentation.
    pub const fn as_micros(&self) -> u64 {
        self.0
    }

    /// Durée totale en nanosecondes, saturée à `u64::MAX` (~584 ans).
    pub const fn as_nanos(&self) -> u64 {
        self.0.saturating_mul(NANOS_PER_MICRO)
    }

    /// Durée en secondes flottantes. Le pendant utile de
    /// `as_millis() as f32 / 1_000.0`, écrit une seule fois.
    pub fn as_secs_f32(&self) -> f32 {
        self.0 as f32 / MICROS_PER_SEC as f32
    }

    /// Durée depuis un nombre de secondes flottantes. Les valeurs
    /// négatives, NaN ou hors portée donnent [`Duration::ZERO`] et
    /// [`Duration::MAX`] respectivement, plutôt qu'un panic.
    pub fn from_secs_f32(secs: f32) -> Self {
        // NaN teste explicitement : toutes ses comparaisons sont fausses,
        // il passerait donc au travers du seul `secs <= 0.0`.
        if secs.is_nan() || secs <= 0.0 {
            return Self::ZERO;
        }
        let micros = secs * MICROS_PER_SEC as f32;
        if micros >= u64::MAX as f32 { Self::MAX } else { Self(micros as u64) }
    }

    // ─── Arithmétique explicite ─────────────────────────────────────────

    /// Somme, ou `None` en cas de débordement.
    pub const fn checked_add(self, rhs: Duration) -> Option<Duration> {
        match self.0.checked_add(rhs.0) {
            Some(us) => Some(Duration(us)),
            None => None,
        }
    }

    /// Somme saturée à [`Duration::MAX`].
    pub const fn saturating_add(self, rhs: Duration) -> Duration {
        Duration(self.0.saturating_add(rhs.0))
    }

    /// Différence, ou `None` si `rhs` est plus grand que `self`.
    pub const fn checked_sub(self, rhs: Duration) -> Option<Duration> {
        match self.0.checked_sub(rhs.0) {
            Some(us) => Some(Duration(us)),
            None => None,
        }
    }

    /// Différence saturée à [`Duration::ZERO`].
    pub const fn saturating_sub(self, rhs: Duration) -> Duration {
        Duration(self.0.saturating_sub(rhs.0))
    }

    /// Produit, ou `None` en cas de débordement.
    pub const fn checked_mul(self, rhs: u32) -> Option<Duration> {
        match self.0.checked_mul(rhs as u64) {
            Some(us) => Some(Duration(us)),
            None => None,
        }
    }

    /// Produit saturé à [`Duration::MAX`].
    pub const fn saturating_mul(self, rhs: u32) -> Duration {
        Duration(self.0.saturating_mul(rhs as u64))
    }

    /// Quotient, ou `None` si `rhs` vaut 0.
    pub const fn checked_div(self, rhs: u32) -> Option<Duration> {
        if rhs == 0 { None } else { Some(Duration(self.0 / rhs as u64)) }
    }

    /// Écart absolu entre deux durées — jamais négatif, donc jamais
    /// débordant.
    pub const fn abs_diff(self, other: Duration) -> Duration {
        Duration(self.0.abs_diff(other.0))
    }

    /// Produit par un facteur flottant, saturé aux bornes.
    pub fn mul_f32(self, rhs: f32) -> Duration {
        Self::from_secs_f32(self.as_secs_f32() * rhs)
    }

    /// Quotient par un facteur flottant, saturé aux bornes.
    pub fn div_f32(self, rhs: f32) -> Duration {
        Self::from_secs_f32(self.as_secs_f32() / rhs)
    }

    /// Rapport entre deux durées. `rhs` nul donne `f32::INFINITY` (ou NaN
    /// si `self` est nul aussi) — sémantique du flottant, pas de panic.
    pub fn div_duration_f32(self, rhs: Duration) -> f32 {
        self.0 as f32 / rhs.0 as f32
    }
}

// ─── Opérateurs ─────────────────────────────────────────────────────────────
//
// Tous saturants, cf. l'écart n° 1 documenté sur le type.

impl core::ops::Add for Duration {
    type Output = Duration;
    fn add(self, rhs: Duration) -> Duration {
        self.saturating_add(rhs)
    }
}

impl core::ops::AddAssign for Duration {
    fn add_assign(&mut self, rhs: Duration) {
        *self = self.saturating_add(rhs);
    }
}

impl core::ops::Sub for Duration {
    type Output = Duration;
    fn sub(self, rhs: Duration) -> Duration {
        self.saturating_sub(rhs)
    }
}

impl core::ops::SubAssign for Duration {
    fn sub_assign(&mut self, rhs: Duration) {
        *self = self.saturating_sub(rhs);
    }
}

impl core::ops::Mul<u32> for Duration {
    type Output = Duration;
    fn mul(self, rhs: u32) -> Duration {
        self.saturating_mul(rhs)
    }
}

impl core::ops::Mul<Duration> for u32 {
    type Output = Duration;
    fn mul(self, rhs: Duration) -> Duration {
        rhs.saturating_mul(self)
    }
}

impl core::ops::MulAssign<u32> for Duration {
    fn mul_assign(&mut self, rhs: u32) {
        *self = self.saturating_mul(rhs);
    }
}

impl core::ops::Div<u32> for Duration {
    type Output = Duration;
    /// Diviser par zéro donne [`Duration::MAX`] plutôt qu'un panic
    /// arithmétique — utiliser [`Duration::checked_div`] pour détecter le
    /// cas.
    fn div(self, rhs: u32) -> Duration {
        match self.checked_div(rhs) {
            Some(d) => d,
            None => Duration::MAX,
        }
    }
}

impl core::ops::DivAssign<u32> for Duration {
    fn div_assign(&mut self, rhs: u32) {
        *self = *self / rhs;
    }
}

impl core::iter::Sum for Duration {
    fn sum<I: Iterator<Item = Duration>>(iter: I) -> Duration {
        iter.fold(Duration::ZERO, |acc, d| acc.saturating_add(d))
    }
}

impl<'a> core::iter::Sum<&'a Duration> for Duration {
    fn sum<I: Iterator<Item = &'a Duration>>(iter: I) -> Duration {
        iter.fold(Duration::ZERO, |acc, d| acc.saturating_add(*d))
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


#[cfg(test)]
mod tests {
    use super::*;

    // ─── Constructeurs et accesseurs ────────────────────────────────────

    /// Verrouille la raison d'être du type : 8 octets, pas les 16 de
    /// `core::time::Duration`. `Instant` est stocké 810 fois dans
    /// `MeasurementHistory` — la taille compte.
    #[test]
    fn duration_and_instant_stay_eight_bytes() {
        assert_eq!(core::mem::size_of::<Duration>(), 8);
        assert_eq!(core::mem::size_of::<Instant>(), 8);
    }


    #[test]
    fn constructors_agree_on_the_same_duration() {
        let one_and_a_half = Duration::from_micros(1_500_000);
        assert_eq!(Duration::from_millis(1_500), one_and_a_half);
        assert_eq!(Duration::new(1, 500_000), one_and_a_half);
        assert_eq!(Duration::from_secs(1) + Duration::from_millis(500), one_and_a_half);
    }

    #[test]
    fn accessors_split_seconds_and_remainder() {
        let d = Duration::from_micros(2_345_678);
        assert_eq!(d.as_secs(), 2);
        assert_eq!(d.subsec_millis(), 345);
        assert_eq!(d.subsec_micros(), 345_678);
        assert_eq!(d.as_millis(), 2_345);
        assert_eq!(d.as_micros(), 2_345_678);
    }

    #[test]
    fn from_nanos_truncates_to_the_microsecond() {
        assert_eq!(Duration::from_nanos(1_999), Duration::from_micros(1));
        assert_eq!(Duration::from_nanos(999), Duration::ZERO);
    }

    #[test]
    fn as_nanos_saturates_instead_of_wrapping() {
        assert_eq!(Duration::from_micros(3).as_nanos(), 3_000);
        assert_eq!(Duration::MAX.as_nanos(), u64::MAX);
    }

    #[test]
    fn zero_is_the_default() {
        assert!(Duration::default().is_zero());
        assert!(!Duration::from_micros(1).is_zero());
    }

    // ─── Saturation, l'écart assume vis-a-vis de core ───────────────────

    #[test]
    fn from_millis_saturates_instead_of_overflowing() {
        // Le bug d'origine : `ms * 1_000` debordait silencieusement.
        assert_eq!(Duration::from_millis(u64::MAX), Duration::MAX);
        assert_eq!(Duration::from_secs(u64::MAX), Duration::MAX);
    }

    #[test]
    fn operators_saturate_at_both_ends() {
        assert_eq!(Duration::MAX + Duration::from_secs(1), Duration::MAX);
        assert_eq!(Duration::ZERO - Duration::from_secs(1), Duration::ZERO);
        assert_eq!(Duration::MAX * 2, Duration::MAX);
    }

    #[test]
    fn checked_variants_report_the_overflow() {
        assert_eq!(Duration::MAX.checked_add(Duration::MICROSECOND), None);
        assert_eq!(Duration::ZERO.checked_sub(Duration::MICROSECOND), None);
        assert_eq!(Duration::MAX.checked_mul(2), None);
        assert_eq!(Duration::from_secs(1).checked_div(0), None);
        assert_eq!(
            Duration::from_secs(1).checked_add(Duration::from_secs(2)),
            Some(Duration::from_secs(3))
        );
    }

    #[test]
    fn division_by_zero_saturates_rather_than_panicking() {
        assert_eq!(Duration::from_secs(1) / 0, Duration::MAX);
    }

    // ─── Arithmetique ordinaire ─────────────────────────────────────────

    #[test]
    fn assign_operators_match_their_binary_form() {
        let mut d = Duration::from_millis(100);
        d += Duration::from_millis(50);
        assert_eq!(d, Duration::from_millis(150));
        d -= Duration::from_millis(50);
        assert_eq!(d, Duration::from_millis(100));
        d *= 3;
        assert_eq!(d, Duration::from_millis(300));
        d /= 2;
        assert_eq!(d, Duration::from_millis(150));
    }

    #[test]
    fn scalar_multiplication_commutes() {
        assert_eq!(Duration::from_millis(10) * 3, 3 * Duration::from_millis(10));
    }

    #[test]
    fn abs_diff_is_symmetric() {
        let a = Duration::from_millis(30);
        let b = Duration::from_millis(50);
        assert_eq!(a.abs_diff(b), Duration::from_millis(20));
        assert_eq!(b.abs_diff(a), Duration::from_millis(20));
    }

    #[test]
    fn sum_folds_an_iterator() {
        let ds = [Duration::from_millis(10), Duration::from_millis(20), Duration::from_millis(30)];
        let total: Duration = ds.iter().sum();
        assert_eq!(total, Duration::from_millis(60));
        let total_owned: Duration = ds.into_iter().sum();
        assert_eq!(total_owned, Duration::from_millis(60));
    }

    // ─── Flottants ──────────────────────────────────────────────────────

    #[test]
    fn secs_f32_round_trips() {
        let d = Duration::from_millis(1_500);
        assert!((d.as_secs_f32() - 1.5).abs() < 1e-6);
        assert_eq!(Duration::from_secs_f32(1.5), d);
    }

    #[test]
    fn from_secs_f32_clamps_hostile_inputs() {
        assert_eq!(Duration::from_secs_f32(-1.0), Duration::ZERO);
        assert_eq!(Duration::from_secs_f32(f32::NAN), Duration::ZERO);
        assert_eq!(Duration::from_secs_f32(f32::INFINITY), Duration::MAX);
    }

    #[test]
    fn float_scaling_works_both_ways() {
        assert_eq!(Duration::from_millis(100).mul_f32(2.5), Duration::from_millis(250));
        assert_eq!(Duration::from_millis(100).div_f32(4.0), Duration::from_millis(25));
        let ratio = Duration::from_millis(150).div_duration_f32(Duration::from_millis(50));
        assert!((ratio - 3.0).abs() < 1e-6);
    }

    // ─── Instant ────────────────────────────────────────────────────────

    #[test]
    fn instant_since_saturates_on_a_backward_clock() {
        let early = Instant::from_micros(100);
        let late = Instant::from_micros(500);
        assert_eq!(late.since(early), Duration::from_micros(400));
        assert_eq!(early.since(late), Duration::ZERO);
        assert_eq!(late - early, Duration::from_micros(400));
    }

    #[test]
    fn instant_plus_duration_advances_the_clock() {
        let t = Instant::from_micros(1_000);
        assert_eq!((t + Duration::from_millis(1)).as_micros(), 2_000);
    }
}
