//! Bibliothèque Cloud Chamber — réexporte tous les modules publics.
//!
//! Ce fichier transforme le projet en crate mixte (lib + bin).
//! Les exemples et tests d'intégration importent via `cloud_chamber::`.
//!
//! # `no_std`… sauf le simulateur
//!
//! Deux exceptions hôte uniquement :
//!
//! - en mode test (`cargo test`), `cfg_attr` désactive `no_std` pour que
//!   les tests s'exécutent sur desktop avec la bibliothèque standard ;
//! - avec la feature `sim`, le simulateur (`src/simulator/`, le bin
//!   `chamber_sim`) est un programme desktop (temps réel, clavier,
//!   `Rc`/`RefCell`) : il force un build std complet. Cette feature n'est
//!   jamais activée pour une cible embarquée (le bin `chamber_sim` est
//!   `required-features = ["sim"]` et ne compile pas sur ARM/RISC-V).
#![cfg_attr(not(any(test, feature = "sim")), no_std)]

/// Mise en route de la carte (horloges, timer, GPIO) et configuration des
/// broches, partagées par `main.rs` et les binaires de bring-up. RP2040
/// uniquement : `rp2040-hal` n'est une dépendance que pour cette cible (cf.
/// Cargo.toml), et le module ne compile donc que là.
#[cfg(all(rp2040, target_arch = "arm"))]
pub mod board;

pub mod cloud_chamber_hal;
pub mod config;
pub mod drivers;
pub mod logic;
pub mod shared;
pub mod ui;

/// Simulateur physique de la chambre : la vraie boucle de contrôle et la
/// vraie UI, alimentées par un modèle thermique — jouable sur un
/// ordinateur, sans matériel. Cf. `src/simulator/mod.rs`.
#[cfg(feature = "sim")]
pub mod simulator;
