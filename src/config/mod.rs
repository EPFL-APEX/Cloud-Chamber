//! Configuration centrale du système, éclatée en catégories pour rester
//! navigable :
//!
//! - [`wiring`] : construction physique de la machine — sur quelle broche/
//!   adresse chaque capteur/actionneur est branché sur cette installation.
//! - [`operating`] : réglages physiques ajustables (cibles, tolérances,
//!   seuils de sécurité) — plusieurs sont déjà exposés comme valeurs par
//!   défaut modifiables dans `ui::screens::settings`.
//!
//! Le timing de la boucle de contrôle (timeouts, délais de séquence) vit à
//! part dans [`crate::logic::timing`], au plus près du code qui l'utilise,
//! plutôt qu'ici. Les compteurs de capteurs/actionneurs (`NUMBER_OF_*`) et
//! les index de rôle (`*_IDX`) restent dans
//! [`crate::cloud_chamber_hal::config`] — le HAL générique en a besoin comme
//! paramètres de type, et ne doit pas dépendre de ce module.

pub mod operating;
pub mod wiring;
