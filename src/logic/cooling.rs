//! Séquence de démarrage (refroidissement).
//!
//! Chaque phase ne réagit qu'aux mesures disponibles dans l'historique et
//! construit son propre `ActuatorPlan` en même temps que sa transition — une
//! seule décision par phase, pas de table séparée ailleurs qui pourrait
//! diverger. Aucune notion de durée ici : les délais (attente minimale,
//! abandon si trop long) sont du ressort de l'appelant (`control_loop.rs`),
//! qui seul connaît la durée passée dans la phase courante.

use crate::cloud_chamber_hal::config::{CHAMBER_TEMP_IDX, ControlSensor};
use crate::cloud_chamber_hal::units::Celsius;
use crate::config::operating::{STABLE_TOLERANCE_C, STABLE_WINDOW};
use crate::cloud_chamber_hal::actuators::ActuatorPlan;
use crate::logic::probing::{MeasurementHistory, ProbingPlan};
use crate::shared::data::SystemTask;
use crate::shared::settings;

/// Tolérance accordée à `FinalCheckBeforeStabilising` au-dessus de la cible
/// de saturation.
///
/// La phase précédente a déjà franchi la cible ; celle-ci ne fait que
/// vérifier qu'on n'en est pas ressorti pendant l'établissement de la haute
/// tension. Un seuil strictement égal la ferait osciller sur le bruit de
/// mesure du DS18B20 (±0,5 °C à 12 bits).
const FINAL_CHECK_MARGIN: Celsius = Celsius::new(2.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoolingPhase {
    SensorCheck,
    PreCoolingThePlate,
    StartingIpaCirculation,
    SaturatingAirWithIpa,
    HighVoltage,
    FinalCheckBeforeStabilising,
}

impl CoolingPhase {
    /// Sonde tout à chaque cycle pour l'instant — l'optimisation "sauter la
    /// conversion température coûteuse (~800ms) sur certaines phases",
    /// l'intention originale de `ProbingPlan`, reste un raffinement
    /// ultérieur, pas requise pour un premier cycle correct.
    pub fn create_probing_plan(&self, _prob_hist: &MeasurementHistory) -> ProbingPlan {
        ProbingPlan::all()
    }

    pub fn react_to(self, history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
        use CoolingPhase::*;
        match self {
            SensorCheck                 => sensor_check(history),
            PreCoolingThePlate          => pre_cooling_the_plate(history),
            StartingIpaCirculation      => starting_ipa_circulation(history),
            SaturatingAirWithIpa        => saturating_air_with_ipa(history),
            HighVoltage                 => high_voltage(history),
            FinalCheckBeforeStabilising => final_check_before_stabilising(history),
        }
    }
}

/// Faut-il une lecture valide de ce capteur avant de laisser un cycle
/// démarrer ?
///
/// Le `match` est exhaustif et sans bras `_` — c'est tout l'intérêt :
/// ajouter un [`ControlSensor`] ne compile pas tant que cette question n'a
/// pas reçu de réponse. Un capteur ne peut donc pas entrer dans la logique
/// de contrôle sans que quelqu'un ait décidé si `SensorCheck` doit
/// l'attendre.
///
/// Répondre `false` est un choix légitime — mais un choix, pas un oubli.
const fn required_to_start(sensor: ControlSensor) -> bool {
    match sensor {
        // Pilote toute la séquence : chaque phase compare sa température à
        // une cible. Sans elle, rien ne peut avancer.
        ControlSensor::ChamberTemp => true,
        // C'est la sonde que surveille `logic::security`. Démarrer sans
        // elle, c'est démarrer sans protection contre la surchauffe — et
        // se faire abandonner 10 s plus tard par `CompressorSensorLost`,
        // sans savoir laquelle manquait.
        ControlSensor::CompressorOut => true,
        // Le thermostat IPA régule dessus dès `StartingIpaCirculation`.
        ControlSensor::IsoTemp => true,
        // Aucune phase ni aucune règle de sécurité ne s'en sert
        // aujourd'hui : elle est affichée, pas décisionnelle. À repasser à
        // `true` le jour où une transition en dépend.
        ControlSensor::ChamberPressure => false,
    }
}

fn sensor_check(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let plan = ActuatorPlan::all_off();

    // La phase s'appelle « vérification des capteurs » : elle les vérifie
    // tous, pas seulement celui de la chambre. Auparavant un démarrage
    // passait avec la sonde compresseur muette, et le cycle était abandonné
    // 10 s plus tard par la perte de capteur — sans dire laquelle.
    let all_present = ControlSensor::ALL
        .iter()
        .filter(|&&sensor| required_to_start(sensor))
        .all(|&sensor| history.has_valid_reading_for(sensor));

    match all_present {
        true => (SystemTask::Cooling(CoolingPhase::PreCoolingThePlate), plan),
        false => (SystemTask::Cooling(CoolingPhase::SensorCheck), plan),
    }
}

fn pre_cooling_the_plate(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let precool_target = settings::get().precool_target;
    let plan = ActuatorPlan::all_off().with_cooling(precool_target);
    match history.is_at_or_below(CHAMBER_TEMP_IDX, precool_target) {
        true => (SystemTask::Cooling(CoolingPhase::StartingIpaCirculation), plan),
        false => (SystemTask::Cooling(CoolingPhase::PreCoolingThePlate), plan),
    }
}

fn starting_ipa_circulation(_history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    // Purement temporisé (pas de capteur dédié) — avancement décidé par
    // l'appelant (cf. `timed_transition` dans control_loop.rs).
    let settings = settings::get();
    let plan = ActuatorPlan::all_off()
        .with_cooling(settings.precool_target)
        .with_iso_heater(settings.ipa_heater_target)
        .with_iso_pump()
        .with_glass_heater();
    (SystemTask::Cooling(CoolingPhase::StartingIpaCirculation), plan)
}

fn saturating_air_with_ipa(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let settings = settings::get();
    let plan = ActuatorPlan::all_off()
        .with_cooling(settings.saturation_target)
        .with_iso_heater(settings.ipa_heater_target)
        .with_iso_pump()
        .with_glass_heater();
    // #todo faire une vrai estimation de la saturation....
    match history.is_at_or_below(CHAMBER_TEMP_IDX, settings.saturation_target) {
        true => (SystemTask::Cooling(CoolingPhase::HighVoltage), plan),
        false => (SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa), plan),
    }
}

