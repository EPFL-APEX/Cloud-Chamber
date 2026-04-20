//! Types d'erreurs spécifiques à la boucle de sécurité.

/// Erreur produite par les opérations sur l'historique des capteurs.
#[derive(Debug, Copy, Clone)]
pub enum Error {
    /// L'index de capteur demandé dépasse le nombre de capteurs disponibles.
    SensorIndexOutOfBounds { index: usize },
    /// L'index dans le buffer historique est hors des limites valides.
    HistoryIndexOutOfBounds { index: usize },
}

pub type Result<T> = core::result::Result<T, Error>;
