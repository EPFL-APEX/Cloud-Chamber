//! Modèle thermique de la chambre : chaque sonde est une masse qui
//! échange avec ses voisines et l'ambiance, et reçoit les puissances
//! des actionneurs.

use crate::cloud_chamber_hal::config::{
    PLATE_MID_IDX, COMPRESSOR_IDX, ISO_HEATER_IDX, NUMBER_OF_TEMP_SENSOR,
};

/// État des sorties que la boucle de contrôle a appliquées — ce que la
/// physique reçoit de `SimActuators` à chaque tour. `None` = la boucle
/// n'a pas d'avis (éclairage), la physique n'en a pas besoin.
#[derive(Debug, Clone, Copy, Default)]
pub struct PowerState {
    pub cooling: bool,
    pub iso_heater: bool,
    pub high_voltage: bool,
    pub iso_pump: bool,
    pub glass_heater: bool,
    /// Injecté par le scénario de test (`SimSensors::fail_compressor_probe`)
    /// quand on simule une panne : la sonde sortie-compresseur devient
    /// muette, mais la physique, elle, continue de calculer sa température
    /// réelle — c'est tout le point d'une simulation honnête.
    pub compressor_probe_failed: bool,
}

/// Paramètres thermiques — calibrés pour un cycle complet en quelques
/// minutes de temps virtuel, pas pour prédire un vrai montage.
///
/// Les ordres de grandeur restent physiquement plausibles : la plaque
/// refroidit en quelques minutes vers -40 °C, le compresseur chauffe
/// sa sortie lentement, l'IPA suit sa cible en dizaines de secondes.
#[derive(Debug, Clone, Copy)]
pub struct ChamberParams {
    /// Température ambiante (°C) — le point de départ de tout.
    pub ambient: f32,
    /// Puissance de refroidissement de la plaque (°C/s quand active).
    /// Négative : la plaque descend, asymptote vers `min_plate`.
    pub cooling_rate: f32,
    /// Plafond froid de la plaque (°C) — le compresseur seul n'atteint
    /// jamais mieux : c'est l'asymptote de la puissance frigorifique.
    pub cooling_floor: f32,
    /// Chauffage IPA (°C/s).
    pub ipa_heating_rate: f32,
    /// Plafond chaud du bain IPA (°C).
    pub ipa_ceiling: f32,
    /// Montée en température de la sortie compresseur sous charge (°C/s).
    pub compressor_load_rate: f32,
    /// Plafond de sortie compresseur sous charge (°C) — sous le seuil
    /// d'alarme de `SafetyMonitor` (120 °C) : la physique ne déclenche
    /// pas toute seule, c'est le test qui l'injecte via `overheat_compressor`.
    pub compressor_ceiling: f32,
    /// Relâchement de la sortie compresseur à l'arrêt (vers l'ambiant,
    /// °C/s de constante de temps — plus lent que la montée).
    pub compressor_cool_tau: f32,
    /// Conduction plaque → autres sondes (1/s) : les sondes du circuit
    /// suivent la plaque avec retard.
    pub conduction: f32,
    /// Fuite vers l'ambiance (1/s) : tout finit par revenir à 20 °C.
    pub leak: f32,
    /// Fuite de la plaque vers l'ambiance (1/s). Volontairement bien
    /// plus faible que `leak` : une vraie plaque froide est une masse
    /// isolée qui garde son froid des dizaines de minutes. C'est aussi
    /// ce qui rend la fenêtre de stabilité de `logic::cooling`
    /// (`high_voltage` : max−min ≤ 1 °C sur 60 s) franchissable.
    ///
    /// **Plafond dur** : à −35 °C, la dérive vaut `plate_leak × 55 °C`.
    /// La fenêtre ne passe que si 60 s de dérive tiennent dans la
    /// tolérance, soit `plate_leak ≤ 0.00025` environ. Au-dessus (essayé
    /// à 0.0004 : 1,32 °C par fenêtre), la phase HV ne passe jamais et
    /// chaque cycle finit au timeout — cf. le test de régression
    /// `cooling_sequence_completes_through_simulated_physics`.
    pub plate_leak: f32,
    /// Bruit de mesure (±°C) : une sonde réelle n'est pas nette, et le
    /// régulateur à hystérésis doit le tolérer.
    pub noise: f32,
}

