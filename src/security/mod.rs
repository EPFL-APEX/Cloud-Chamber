//! Sécurité de la chambre — évaluation des seuils et disjoncteur logiciel.
//!
//! Renommé depuis `security_loop` suite à la review PR #20 : ce n'est plus
//! une boucle. Le module ne fait qu'évaluer un état et décider s'il faut
//! couper ; l'ordonnancement appartient à l'appelant (aujourd'hui la boucle
//! de contrôle Core0, à 10 Hz).
//!
//! - [`safety`]  : seuils deux niveaux (warn / alarm) et causes de coupure
//! - [`monitor`] : disjoncteur verrouillant avec anti-rebond

pub mod monitor;
pub mod safety;
