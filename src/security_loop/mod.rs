//! Boucle de sécurité temps-réel s'exécutant sur Core1.
//!
//! # Architecture
//!
//! Ce module implémente la boucle de contrôle critique qui tourne en continu
//! sur le second cœur du RP2040/RP2350. Elle a la priorité absolue et ne doit
//! jamais être bloquée.
//!
//! ## Structure temporelle de chaque itération
//!
//! ```text
//! ┌──────────────────────────────────────────────────────┐ période = 10 ms
//! │ Phase 1 (obligatoire)                                │
//! │  critical_sensor_read()  →  evaluate_and_react()    │
//! ├──────────────────────────────────────────────────────┤
//! │ Phase 2 (si budget temporel restant)                 │
//! │  non_critical_sensor_read()  +  push_to_core0()     │
//! ├──────────────────────────────────────────────────────┤
//! │ Attente busy-wait jusqu'à la prochaine période       │
//! └──────────────────────────────────────────────────────┘
//! ```
//!
//! # Organisation des sous-modules
//!
//! - [`error`]       : types d'erreurs spécifiques à la boucle de sécurité
//! - [`states`]      : historique des mesures capteurs (buffers circulaires)
//! - [`safety`]      : logique d'évaluation des seuils de sécurité
//! - [`loop_runner`] : structure principale `SecurityLoop` et boucle `run()`

//pub mod error;
//pub mod loop_runner;
//pub mod safety;
//pub mod states;
