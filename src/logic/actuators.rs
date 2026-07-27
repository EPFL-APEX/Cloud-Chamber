// ─────────────────────────────────────────────────────────────────────────────
// Actionneurs — décision (ActuatorPlan, retournée par react_to) séparée de
// l'application (Actuators::apply). Voir logic/cooling.rs et logic/stopping.rs
// pour la justification : une table séparée risquait de diverger du match de
// transition (cf. régression déjà vue sur ce repo avec l'interlock HT), et un
// effet de bord direct dans react_to aurait empêché de tester la logique de
// séquencement avec cargo test-host sans simuler des GPIO.
// ─────────────────────────────────────────────────────────────────────────────

/// Ce que la phase courante demande aux actionneurs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActuatorPlan {
    pub compressor: bool,
    pub iso_heater: bool,
    pub high_voltage: bool,
}

/// Applique un `ActuatorPlan` au matériel — ne décide rien, exécute
/// seulement.
pub struct Actuators<Hv, Comp, Iso> {
    pub high_voltage: Hv,
    pub compressor: Comp,
    pub iso_heater: Iso,
}

impl<Hv, Comp, Iso> Actuators<Hv, Comp, Iso> {
    pub fn apply(&mut self, _plan: ActuatorPlan) {
        // Types/traits des actionneurs pas encore décidés (tout-ou-rien vs
        // PWM pour l'iso notamment) — cf. plan de réconciliation.
        todo!()
    }
}