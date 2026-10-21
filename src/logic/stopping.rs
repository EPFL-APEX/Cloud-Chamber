//! Séquence d'arrêt propre.
//!
//! Ordre physique : HV off → circulation IPA off → compresseur off →
//! attendre l'équilibrage pression (l'équilibrage ne peut pas se produire
//! tant que le compresseur tourne, il maintient le ΔP). Comme `cooling.rs`, chaque phase construit
//! son propre `ActuatorPlan` en même temps que sa transition ; les délais
//! fixes (décharge HV, settle compresseur, équilibrage) sont gérés par
//! l'appelant, pas ici.
//!
//! Pas de capteur dédié à l'équilibrage du circuit réfrigérant (l'unique
//! capteur de pression restant mesure la chambre, pas le circuit — cf.
//! `cloud_chamber_hal::config::CHAMBER_PRESSURE_IDX`) : `WaitPressureEquilibrium`
//! est donc purement temporisée, comme `StartingIpaCirculation` dans
//! `cooling.rs`. La transition vers `Idle` est gérée par l'appelant via le
//! timeout de `SystemTask::time_limit()` (`STOP_EQUALIZE_FALLBACK`).

use crate::cloud_chamber_hal::actuators::ActuatorPlan;
use crate::logic::probing::{MeasurementHistory, ProbingPlan};
use crate::shared::data::SystemTask;
use crate::shared::settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoppingPhase {
    /// HV coupée immédiatement ; court délai de décharge géré par l'appelant.
    CutHighVoltage,

    CutIsoprop,
    /// Compresseur coupé.
    CutCompressor,
    /// Attente d'équilibrage pression — purement temporisée (pas de capteur
    /// dédié sur le circuit réfrigérant), avancement géré par l'appelant.
    WaitPressureEquilibrium,
}

impl StoppingPhase {
    /// Sonde tout à chaque cycle — cf. commentaire équivalent dans
    /// `logic::cooling::CoolingPhase::create_probing_plan`.
    pub fn create_probing_plan(&self, _prob_hist: &MeasurementHistory) -> ProbingPlan {
        ProbingPlan::all()
    }

    pub fn react_to(self, history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
        use StoppingPhase::*;
        match self {
            CutHighVoltage          => cut_high_voltage(history),
            CutIsoprop              => cut_isoprop(history),
            CutCompressor           => cut_compressor(history),
            WaitPressureEquilibrium => wait_pressure_equilibrium(history),
        }
    }
}

fn cut_high_voltage(_history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    // Délai de décharge géré par l'appelant (STOP_HV_SETTLE) ; HT coupée
    // dès l'entrée en phase. Froid encore actif (l'IPA continue de
    // circuler pendant la décharge) ; chauffage IPA coupé.
    let plan = ActuatorPlan::all_off()
        .with_cooling(settings::get().saturation_target)
        .with_fans()
        .with_iso_pump()
        .with_glass_heater();
    (SystemTask::Stopping(StoppingPhase::CutHighVoltage), plan)
}

fn cut_isoprop(_history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    // Géré par un délai, pas par un passage direct : la phase reste sur
    // elle-même et c'est `STOP_ISOPROP_SETTLE` (cf. `phase_clock`) qui
    // fait avancer vers `CutCompressor` — même mécanisme que
    // `CutHighVoltage`. Le froid reste actif pendant ce temps, pour que la
    // pompe s'arrête sans que la plaque se réchauffe.
    (SystemTask::Stopping(StoppingPhase::CutIsoprop), ActuatorPlan{
        cooling: Some(settings::get().saturation_target), iso_heater:None, high_voltage: false,
        iso_pump: false, lights: None, glass_heater: true, fans: true,
    })
}

fn cut_compressor(_history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    // Délai de settle géré par l'appelant (STOP_COMPRESSOR_SETTLE) ;
    // compresseur coupé dès l'entrée en phase.
    (SystemTask::Stopping(StoppingPhase::CutCompressor), ActuatorPlan::all_off())
}

fn wait_pressure_equilibrium(_history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    // Purement temporisé (pas de capteur dédié sur le circuit réfrigérant) —
    // avancement vers Idle décidé par l'appelant (cf. `timed_transition`
    // dans `phase_clock.rs`, même mécanisme que `StartingIpaCirculation`).
    (SystemTask::Stopping(StoppingPhase::WaitPressureEquilibrium), ActuatorPlan::all_off())
}

// ─── Ce qui tournait quand l'arrêt a commencé ────────────────────────────────

/// Mémoire des sorties actives au moment de l'arrêt.
///
/// Les plans d'arrêt ci-dessus décrivent un cycle **complet** — la machine
/// telle qu'elle est au bout de la séquence, tout allumé. Mais l'opérateur
/// peut arrêter n'importe quand : arrêter en plein pré-refroidissement
/// rallumait la pompe IPA quelques secondes (le plan `CutHighVoltage` la
/// maintient « encore active »), juste pour l'éteindre à la phase
/// suivante. Tout ce qui était éteint doit rester éteint ; seul ce qui
/// tournait suit les étapes pour s'éteindre.
///
/// La boucle de contrôle observe chaque plan appliqué hors arrêt
/// ([`RunningOutputs::observe`]), et restreint les plans d'arrêt
/// ([`RunningOutputs::restrict`]). Le verrou vit ici plutôt que dans les
/// plans eux-mêmes : ceux-ci restent la description honnête d'un arrêt
/// complet, et la collision avec la réalité se règle à un seul endroit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunningOutputs {
    cooling: bool,
    iso_heater: bool,
    high_voltage: bool,
    iso_pump: bool,
    /// L'éclairage garde l'état de l'opérateur (`None` = pas d'avis) :
    /// le verrou ne force jamais son extinction, il empêche seulement
    /// la séquence d'arrêt de le **rallumer**.
    lights: bool,
    glass_heater: bool,
    fans: bool,
}

