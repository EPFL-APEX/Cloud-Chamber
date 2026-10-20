//! Forme des tableaux `SensorSnapshot`/`MeasurementHistory`/`Sensors` :
//! combien de capteurs de chaque catégorie, et quel index correspond à quel
//! rôle physique.
//!
//! Ces constantes vivent ici plutôt que dans `crate::config` : elles
//! décrivent la forme de l'abstraction générique que `cloud_chamber_hal`
//! définit, pas le câblage concret (broches GPIO, adresses I²C — ça reste
//! dans `crate::config`, question différente de bring-up matériel) ni les
//! réglages de contrôle (seuils de sécurité, timing de phase — `crate::config`
//! aussi pour l'instant, chantier `logic/` séparé).
//!
//! ATTENTION : les slots `ds0..ds4` suivent l'ordre de découverte SEARCH ROM
//! du bus 1-Wire, pas un ordre physique fixe — à vérifier au boot (lignes
//! INFO ds{i}) avant de faire confiance à ces valeurs sur un nouveau montage.

/// Nombre de sondes de température (DS18B20 sur le bus 1-Wire).
pub const NUMBER_OF_TEMP_SENSOR: usize = 8;
/// Nombre de capteurs de pression (1 ABP2, pression chambre — pas de mesure
/// séparée basse/haute pression circuit réfrigérant).
pub const NUMBER_OF_PRESSURE_SENSOR: usize = 1;
/// Nombre d'ampèremètres.
pub const NUMBER_OF_AMPMETER: usize = 0;

// ─── Placement des huit sondes sur le bus 1-Wire ─────────────────────────────
//
// Le placement suit l'ordre de découverte SEARCH ROM, établi au banc
// (chaque sonde chauffée à la main s'identifie sur l'écran Stats — un
// nom par ligne). L'ordre ci-dessous est celui du montage réel.
//
// Circuit réfrigérant : compresseur → radiateur (in → out) → plaque
// froide (in → milieu → out). Le corps de chauffe de l'isopropanol
// porte sa propre sonde pour son thermostat.

/// Sonde posée sur le corps du compresseur — surveillance surchauffe.
pub const COMPRESSOR_IDX: usize = 0;
/// Entrée du radiateur.
pub const RADIATOR_IN_IDX: usize = 1;
/// Sortie du radiateur.
pub const RADIATOR_OUT_IDX: usize = 2;
/// Entrée de la plaque froide.
pub const PLATE_IN_IDX: usize = 3;
/// Milieu de la plaque froide — LA sonde pilote : c'est elle que la
/// séquence de refroidissement cible, que la sécurité surveille, que
/// l'écran Stats met en héroïne.
pub const PLATE_MID_IDX: usize = 4;
/// Sortie de la plaque froide.
pub const PLATE_OUT_IDX: usize = 5;
/// Corps de chauffe de l'isopropanol — sonde du thermostat IPA.
pub const ISO_HEATER_IDX: usize = 6;
/// Gaz à l'entrée du compresseur (aspiration). Lu, affiché — aucun rôle
/// de contrôle pour l'instant.
pub const COMPRESSOR_IN_IDX: usize = 7;

/// Le placement doit couvrir le bus sans doublon — vérifié à la
/// compilation, comme `ASSIGNED_PINS` pour les broches : réaffecter un
/// index à deux rôles doit casser le build, pas le bench.
const _: () = {
    let placements = [
        COMPRESSOR_IDX,
        RADIATOR_IN_IDX,
        RADIATOR_OUT_IDX,
        PLATE_IN_IDX,
        PLATE_MID_IDX,
        PLATE_OUT_IDX,
        ISO_HEATER_IDX,
        COMPRESSOR_IN_IDX,
    ];
    let mut i = 0;
    while i < placements.len() {
        assert!(placements[i] < NUMBER_OF_TEMP_SENSOR, "placement hors bus");
        let mut j = i + 1;
        while j < placements.len() {
            assert!(placements[i] != placements[j], "deux roles partagent une sonde");
            j += 1;
        }
        i += 1;
    }
};

