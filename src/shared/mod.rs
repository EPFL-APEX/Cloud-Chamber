//! Module partagé entre Core0 (UI/logging) et Core1 (boucle de sécurité).
//!
//! # Structure des modules en Rust
//!
//! En Rust, un module dans un répertoire `shared/` a besoin d'un fichier
//! `shared/mod.rs` pour servir de racine du module. Ce fichier déclare
//! les sous-modules avec `pub mod <nom>`.
//!
//! `pub mod` rend le sous-module accessible depuis l'extérieur du module parent.
//! Sans `pub`, le module serait privé (utilisable uniquement dans ce module).

/// Structures de données échangées entre les deux cœurs.
pub mod data;