impl Default for ChamberParams {
    fn default() -> Self {
        Self {
            ambient: 20.0,
            cooling_rate: -0.35,
            cooling_floor: -45.0,
            ipa_heating_rate: 0.15,
            ipa_ceiling: 45.0,
            compressor_load_rate: 0.4,
            compressor_ceiling: 95.0,
            compressor_cool_tau: 0.02,
            conduction: 0.004,
            leak: 0.0012,
            plate_leak: 0.0002,
            noise: 0.05,
        }
    }
}

/// La chambre simulée : `NUMBER_OF_TEMP_SENSOR` masses thermiques.
/// [`ChamberParams`] est sa configuration, passée au constructeur.
pub struct ChamberPhysics {
    params: ChamberParams,
    /// Températures **réelles** — ce que la physique calcule, pas ce que
    /// les sondes rapportent (le bruit et les pannes sont ajoutés à la
    /// lecture, jamais au modèle).
    temps: [f32; NUMBER_OF_TEMP_SENSOR],
    power: PowerState,
    /// Générateur pseudo-aléatoire déterministe (xorshift) : le bruit de
    /// mesure est reproductible d'un run à l'autre — un test de
    /// non-régression ne doit pas être un dé de temps en temps.
    rng: u32,
}

impl ChamberPhysics {
    /// Chambre au repos, partout à l'ambiant.
    pub fn new(params: ChamberParams) -> Self {
        Self {
            temps: [params.ambient; NUMBER_OF_TEMP_SENSOR],
            params,
            power: PowerState::default(),
            rng: 0x9E37_79B9,
        }
    }

    /// Ce que la boucle a appliqué aux actionneurs ce tour.
    pub fn apply_power(&mut self, power: PowerState) {
        self.power = power;
    }

    /// Avance la physique de `dt` secondes virtuelles.
    ///
    /// Modèle par sonde : chaque masse tend vers son équilibre
    /// (voisines + ambiance + source) à son rythme. La plaque est la
    /// seule à avoir une « source » froide ; l'IPA une source chaude ;
    /// la sortie compresseur monte sous charge.
    pub fn step(&mut self, dt: f32) {
        let p = &self.params;
        let before = self.temps;

        for i in 0..NUMBER_OF_TEMP_SENSOR {
            let t = before[i];
            let mut rate = 0.0;

            // Échange avec l'ambiance — tout le monde y revient. La
            // plaque a sa propre fuite, plus lente : cf. `plate_leak`.
            let leak = if i == PLATE_MID_IDX { p.plate_leak } else { p.leak };
            rate += leak * (p.ambient - t);

            // Conduction : la plaque est le pivot thermique du montage
            // — les autres sondes traînent derrière elle. La plaque,
            // elle, ne reçoit que la source froide du compresseur
            // (l'échange avec les voisines est du second ordre, la
            // plaque a une masse bien plus grande que les sondes).
            if i == PLATE_MID_IDX {
                if self.power.cooling {
                    let pull = (p.cooling_floor - t) * p.cooling_rate.abs();
                    rate += pull;
                }
            } else {
                rate += p.conduction * (before[PLATE_MID_IDX] - t);
                // Fuite inter-sondes vers l'ambiant : sans elle, une
                // sonde refroidie par conduction ne remonte jamais —
                // le test `everything_relaxes_to_ambient` l'a montré :
                // la conduction seule la laissait à 16 °C.
                rate += p.leak * 30.0 * (p.ambient - t);
            }

            // Le bain d'isopropanol : chauffage vers son plafond.
            if i == ISO_HEATER_IDX && self.power.iso_heater {
                let pull = (p.ipa_ceiling - t) * p.ipa_heating_rate.abs();
                rate += pull;
            }

            // La sortie compresseur : monte sous charge, retombe à
            // l'arrêt — bien plus vite qu'elle ne refroidit.
            if i == COMPRESSOR_IDX {
                if self.power.cooling {
                    let pull = (p.compressor_ceiling - t) * p.compressor_load_rate.abs();
                    rate += pull;
                } else {
                    rate += p.compressor_cool_tau * (p.ambient - t);
                }
            }

            self.temps[i] = t + rate * dt;
        }
    }

    /// Température **réelle** de la sonde `idx` (sans bruit, sans panne).
    pub fn real_temp(&self, idx: usize) -> f32 {
        self.temps[idx]
    }

