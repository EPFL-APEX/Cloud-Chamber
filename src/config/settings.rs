//! Version modifiable en marche des cibles de [`super::operating`], et
//! abstraction du support de stockage persistant.
//!
//! Vit dans `config/` plutôt que dans `cloud_chamber_hal` : `Settings`
//! dépend de `super::operating` pour ses valeurs par défaut, et le HAL ne
//! doit jamais dépendre de `crate::config` (sens de dépendance inverse,
//! cf. doc de `config/mod.rs`).
//!
//! Les seuils de sécurité (`SAFETY_TEMP_COMPRESSOR_MAX`) n'y sont pas :
//! une limite qu'on peut changer depuis un menu n'est plus une limite, et
//! elle deviendrait aussi modifiable par une corruption de flash. Ils
//! restent des constantes compilées dans `operating.rs`. Même raisonnement
//! pour `sensor_loss_ms` (protection de sécurité, `logic::security`) et
//! `regulation_band` (encore consommé nulle part) : laissés de côté pour
//! l'instant plutôt que persistés sans lecteur ou avec une lecture
//! partielle qui donnerait une fausse impression de contrôle.
//!
//! # Pourquoi sérialiser champ par champ
//!
//! La tentation est d'écrire les octets bruts d'une `#[repr(C)]`. Mais le
//! padding entre champs de tailles différentes n'est pas initialisé : le
//! CRC changerait d'un build à l'autre sans que la moindre valeur ait
//! bougé. On écrit donc chaque champ explicitement, en petit-boutiste, ce
//! qui fixe aussi la représentation si on change un jour de cible.

use crate::cloud_chamber_hal::units::Celsius;

/// Version du schéma. À incrémenter dès qu'un champ est ajouté, retiré ou
/// change de sens — une version inconnue fait repartir sur les défauts
/// plutôt que de relire les anciens octets en décalé.
pub const SETTINGS_VERSION: u16 = 1;

/// Repère de début d'enregistrement. Une flash vierge est à `0xFF` partout ;
/// sans ce motif on relirait ces `0xFF` comme des `f32` valant `NaN`, et la
/// chambre démarrerait sur des consignes NaN.
const MAGIC: u32 = 0x43_43_53_31; // "CCS1"

const HEADER_LEN: usize = 8; // magic + version + longueur utile
const PAYLOAD_LEN: usize = 4 * 4; // 4 f32
const CRC_LEN: usize = 4;

/// Taille totale de l'enregistrement écrit en flash.
pub const RECORD_LEN: usize = HEADER_LEN + PAYLOAD_LEN + CRC_LEN;

/// Cibles modifiables en marche, sauvegardées entre deux démarrages.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub chamber_target: Celsius,
    pub precool_target: Celsius,
    pub saturation_target: Celsius,
    pub ipa_heater_target: Celsius,
}

/// Ce que le stockage peut refuser de faire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    /// L'écriture matérielle a échoué.
    Write,
    /// Relecture après écriture non conforme — la flash n'a pas pris.
    Verify,
}

/// Support de stockage persistant. `logic/` et `ui/` ne connaissent que ce
/// trait ; savoir ce qu'est un secteur de flash est le travail du driver.
pub trait SettingsStore {
    /// Relit les réglages. `None` si rien n'a jamais été écrit, ou si
    /// l'enregistrement est illisible — l'appelant repart alors sur
    /// `Settings::defaults()`.
    fn load(&mut self) -> Option<Settings>;

    /// Écrit les réglages. Opération lente et limitée en nombre de cycles :
    /// à n'appeler que sur demande explicite de l'opérateur, jamais à chaque
    /// cran d'encodeur.
    fn save(&mut self, settings: &Settings) -> Result<(), StoreError>;
}

impl Settings {
    /// Valeurs de repli, identiques aux constantes de `super::operating`.
    ///
    /// C'est la seule source de vérité au premier démarrage, après une
    /// corruption, et après un changement de version de schéma.
    pub const fn defaults() -> Self {
        use super::operating::*;
        Self {
            chamber_target: TARGET_CHAMBER_TEMP,
            precool_target: PRECOOL_TARGET_C,
            saturation_target: SATURATION_TARGET_C,
            ipa_heater_target: IPA_HEATER_TARGET_C,
        }
    }

