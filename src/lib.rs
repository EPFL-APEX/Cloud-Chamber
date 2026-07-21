#![no_std]
// Pendant le développement, beaucoup de symboles de la lib ne sont pas encore
// utilisés dans les binaires de test. 
#![allow(dead_code)]

/// Configuration centrale (broches, adresses, seuils, timings).
pub mod config;

/// Structures de données partagées entre les coeurs.
pub mod data;

/// Drivers capteurs + traits (DS18B20, BME280, ABP2).
pub mod sensors;

/// Logique de contrôle : TargetState, ControlOutput, PID, planificateur de mesures.
pub mod control;

/// Machine à états (SystemTask, phases Cooling/Stopping, historique de mesures).
pub mod logic;

/// Structures partagées (RingBuffer) — arborescence de la branche phases.
pub mod shared;

/// Boucle de sécurité (seuils deux niveaux, disjoncteur logiciel).
pub mod security_loop;

/// Affichage TFT ILI9341 (KMRTM28028-SPI).
pub mod display;
