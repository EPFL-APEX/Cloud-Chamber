//! Unités physiques utilisées comme paramètre `Unit` de `Measurement<Unit>`.
//!
//! `Eq`/`Ord` ne sont pas dérivables ici : `f32` ne les implémente pas
//! (NaN casse toute relation d'ordre total).

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Celsius(pub f32);

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct HectoPascal(pub f32);

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Volt(pub f32);

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Ampere(pub f32);
