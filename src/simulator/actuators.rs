//! Actionneurs simulés : la boucle de contrôle les pilote par les traits
//! du HAL, ils écrivent l'état dans la physique — et appliquent le même
//! hystérésis que les vrais drivers, pour que la boucle voie le même
//! comportement (un relais réel ne clignote pas à chaque bruit).

use crate::cloud_chamber_hal::actuators::{BinaryActuator, TargetActuator};
use crate::cloud_chamber_hal::measurement::Measurement;
use crate::cloud_chamber_hal::ring_buffer::RingBuffer;
use crate::cloud_chamber_hal::units::{Celsius, Unit};
use crate::drivers::regulate_method::{hysteresis, RegulationDirection};

use super::physics::PowerState;

/// Erreur d'actionneur simulé : jamais — la simulation n'a pas de
/// matériel qui lâche. Le type existe parce que le trait l'exige.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimActuatorError;

/// Un relais simulé : retient son état pour que la physique le lise.
///
/// Chaque instance écrit dans la case qui lui correspond de l'état de
/// puissance partagé — partagé par `&Cell` plutôt que `&mut`, parce que
/// `Actuators` du HAL réclame `&mut self` sur chaque actionneur
/// indépendamment : ils ne peuvent pas tous emprunter la même structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayKind {
    Cooling,
    IsoHeater,
    HighVoltage,
    IsoPump,
    Lights,
    GlassHeater,
}

pub struct SimRelay {
    kind: RelayKind,
    on: bool,
    /// Le panneau d'état partagé — écrit à chaque `turn_on`/`turn_off`,
    /// lu par le simulateur après le tour de boucle. `Rc` : les six
    /// relais partagent le MÊME panneau ; un `RefCell` cloné par relais
    /// écrirait dans six panneaux distincts et la physique ne verrait
    /// rien (bug réel du premier jet, attrapé par les tests).
    power: std::rc::Rc<core::cell::RefCell<PowerStateShared>>,
}

/// L'état partagé entre les six relais : `RefCell` dans une structure
/// détenue par le simulateur, clonée en lecture à chaque pas de physique.
#[derive(Debug, Clone, Copy, Default)]
pub struct PowerStateShared(pub PowerState);

impl SimRelay {
    fn new(kind: RelayKind, power: std::rc::Rc<core::cell::RefCell<PowerStateShared>>) -> Self {
        Self { kind, on: false, power }
    }

    fn set(&self, on: bool) {
        let mut p = self.power.borrow_mut();
        match self.kind {
            RelayKind::Cooling => p.0.cooling = on,
            RelayKind::IsoHeater => p.0.iso_heater = on,
            RelayKind::HighVoltage => p.0.high_voltage = on,
            RelayKind::IsoPump => p.0.iso_pump = on,
            RelayKind::Lights => {} // pas de physique de l'éclairage
            RelayKind::GlassHeater => p.0.glass_heater = on,
        }
    }
}

impl BinaryActuator for SimRelay {
    type Error = SimActuatorError;

    fn turn_on(&mut self) -> Result<(), Self::Error> {
        self.on = true;
        self.set(true);
        Ok(())
    }

    fn turn_off(&mut self) -> Result<(), Self::Error> {
        self.on = false;
        self.set(false);
        Ok(())
    }
}

/// Actionneur à hystérésis — le même comportement que
/// `drivers::compressor`/`drivers::heater` : le relais ne bascule que
/// hors de la bande, sinon la physique répondrait à chaque bruit de
/// mesure et la boucle clignoterait comme un sapin.
pub struct SimRegulatedRelay {
    inner: SimRelay,
    band: Celsius,
    /// Le froid régule vers le bas (ActivateBelow), le chauffage vers
    /// le haut (ActivateAbove) — le sens de la bande d'hystérésis.
    direction: RegulationDirection,
}

impl SimRegulatedRelay {
    pub fn new(inner: SimRelay, band: Celsius, direction: RegulationDirection) -> Self {
        Self { inner, band, direction }
    }

    pub fn is_on(&self) -> bool {
        self.inner.on
    }
}

impl TargetActuator<Celsius, 90> for SimRegulatedRelay {
    type Error = SimActuatorError;

    fn regulate(
        &mut self,
        hist: &RingBuffer<Measurement<Celsius>, 90>,
        target: Option<Celsius>,
    ) -> Result<(), Self::Error> {
        let Some(target) = target else {
            return self.inner.turn_off();
        };
        let Some(current) = hist.get(0).ok().filter(|m| !m.value.is_nan()) else {
            return self.inner.turn_off();
        };
        let should_be_on = hysteresis(current.value, target, self.band, self.inner.on, self.direction);
        if should_be_on {
            self.inner.turn_on()
        } else {
            self.inner.turn_off()
        }
    }
}

/// Les six actionneurs du simulateur, prêts à peupler le `Actuators`
/// du HAL. Tous partagent le même panneau `PowerState` : la physique le
/// lit après chaque tour de boucle.
pub struct SimActuators {
    pub high_voltage: SimRelay,
    pub cooling: SimRegulatedRelay,
    pub iso_heater: SimRegulatedRelay,
    pub iso_pump: SimRelay,
    pub lights: SimRelay,
    pub glass_heater: SimRelay,
    power: std::rc::Rc<core::cell::RefCell<PowerStateShared>>,
}