    /// Température **rapportée** par la sonde `idx` : réelle + bruit,
    /// ou rien du tout si la panne est injectée.
    pub fn read_temp(&mut self, idx: usize) -> Option<f32> {
        if idx == COMPRESSOR_IDX && self.power.compressor_probe_failed {
            return None;
        }
        Some(self.temps[idx] + self.noise())
    }

    /// Bruit blanc faible, déterministe — xorshift32.
    fn noise(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        // [-1, 1] × amplitude.
        ((x as f32) / (u32::MAX as f32) - 0.5) * 2.0 * self.params.noise
    }

    /// Injecte une surchauffe compresseur : la température réelle de la
    /// sortie saute au-dessus du seuil d'alarme — pour voir la sécurité
    /// déclencher avec la physique pour cause, pas par décret.
    pub fn overheat_compressor(&mut self) {
        self.temps[COMPRESSOR_IDX] = 130.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cooling_pulls_the_plate_toward_its_floor() {
        let mut ch = ChamberPhysics::new(ChamberParams::default());
        ch.apply_power(PowerState { cooling: true, ..Default::default() });

        // 10 minutes de refroidissement : la plaque descend nettement.
        for _ in 0..600 {
            ch.step(1.0);
        }
        assert!(ch.real_temp(PLATE_MID_IDX) < 0.0, "la plaque doit refroidir");
        assert!(ch.real_temp(PLATE_MID_IDX) > -45.0, "sans traverser le plancher");
    }

    #[test]
    fn everything_relaxes_to_ambient() {
        let mut ch = ChamberPhysics::new(ChamberParams::default());
        ch.apply_power(PowerState { cooling: true, ..Default::default() });
        for _ in 0..600 {
            ch.step(1.0);
        }
        ch.apply_power(PowerState::default());
        // Une heure plus tard : la plaque garde encore son froid (fuite
        // faible, constante de temps ~80 min) — et les petites sondes,
        // couplées à elle par conduction, traînent derrière l'ambiant.
        for _ in 0..3600 {
            ch.step(1.0);
        }
        assert!(
            ch.real_temp(PLATE_MID_IDX) < -5.0,
            "la plaque doit encore garder son froid, pas {}",
            ch.real_temp(PLATE_MID_IDX)
        );
        // Huit heures plus tard : tout est revenu près de l'ambiant.
        for _ in 0..(8 * 720) {
            ch.step(5.0);
        }
        for i in 0..NUMBER_OF_TEMP_SENSOR {
            assert!(
                (ch.real_temp(i) - 20.0).abs() < 1.0,
                "sonde {i} doit revenir a l'ambiant, pas {}",
                ch.real_temp(i)
            );
        }
    }

    #[test]
    fn compressor_out_heats_under_load_and_cools_when_idle() {
        let mut ch = ChamberPhysics::new(ChamberParams::default());
        ch.apply_power(PowerState { cooling: true, ..Default::default() });
        for _ in 0..300 {
            ch.step(1.0);
        }
        let hot = ch.real_temp(COMPRESSOR_IDX);
        assert!(hot > 40.0, "la sortie compresseur chauffe sous charge");
        assert!(hot < 95.0, "sans depasser son plafond de charge");

        ch.apply_power(PowerState::default());
        for _ in 0..600 {
            ch.step(1.0);
        }
        assert!(
            ch.real_temp(COMPRESSOR_IDX) < 30.0,
            "et refroidit a l'arret"
        );
    }

    #[test]
    fn a_failed_probe_reads_nothing_but_the_physics_stays_true() {
        let mut ch = ChamberPhysics::new(ChamberParams::default());
        ch.apply_power(PowerState { cooling: true, compressor_probe_failed: true, ..Default::default() });
        for _ in 0..60 {
            ch.step(1.0);
        }
        assert_eq!(ch.read_temp(COMPRESSOR_IDX), None, "la sonde muette ne lit rien");
        assert!(
            ch.real_temp(COMPRESSOR_IDX) > 20.0,
            "mais la physique continue de calculer la vraie temperature"
        );
    }

    /// Le bruit est borné par `params.noise` — un capteur ne doit pas
    /// sauter de 5 °C d'une lecture à l'autre.
    #[test]
    fn noise_stays_within_bounds() {
        let mut ch = ChamberPhysics::new(ChamberParams::default());
        for _ in 0..1000 {
            let reading = ch.read_temp(PLATE_MID_IDX).unwrap();
            assert!(
                (reading - 20.0).abs() <= 0.2,
                "bruit hors bornes : {reading}"
            );
        }
    }
}
