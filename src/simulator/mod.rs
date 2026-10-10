//! Simulateur physique de la chambre, pour faire tourner la **vraie**
//! boucle de contrôle et la **vraie** UI sur un ordinateur.
//!
//! # Pourquoi
//!
//! Le firmware est déjà testé bout en bout avec des mocks (`drivers::mock`),
//! mais un mock dit ce que le test veut entendre : `set_temp(-40.0)` et la
//! chambre « est » à -40 °C. Ce simulateur, lui, **modélise** : la
//! température descend parce que le compresseur tourne, parce que la
//! logique l'a décidé, parce que la sonde le mesure. On voit la physique
//! répondre au contrôle — le régulateur chasser sa cible, un capteur
//! tomber en panne et la sécurité réagir — exactement comme sur la
//! machine, juste sans la machine.
//!
//! # Le modèle thermique
//!
//! Chaque sonde est une **masse thermique** à sa température, qui échange
//! avec ses voisines par conduction et avec l'ambiance par fuite :
//!
//! ```text
//! dT/dt = (k_conduction × Σ(T_voisin − T) + k_ambiance × (T_ambiant − T)
//!          + P_appliquée / C) × dt
//! ```
//!
//! - la **plaque froide** (sonde chambre) reçoit la puissance du
//!   compresseur quand il tourne (négative : elle refroidit) ;
//! - le **bain d'isopropanol** reçoit celle du chauffage IPA ;
//! - la sonde **sortie compresseur** monte quand le compresseur tourne
//!   (charge) et retombe à l'ambiant quand il s'arrête ;
//! - les autres sondes traînent derrière la plaque (fuites thermiques).
//!
//! Les constantes sont calibrées pour que la séquence complète tienne en
//! quelques minutes de temps simulé — pas pour prédire un vrai copeau de
//! cuivre : les rapports importent, pas les valeurs absolues.
//!
//! # L'horloge virtuelle
//!
//! Tout le monde parle à [`VirtualClock`] : la boucle de contrôle (ses
//! timeouts), les capteurs (leurs horodatages), le simulateur (ses pas
//! de physique). L'avancer de `dt` fait avancer **tout** — un cycle de
//! 45 min se joue en quelques secondes réelles, et la boucle de contrôle
//! ne voit jamais la différence avec le temps réel.
//!
//! # Utilisation
//!
//! ```text
//! cargo run --release --target x86_64-unknown-linux-gnu \
//!     --features sim --bin chamber_sim
//! ```
//!
//! L'UI s'affiche dans une fenêtre SDL2 (nécessite SDL2 installé), le
//! terminal reste une console de télémétrie. Clavier : `←/→` ou `D/G`
//! encoder, `Entrée` clic, `P` panne sonde compresseur, `S` surchauffe,
//! `Espace` pause, `+/-` vitesse du temps virtuel, `Q` quitter.

pub mod physics;
pub mod sensors;
pub mod actuators;
pub mod clock;

pub use actuators::SimActuators;
pub use clock::VirtualClock;
pub use physics::ChamberPhysics;
