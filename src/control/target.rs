use crate::config::TARGET_CHAMBER_TEMP;

/// Consignes du système — ce vers quoi le contrôleur cherche à aller.
#[derive(Clone, Copy, Debug)]
pub struct TargetState {
    /// Température cible de la base de la chambre (°C).
    pub chamber_temp_c: f32,
    /// Température cible de l'isopropanol (°C). TODO: valeur expérimentale.
    pub isopropanol_temp_c: f32,
    /// Autoriser l'activation du haut voltage quand la chambre est prête.
    pub high_voltage_enabled: bool,
}

impl Default for TargetState {
    fn default() -> Self {
        Self {
            chamber_temp_c: TARGET_CHAMBER_TEMP,
            // TODO équipe : consigne à déterminer expérimentalement — et valider
            // que ds3 (sortie évaporateur) est bien le bon capteur de référence
            // pour le chauffage IPA (cf. revue des décisions de design).
            isopropanol_temp_c: -20.0,
            high_voltage_enabled: false,
        }
    }
}