impl SimActuators {
    /// La bande d'hystérésis des régulés — la même que la config réelle
    /// (`REGULATION_BAND_C`), pour un comportement comparable à la
    /// machine.
    pub fn new() -> Self {
        let power = std::rc::Rc::new(core::cell::RefCell::new(PowerStateShared::default()));
        Self {
            high_voltage: SimRelay::new(RelayKind::HighVoltage, power.clone()),
            // Même direction que le vrai `drivers::compressor`
            // (`Upward` : ON quand la lecture est AU-DESSUS de la cible —
            // on refroidit quand il fait trop chaud).
            cooling: SimRegulatedRelay::new(
                SimRelay::new(RelayKind::Cooling, power.clone()),
                Celsius::new(1.0),
                RegulationDirection::Upward,
            ),
            // Même direction que le vrai `drivers::heater`
            // (`Downward` : ON quand la lecture est SOUS la cible —
            // on chauffe quand il fait trop froid).
            iso_heater: SimRegulatedRelay::new(
                SimRelay::new(RelayKind::IsoHeater, power.clone()),
                Celsius::new(1.0),
                RegulationDirection::Downward,
            ),
            iso_pump: SimRelay::new(RelayKind::IsoPump, power.clone()),
            lights: SimRelay::new(RelayKind::Lights, power.clone()),
            glass_heater: SimRelay::new(RelayKind::GlassHeater, power.clone()),
            power,
        }
    }

    /// L'état courant des sorties — ce que la physique doit appliquer.
    pub fn power_state(&self) -> PowerState {
        self.power.borrow().0
    }

    /// Le panneau partagé lui-même — pour le binaire du simulateur,
    /// qui décompose `SimActuators` en `Actuators` du HAL par valeur
    /// et doit continuer à lire l'état des sorties.
    pub fn panel(
        &self,
    ) -> std::rc::Rc<core::cell::RefCell<PowerStateShared>> {
        std::rc::Rc::clone(&self.power)
    }
}

impl Default for SimActuators {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_chamber_hal::timer::Instant;
    use crate::cloud_chamber_hal::units::Celsius;

    fn history_with(value: f32) -> RingBuffer<Measurement<Celsius>, 90> {
        let mut h = RingBuffer::filled(Measurement::new(Instant::ZERO, Celsius(f32::NAN)));
        h.push(Measurement::new(Instant::from_micros(1), Celsius(value)));
        h
    }

    #[test]
    fn cooling_regulated_relay_follows_the_target_with_hysteresis() {
        let mut actuators = SimActuators::new();
        let target = Celsius(-40.0);

        // À -30 °C (10 au-dessus de la cible) : la plaque doit refroidir.
        actuators.cooling.regulate(&history_with(-30.0), Some(target)).unwrap();
        assert!(actuators.power_state().cooling, "au-dessus de la cible : ON");

        // À -39,5 (dans la bande) : reste ON — pas de clignotement.
        actuators.cooling.regulate(&history_with(-39.5), Some(target)).unwrap();
        assert!(actuators.power_state().cooling, "dans la bande : reste ON");

        // Cible atteinte : reste ON tant que l'erreur > -band, i.e.
        // jusqu'à target+band (-39) — l'hystérésis du vrai compresseur.
        actuators.cooling.regulate(&history_with(-40.5), Some(target)).unwrap();
        assert!(actuators.power_state().cooling, "legerement sous la cible : reste ON");

        actuators.cooling.regulate(&history_with(-41.5), Some(target)).unwrap();
        assert!(!actuators.power_state().cooling, "sous la cible - bande : OFF");
    }

    #[test]
    fn none_target_cuts_everything() {
        let mut actuators = SimActuators::new();
        actuators.cooling.regulate(&history_with(-30.0), Some(Celsius(-40.0))).unwrap();
        assert!(actuators.power_state().cooling);

        actuators.cooling.regulate(&history_with(-30.0), None).unwrap();
        assert!(!actuators.power_state().cooling, "None = coupure forcée");
    }

    #[test]
    fn binary_relays_write_their_state() {
        let mut actuators = SimActuators::new();
        actuators.high_voltage.turn_on().unwrap();
        actuators.iso_pump.turn_on().unwrap();
        let p = actuators.power_state();
        assert!(p.high_voltage);
        assert!(p.iso_pump);
        assert!(!p.cooling);
        actuators.high_voltage.turn_off().unwrap();
        assert!(!actuators.power_state().high_voltage);
    }

    #[test]
    fn heater_regulates_upward() {
        let mut actuators = SimActuators::new();
        // L'IPA chauffe quand elle est SOUS sa cible (ActivateAbove).
        actuators.iso_heater.regulate(&history_with(10.0), Some(Celsius(40.0))).unwrap();
        assert!(actuators.power_state().iso_heater, "sous la cible : ON");

        // Cible dépassée : reste ON jusqu'à target+band (41 °C).
        actuators.iso_heater.regulate(&history_with(40.5), Some(Celsius(40.0))).unwrap();
        assert!(actuators.power_state().iso_heater, "dans la bande au-dessus : reste ON");

        actuators.iso_heater.regulate(&history_with(41.5), Some(Celsius(40.0))).unwrap();
        assert!(!actuators.power_state().iso_heater, "au-dela de la bande : OFF");
    }
}
