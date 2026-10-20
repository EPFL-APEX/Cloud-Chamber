//! Driver ventilateurs : relais GPIO tout-ou-rien (marche/arrêt).
//!
//! Le corps est celui de [`crate::drivers::relay::Relay`] — cf. sa doc pour
//! le pourquoi du marqueur de rôle.

use crate::drivers::relay::Relay;

/// Marqueur de rôle. Sa seule fonction est de rendre `Fans<P>` distinct
/// de `Pump<P>`, `Lights<P>`, `WindowHeater<P>` et `LedStrip<P>` aux yeux
/// du compilateur.
pub struct FansRole;

/// Ventilateurs du radiateur, pilotés comme un seul actionneur.
pub type Fans<P> = Relay<P, FansRole>;
