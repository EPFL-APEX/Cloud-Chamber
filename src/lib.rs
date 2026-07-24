//! Bibliothèque Cloud Chamber — réexporte tous les modules publics.
//!
//! Ce fichier transforme le projet en crate mixte (lib + bin).
//! Les exemples et tests d'intégration importent via `cloud_chamber::`.
//!
//! # `#![no_std]`
//!
//! La lib est `no_std` pour que les modules embarqués compilent sans std.
//! En mode test (`cargo test`), `cfg_attr` désactive `no_std` pour que
//! les tests s'exécutent sur desktop avec accès à la bibliothèque standard.

#![cfg_attr(not(test), no_std)]

pub mod cloud_chamber_hal;
pub mod config;
pub mod drivers;
pub mod logic;
pub mod shared;
pub mod ui;