    /// Sérialise en un bloc de taille fixe, prêt à écrire.
    pub fn to_bytes(&self) -> [u8; RECORD_LEN] {
        let mut out = [0u8; RECORD_LEN];
        out[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        out[4..6].copy_from_slice(&SETTINGS_VERSION.to_le_bytes());
        out[6..8].copy_from_slice(&(PAYLOAD_LEN as u16).to_le_bytes());

        let mut at = HEADER_LEN;
        for value in self.floats() {
            out[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }

        let crc = crc32(&out[..at]);
        out[at..at + 4].copy_from_slice(&crc.to_le_bytes());
        out
    }

    /// Relit un bloc. `None` dès que quelque chose ne colle pas — motif
    /// absent, version inconnue, longueur inattendue, CRC faux. On ne
    /// tente aucune récupération partielle : une valeur à moitié juste
    /// serait pire qu'un retour aux défauts.
    pub fn from_bytes(raw: &[u8; RECORD_LEN]) -> Option<Self> {
        if u32::from_le_bytes(raw[0..4].try_into().ok()?) != MAGIC {
            return None;
        }
        if u16::from_le_bytes(raw[4..6].try_into().ok()?) != SETTINGS_VERSION {
            return None;
        }
        if u16::from_le_bytes(raw[6..8].try_into().ok()?) as usize != PAYLOAD_LEN {
            return None;
        }
        let end = HEADER_LEN + PAYLOAD_LEN;
        if crc32(&raw[..end]) != u32::from_le_bytes(raw[end..end + 4].try_into().ok()?) {
            return None;
        }

        let mut at = HEADER_LEN;
        let mut floats = [0.0f32; 4];
        for slot in floats.iter_mut() {
            *slot = f32::from_le_bytes(raw[at..at + 4].try_into().ok()?);
            at += 4;
        }

        // Un CRC juste ne garantit qu'une chose : les octets sont ceux qu'on
        // a écrits. Il ne dit pas qu'ils ont du sens — un NaN écrit par un
        // bug se relirait proprement. On refuse donc aussi les non-finis.
        if floats.iter().any(|v| !v.is_finite()) {
            return None;
        }

        Some(Self {
            chamber_target: Celsius(floats[0]),
            precool_target: Celsius(floats[1]),
            saturation_target: Celsius(floats[2]),
            ipa_heater_target: Celsius(floats[3]),
        })
    }

    /// Ordre de sérialisation des champs. Cet ordre fait partie du format :
    /// le changer sans toucher `SETTINGS_VERSION` casserait la relecture
    /// des enregistrements existants.
    fn floats(&self) -> [f32; 4] {
        [
            self.chamber_target.0,
            self.precool_target.0,
            self.saturation_target.0,
            self.ipa_heater_target.0,
        ]
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::defaults()
    }
}

/// CRC-32 (polynôme IEEE réfléchi), calculé bit à bit.
///
/// La version par table est plus rapide mais coûte 1 ko de flash pour la
/// table. Ici on couvre une trentaine d'octets, deux fois par sauvegarde :
/// la vitesse n'a aucune importance, la place si.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_every_field() {
        let mut settings = Settings::defaults();
        settings.chamber_target = Celsius(-41.5);
        settings.ipa_heater_target = Celsius(38.0);

        let decoded = Settings::from_bytes(&settings.to_bytes()).unwrap();
        assert_eq!(decoded, settings);
    }

    #[test]
    fn blank_flash_is_rejected() {
        // Cas du Pico neuf : tout à 0xFF. Relu naïvement, ça donnerait des
        // NaN — c'est précisément ce que le motif doit intercepter.
        assert!(Settings::from_bytes(&[0xFF; RECORD_LEN]).is_none());
    }

    #[test]
    fn zeroed_flash_is_rejected() {
        assert!(Settings::from_bytes(&[0x00; RECORD_LEN]).is_none());
    }

    #[test]
    fn a_single_flipped_bit_is_caught() {
        let mut raw = Settings::defaults().to_bytes();
        raw[HEADER_LEN] ^= 0x01;
        assert!(Settings::from_bytes(&raw).is_none());
    }

    #[test]
    fn unknown_version_falls_back() {
        let mut raw = Settings::defaults().to_bytes();
        raw[4..6].copy_from_slice(&(SETTINGS_VERSION + 1).to_le_bytes());
        assert!(Settings::from_bytes(&raw).is_none());
    }

    #[test]
    fn non_finite_value_is_rejected() {
        // Un NaN écrit par un bug passerait le CRC : il est bien "celui
        // qu'on a écrit". C'est le contrôle de finitude qui l'arrête.
        let mut settings = Settings::defaults();
        settings.chamber_target = Celsius(f32::NAN);
        assert!(Settings::from_bytes(&settings.to_bytes()).is_none());
    }

    #[test]
    fn crc_matches_a_known_vector() {
        // Vecteur de référence du CRC-32 IEEE, pour vérifier que c'est bien
        // ce polynôme-là et pas une variante maison.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
