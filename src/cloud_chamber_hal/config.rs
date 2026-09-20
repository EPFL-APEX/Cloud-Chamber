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

/// Index de la sonde base-chambre (ds4) — cible du refroidissement.
pub const CHAMBER_TEMP_IDX: usize = 4;
/// Index de la sonde sortie-compresseur (ds0) — surveillance surchauffe.
pub const COMPRESSOR_OUT_IDX: usize = 0;
/// Index de la sonde utilisée par le thermostat chauffage isopropanol.
pub const ISO_TEMP_IDX: usize = 3;

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
/// Les emplacements 1-Wire sans rôle (ds5..ds7, libres — cf.
/// [`TEMP_LABELS`](crate::config::wiring::TEMP_LABELS)) n'y figurent pas :
/// ils sont lus et affichés, mais aucune décision ne s'appuie dessus. Le
/// jour où l'un d'eux en gagne un, c'est ici qu'il entre.
#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, num_enum::TryFromPrimitive, num_enum::IntoPrimitive)]
pub enum ControlSensor {
    /// Sonde base-chambre : pilote toute la séquence de refroidissement.
    ChamberTemp,
    /// Sonde sortie-compresseur : c'est elle que surveille
    /// `logic::security` pour la surchauffe.
    CompressorOut,
    /// Sonde du thermostat chauffage isopropanol.
    IsoTemp,
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
        ControlSensor::ChamberTemp,
        ControlSensor::CompressorOut,
        ControlSensor::IsoTemp,
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
