//! Unités physiques utilisées comme paramètre `Unit` de `Measurement<Unit>`.
//!
//! `Eq`/`Ord` ne sont pas dérivables ici : `f32` ne les implémente pas
//! (NaN casse toute relation d'ordre total).

use core::ops::{Add, AddAssign, Div, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Celsius(pub f32);

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct HectoPascal(pub f32);

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Volt(pub f32);

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Ampere(pub f32);

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Percentage(pub f32);


/// Tout ce dont `hysteresis`/`pid` ont besoin sur l'unité physique régulée
pub trait Unit:
    Copy
    + PartialOrd
    + Add<Output = Self>
    + AddAssign
    + Sub<Output = Self>
    + Neg<Output = Self>
    + Mul<f32, Output = Self>
    + Div<f32, Output = Self>
{
    /// Valeur neutre pour initialiser un accumulateur (intégrale/dérivée).
    fn zero() -> Self;
    fn is_nan(&self) -> bool;
}

impl Celsius {
    /// Constructeur `const` — le tuple `Celsius(x)` marche aussi, mais une
    /// constante de configuration se lit mieux en `Celsius::new(-40.0)`.
    pub const fn new(degrees: f32) -> Self {
        Self(degrees)
    }
}

impl Add for Celsius {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Celsius(self.0 + rhs.0)
    }
}

impl AddAssign for Celsius {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for Celsius {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Celsius(self.0 - rhs.0)
    }
}

impl Neg for Celsius {
    type Output = Self;
    fn neg(self) -> Self {
        Celsius(-self.0)
    }
}

impl Mul<f32> for Celsius {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Celsius(self.0 * rhs)
    }
}

impl Div<f32> for Celsius {
    type Output = Self;
    fn div(self, rhs: f32) -> Self {
        Celsius(self.0 / rhs)
    }
}

impl Unit for Celsius {
    fn zero() -> Self {
        Celsius(0.0)
    }
    fn is_nan(&self) -> bool {
        self.0.is_nan()
    }
}

