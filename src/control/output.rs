/// Commandes calculées par le contrôleur à chaque cycle.
#[derive(Clone, Copy, Debug, Default)]
pub struct ControlOutput {
    /// Compresseur : tout ou rien.
    pub compressor: bool,
    /// Rapport cyclique du chauffage isopropanol : 0.0 (off) → 1.0 (pleine puissance).
    pub isopropanol_heater_duty: f32,
    /// Haut voltage : tout ou rien.
    pub high_voltage: bool,
    /// Arrêt d'urgence actif — si true, tous les actionneurs dangereux sont coupés.
    pub safety_override: bool,
}

impl ControlOutput {
    /// État sûr : tout à l'arrêt, override signalé.
    pub const fn emergency_stop() -> Self {
        Self {
            compressor: false,
            isopropanol_heater_duty: 0.0,
            high_voltage: false,
            safety_override: true,
        }
    }
}