impl RunningOutputs {
    pub const fn all_off() -> Self {
        Self {
            cooling: false,
            iso_heater: false,
            high_voltage: false,
            iso_pump: false,
            lights: false,
            glass_heater: false,
            fans: false,
        }
    }

    /// Mémorise ce que `plan` commande — l'état des sorties au moment où
    /// un arrêt sera demandé. `lights: None` (« pas d'avis ») laisse la
    /// mémoire telle quelle : la séquence d'arrêt n'en aura de toute façon
    /// pas davantage.
    pub fn observe(&mut self, plan: &ActuatorPlan) {
        self.cooling = plan.cooling.is_some();
        self.iso_heater = plan.iso_heater.is_some();
        self.high_voltage = plan.high_voltage;
        self.iso_pump = plan.iso_pump;
        if let Some(on) = plan.lights {
            self.lights = on;
        }
        self.glass_heater = plan.glass_heater;
        self.fans = plan.fans;
    }

    /// Retire du plan d'arrêt tout ce qui n'était pas allumé : la
    /// séquence ne rallume rien, elle ne fait qu'éteindre — dans l'ordre.
    pub fn restrict(&self, plan: &mut ActuatorPlan) {
        if !self.cooling {
            plan.cooling = None;
        }
        if !self.iso_heater {
            plan.iso_heater = None;
        }
        if !self.high_voltage {
            plan.high_voltage = false;
        }
        if !self.iso_pump {
            plan.iso_pump = false;
        }
        if !self.lights && plan.lights == Some(true) {
            plan.lights = Some(false);
        }
        if !self.glass_heater {
            plan.glass_heater = false;
        }
        if !self.fans {
            plan.fans = false;
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_chamber_hal::units::Celsius;

    /// Le scénario du terrain : arrêt pendant le pré-refroidissement —
    /// pompe IPA, HV, vitre et éclairage éteints, seul le compresseur
    /// tourne. Le plan `CutHighVoltage` les décrit pourtant « actifs »
    /// (il décrit l'arrêt d'un cycle complet) : sans verrou, la pompe
    /// s'allumait quelques secondes, juste pour s'éteindre ensuite.
    #[test]
    fn stopping_from_precooling_never_lights_the_ipa_pump() {
        crate::shared::settings::with_isolated_settings(|| {
            let mut running = RunningOutputs::all_off();
            running.observe(&ActuatorPlan::all_off().with_cooling(Celsius(-20.5)).with_fans());

            let (_, mut plan) =
                StoppingPhase::CutHighVoltage.react_to(&MeasurementHistory::new());
            running.restrict(&mut plan);

            assert!(!plan.iso_pump, "la pompe IPA etait eteinte, elle doit le rester");
            assert!(!plan.glass_heater, "la vitre etait eteinte, elle doit le rester");
            assert!(!plan.high_voltage, "la HV etait coupee, elle doit le rester");
            assert!(plan.cooling.is_some(), "le compresseur tournait, il suit la sequence");
            assert!(plan.fans, "les ventilateurs suivaient le compresseur");
        });
    }

    /// Arrêt depuis un cycle complet (tout ce qui doit tourner tourne) :
    /// le verrou ne retire rien de légitime, la séquence s'applique
    /// telle quelle.
    #[test]
    fn stopping_from_a_full_cycle_keeps_the_sequence_intact() {
        crate::shared::settings::with_isolated_settings(|| {
            let mut running = RunningOutputs::all_off();
            running.observe(&ActuatorPlan::all_off()
                .with_cooling(Celsius(-35.0))
                .with_iso_heater(Celsius(58.0))
                .with_high_voltage()
                .with_iso_pump()
                .with_lights(true)
                .with_glass_heater()
                .with_fans());

            let (_, mut plan) =
                StoppingPhase::CutHighVoltage.react_to(&MeasurementHistory::new());
            running.restrict(&mut plan);

            assert!(plan.iso_pump, "la pompe tournait, elle suit la sequence jusqu'a CutIsoprop");
            assert!(plan.glass_heater);
            assert!(plan.cooling.is_some());
            assert!(plan.fans);
        });
    }

    /// L'éclairage est à l'opérateur (`None` = pas d'avis) : le verrou
    /// ne l'éteint pas — un plan d'arrêt sans avis sur les lumières ne
    /// doit pas y en avoir un — il empêche seulement la séquence de les
    /// **rallumer**.
    #[test]
    fn lights_keep_operator_state_but_never_relight() {
        let mut running = RunningOutputs::all_off();
        running.observe(&ActuatorPlan::all_off().with_cooling(Celsius(-20.5)));

        let mut plan = ActuatorPlan::all_off();
        plan.lights = Some(true); // un plan qui voudrait les rallumer
        running.restrict(&mut plan);
        assert_eq!(plan.lights, Some(false), "eteintes a l'entree, pas de rallumage");

        // L'opérateur les avait laissées allumées : un plan sans avis les
        // laisse tranquilles.
        running.observe(&ActuatorPlan::all_off().with_lights(true));
        let mut plan = ActuatorPlan::all_off();
        plan.lights = None;
        running.restrict(&mut plan);
        assert_eq!(plan.lights, None, "pas d'avis, pas de changement force");
    }
}
