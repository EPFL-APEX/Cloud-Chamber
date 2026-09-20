//! Driver éclairage : relais GPIO tout-ou-rien (marche/arrêt).
//!
//! Le corps est celui de [`crate::drivers::relay::Relay`] — cf. sa doc pour
//! le pourquoi du marqueur de rôle.

use crate::drivers::relay::Relay;

/// Marqueur de rôle. Sa seule fonction est de rendre `Lights<P>` distinct
/// de `Pump<P>` et `WindowHeater<P>` aux yeux du compilateur.
pub struct LightsRole;

/// Éclairage de la chambre — deux ampoules sur le même circuit, pilotées
/// comme un seul actionneur.
pub type Lights<P> = Relay<P, LightsRole>;