/// Index de l'unique capteur de pression (ABP2, dans la chambre) dans `press`.
pub const CHAMBER_PRESSURE_IDX: usize = 0;

// ─── Rôles dont la logique de contrôle dépend ────────────────────────────────

/// Les capteurs sur lesquels `logic/` s'appuie pour décider quelque chose.
///
/// Un `usize` ne se vérifie pas : rien n'empêche d'ajouter un
/// `MACHIN_IDX: usize` et d'oublier la moitié des endroits qui devraient en
/// tenir compte. Un enum, si — à condition que les `match` qui l'exploitent
/// n'aient **pas de bras `_`**. C'est le cas des deux qui comptent :
///
/// - [`crate::logic::probing::MeasurementHistory::has_valid_reading_for`] —
///   *où* lire ce capteur ;
/// - `logic::cooling::required_to_start` — *faut-il l'attendre* avant de
///   laisser un cycle démarrer.
///
/// Ajouter une variante ici casse donc la compilation aux deux endroits,
/// avec le nom de la variante dans le message. Impossible d'ajouter un
/// capteur au système de contrôle sans répondre à ces deux questions.
///
/// Les sondes sans rôle de contrôle (radiateur, entrée/sortie de plaque,
/// entrée compresseur — cf.
/// [`TEMP_LABELS`](crate::config::wiring::TEMP_LABELS)) n'y figurent pas :
/// ils sont lus et affichés, mais aucune décision ne s'appuie dessus. Le
/// jour où l'un d'eux en gagne un, c'est ici qu'il entre.
#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, num_enum::TryFromPrimitive, num_enum::IntoPrimitive)]
pub enum ControlSensor {
    /// Sonde du milieu de plaque froide : pilote toute la séquence de
    /// refroidissement.
    PlateMid,
    /// Sonde du corps du compresseur : c'est elle que surveille
    /// `logic::security` pour la surchauffe.
    Compressor,
    /// Sonde du corps de chauffe de l'isopropanol (thermostat IPA).
    IsoHeater,
    /// Capteur de pression de la chambre.
    ChamberPressure,
}

impl ControlSensor {
    /// Tous les rôles, pour les parcourir.
    ///
    /// Tenu complet par `every_variant_is_listed_in_all` dans les tests :
    /// `TryFromPrimitive` sait exactement quels discriminants existent, le
    /// test compte jusqu'à ce qu'il refuse et compare. Une variante ajoutée
    /// et pas listée ici donne donc un test rouge — le `match` exhaustif
    /// ayant déjà, lui, bloqué la compilation.
    pub const ALL: [ControlSensor; 4] = [
        ControlSensor::PlateMid,
        ControlSensor::Compressor,
        ControlSensor::IsoHeater,
        ControlSensor::ChamberPressure,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ALL` doit contenir chaque variante exactement une fois.
    ///
    /// Le `match` exhaustif de `has_valid_reading_for` empêche déjà d'ajouter
    /// une variante en silence ; ce test ferme l'autre moitié du problème,
    /// celle que le typage ne couvre pas : une variante déclarée, traitée
    /// dans les `match`, mais absente de `ALL` — donc jamais parcourue.
    #[test]
    fn every_variant_is_listed_in_all() {
        let mut variants = 0usize;
        while ControlSensor::try_from(variants).is_ok() {
            variants += 1;
        }

        assert_eq!(
            variants,
            ControlSensor::ALL.len(),
            "une variante de ControlSensor manque dans ALL"
        );

        for (i, a) in ControlSensor::ALL.iter().enumerate() {
            for b in &ControlSensor::ALL[i + 1..] {
                assert_ne!(a, b, "doublon dans ALL");
            }
        }
    }
}
