#![no_std]
// Pendant le développement, beaucoup de symboles de la lib ne sont pas encore
// utilisés dans les binaires de test.
#![allow(dead_code)]

/// Liaison série USB avec l'hôte : transport, commandes, télémétrie.
pub mod comm;

/// Configuration centrale (broches, adresses, seuils, timings, phases).
pub mod config;

/// Structures de données d'acquisition Core0 (SystemState).
pub mod data;

/// Drivers capteurs + traits (DS18B20, BME280, ABP2).
pub mod sensors;

/// Logique de contrôle : Controller (machine à états), TargetState, ControlOutput.
pub mod control;

/// Interface utilisateur : rendu TFT ILI9341 (KMRTM28028-SPI) + tactile.
/// Renommé depuis `display` suite à la review PR #20 — le module porte toute
/// l'interaction utilisateur, pas seulement l'écran.
pub mod ui;

/// Machine à états (phases Cooling/Stopping, historique de mesures).
pub mod logic;

/// Sécurité : seuils deux niveaux, disjoncteur logiciel (ex-`security_loop`).
pub mod security;

/// Structures partagées (RingBuffer, SystemTask) — arborescence branche équipe.
pub mod shared;

// ── Modules de la branche équipe, conservés mais PAS ENCORE COMPILÉS ────────
// Ils dépendent des traits HAL abstraits et comportent des erreurs connues.
// Réactivation prévue par étapes du plan de convergence :
//   1. cloud_chamber_hal (units, Measurement, traits Sensor/BatchSensor)
//   2. drivers adaptés en wrappers autour des drivers éprouvés de sensors/
//   3. ui::{screens, navigator, theme, interactions} une fois branchés sur
//      ui::screen_driver (cf. src/ui/mod.rs et mod_equipe.rs.disabled)
// pub mod cloud_chamber_hal;
// pub mod drivers;
