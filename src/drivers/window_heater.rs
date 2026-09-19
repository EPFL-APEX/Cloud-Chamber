//! Driver chauffage de la vitre supérieure : relais GPIO tout-ou-rien
//! (marche/arrêt) — anti-buée/anti-givre.
//!
//! Le corps est celui de [`crate::drivers::relay::Relay`] — cf. sa doc pour
//! le pourquoi du marqueur de rôle.

use crate::drivers::relay::Relay;

/// Marqueur de rôle. Sa seule fonction est de rendre `WindowHeater<P>`
/// distinct de `Pump<P>` et `Lights<P>` aux yeux du compilateur.
pub struct WindowHeaterRole;

/// Chauffage de la vitre supérieure, piloté par une sortie GPIO.
pub type WindowHeater<P> = Relay<P, WindowHeaterRole>;