fn high_voltage(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let settings = settings::get();
    let plan = ActuatorPlan::all_off()
        .with_cooling(settings.saturation_target)
        .with_iso_heater(settings.ipa_heater_target)
        .with_high_voltage()
        .with_iso_pump()
        .with_lights(true)
        .with_glass_heater();

    // Est-ce qu'on veut vraiment check la stabilité ? Ou est-ce qu'on veut juste allumer le HV
    match history.is_temp_stable(CHAMBER_TEMP_IDX, STABLE_WINDOW, STABLE_TOLERANCE_C) {
        true  => (SystemTask::Cooling(CoolingPhase::FinalCheckBeforeStabilising), plan),
        false => (SystemTask::Cooling(CoolingPhase::HighVoltage), plan),
    }
}

fn final_check_before_stabilising(history: &MeasurementHistory) -> (SystemTask, ActuatorPlan) {
    let settings = settings::get();
    let plan = ActuatorPlan::all_off()
        .with_cooling(settings.saturation_target)
        .with_iso_heater(settings.ipa_heater_target)
        .with_high_voltage()
        .with_iso_pump()
        .with_lights(true)
        .with_glass_heater();

    // Qu'est-ce qu'on veut check ici ??
    match history.is_at_or_below(CHAMBER_TEMP_IDX, settings.saturation_target + FINAL_CHECK_MARGIN) {
        true => (SystemTask::Stabilising, plan),
        false => (SystemTask::Cooling(CoolingPhase::FinalCheckBeforeStabilising), plan),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_chamber_hal::config::{COMPRESSOR_OUT_IDX, ISO_TEMP_IDX};
    use crate::cloud_chamber_hal::measurement::Measurement;
    use crate::cloud_chamber_hal::timer::Instant;
    use crate::cloud_chamber_hal::units::HectoPascal;

    /// Un historique où tous les capteurs exigés au démarrage sont valides.
    fn all_sensors_alive() -> MeasurementHistory {
        let mut h = MeasurementHistory::new();
        let t0 = Instant::from_micros(1);
        h.temps[CHAMBER_TEMP_IDX].push(Measurement::new(t0, Celsius(20.0)));
        h.temps[COMPRESSOR_OUT_IDX].push(Measurement::new(t0, Celsius(20.0)));
        h.temps[ISO_TEMP_IDX].push(Measurement::new(t0, Celsius(20.0)));
        h
    }

    fn advanced(history: &MeasurementHistory) -> bool {
        sensor_check(history).0 == SystemTask::Cooling(CoolingPhase::PreCoolingThePlate)
    }

    #[test]
    fn every_required_sensor_present_lets_the_cycle_start() {
        assert!(advanced(&all_sensors_alive()));
    }

    #[test]
    fn a_fresh_history_stays_in_sensor_check() {
        assert!(!advanced(&MeasurementHistory::new()));
    }

    /// Le défaut que cette phase corrige : la sonde compresseur manquante
    /// laissait démarrer, et le cycle était abandonné dix secondes plus
    /// tard par `CompressorSensorLost` — sans dire laquelle manquait.
    #[test]
    fn a_missing_required_sensor_blocks_the_start() {
        for missing in ControlSensor::ALL.iter().filter(|&&s| required_to_start(s)) {
            let mut h = all_sensors_alive();
            // On efface la lecture en la remplaçant par un NaN plus récent.
            let t1 = Instant::from_micros(2);
            match missing {
                ControlSensor::ChamberTemp => {
                    h.temps[CHAMBER_TEMP_IDX].push(Measurement::new(t1, Celsius(f32::NAN)))
                }
                ControlSensor::CompressorOut => {
                    h.temps[COMPRESSOR_OUT_IDX].push(Measurement::new(t1, Celsius(f32::NAN)))
                }
                ControlSensor::IsoTemp => {
                    h.temps[ISO_TEMP_IDX].push(Measurement::new(t1, Celsius(f32::NAN)))
                }
                ControlSensor::ChamberPressure => unreachable!("non exigee au demarrage"),
            }
            assert!(!advanced(&h), "{missing:?} manquante doit bloquer le demarrage");
        }
    }

    /// La pression n'est exigée par aucune phase ni aucune règle de
    /// sécurité : son absence ne doit pas empêcher de démarrer. Ce test
    /// tombera le jour où `required_to_start` la passera à `true`, ce qui
    /// est exactement le rappel voulu.
    #[test]
    fn an_optional_sensor_does_not_block_the_start() {
        assert!(!required_to_start(ControlSensor::ChamberPressure));

        let mut h = all_sensors_alive();
        assert!(!h.has_valid_reading_for(ControlSensor::ChamberPressure));
        assert!(advanced(&h), "la pression manquante ne doit pas bloquer");

        // Et sa présence ne change rien non plus.
        h.press[0].push(Measurement::new(Instant::from_micros(1), HectoPascal(1013.0)));
        assert!(advanced(&h));
    }
}
