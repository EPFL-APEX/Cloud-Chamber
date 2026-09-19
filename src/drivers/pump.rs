//! Driver pompe : relais GPIO tout-ou-rien (marche/arrêt).
//!
//! Le corps est celui de [`crate::drivers::relay::Relay`] — cf. sa doc pour
//! le pourquoi du marqueur de rôle.

use crate::drivers::relay::Relay;

/// Marqueur de rôle. Sa seule fonction est de rendre `Pump<P>` distinct de
/// `Lights<P>` et `WindowHeater<P>` aux yeux du compilateur.
pub struct PumpRole;

/// Pompe de circulation de l'isopropanol, pilotée par une sortie GPIO.
pub type Pump<P> = Relay<P, PumpRole>;
