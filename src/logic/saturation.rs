//! Sursaturation de la chambre : le seul chiffre qui dit si des traces sont
//! possibles maintenant.
//!
//! # Ce que mesure le rapport
//!
//! L'isopropanol s'évapore du feutre chaud et diffuse vers la plaque froide.
//! Le rapport des pressions de vapeur saturante aux deux extrémités,
//! `p_sat(chaud) / p_sat(froid)`, est la sursaturation maximale que ce
//! gradient peut produire. C'est une borne supérieure, pas la valeur locale
//! dans la couche sensible : la vapeur condense en chemin et appauvrit ce
//! qui reste. Une borne suffit pour l'affichage — elle est monotone en
//! température de plaque, donc elle répond bien à « est-ce que ça
//! progresse », et elle ne demande que deux sondes déjà présentes.
//!
//! # Pourquoi ces deux sondes
//!
//! Côté froid ds4 (`CHAMBER_TEMP_IDX`), la base de chambre, celle que toute
//! la séquence de refroidissement pilote. Côté chaud ds3 (`ISO_TEMP_IDX`),
//! la sonde du thermostat de chauffage isopropanol — donc le feutre, le
//! point chaud du gradient.
//!
//! ATTENTION : `config::wiring::TEMP_LABELS[3]` dit « sortie_evaporateur »
//! alors que `cloud_chamber_hal::config::ISO_TEMP_IDX` vaut 3. Les deux ne
//! peuvent pas être vrais : la sortie d'évaporateur est un point froid du
//! circuit frigorifique, pas le feutre. Tant que ce n'est pas tranché sur le
//! montage, le chiffre affiché par ce module est faux d'autant. Le module
//! ne peut pas trancher à notre place, mais `identify_temp_sensors` le peut.
//!
//! # Pourquoi la référence n'est pas une constante
//!
//! L'ancien firmware (`ui/screen_driver.rs`, branche `merge-Kynan-Thomas`)
//! annonçait « CHAMBRE PRETE » à partir de S ≥ 50, avec la température
//! ambiante comme point chaud. Ce seuil ne se transpose pas : avec le
//! feutre chauffé à `IPA_HEATER_TARGET_C`, S atteint 50 dès que la plaque
//! passe −17 °C, soit avant même la fin du pré-refroidissement. Plutôt que
//! de recalibrer un nombre magique à chaque changement de consigne, la
//! référence se déduit des réglages courants (cf. [`reference`]) : la barre
//! est pleine quand la chambre est à son point de fonctionnement, et elle
//! suit l'opérateur s'il modifie une cible depuis l'écran de réglages.

use libm::powf;

use crate::cloud_chamber_hal::units::Celsius;
use crate::shared::settings;

// Constantes d'Antoine du 2-propanol, forme log10(p_mmHg) = A - B/(C + t_C).
// Reprises telles quelles de l'ancien `ui::screen_driver::p_sat_ipa` : même
// corrélation, donc les chiffres restent comparables à ceux qu'on lisait
// avant la réécriture. L'unité (mmHg) n'a aucune importance ici, elle se
// simplifie dans le rapport.
const ANTOINE_A: f32 = 8.118;
const ANTOINE_B: f32 = 1580.92;
const ANTOINE_C: f32 = 219.617;

// Domaine de validité déclaré. Antoine diverge en `C + t = 0`, soit
// −219.6 °C : loin de nos températures, mais une sonde en défaut peut très
// bien rendre une valeur aberrante, et une division par ~0 donnerait un
// `inf` qui remonterait jusqu'à l'écran. On borne largement plutôt que de
// se fier au câblage.
const T_MIN_C: f32 = -80.0;
const T_MAX_C: f32 = 120.0;

/// Pression de vapeur saturante de l'isopropanol, en mmHg.
///
/// `None` hors du domaine déclaré ou sur `NaN` — une sonde 1-Wire absente
/// rend `NaN`, et le laisser se propager donnerait une barre de longueur
/// indéterminée plutôt qu'une absence d'affichage.
fn p_sat_ipa(temperature: Celsius) -> Option<f32> {
    let t = temperature.0;
    // `t < T_MIN_C` est faux pour `NaN` : le test explicite est nécessaire,
    // les comparaisons ne suffisent pas à l'écarter.
    if t.is_nan() || t < T_MIN_C || t > T_MAX_C {
        return None;
    }
    Some(powf(10.0, ANTOINE_A - ANTOINE_B / (ANTOINE_C + t)))
}

/// Sursaturation accessible entre un point chaud et un point froid.
///
/// Vaut 1.0 quand les deux sont à la même température (pas de gradient,
/// donc pas de sursaturation), et croît très vite en refroidissant.
pub fn ratio(warm: Celsius, cold: Celsius) -> Option<f32> {
    let numerator = p_sat_ipa(warm)?;
    let denominator = p_sat_ipa(cold)?;
    // `p_sat` ne s'annule pas sur le domaine borné ci-dessus, mais elle
    // descend à 1e-3 mmHg vers −80 °C : le garde-fou coûte une comparaison
    // et évite qu'un futur élargissement du domaine passe inaperçu.
    (denominator > 0.0).then_some(numerator / denominator)
}

/// Sursaturation visée au point de fonctionnement, d'après les réglages
/// courants : feutre à `ipa_heater_target`, plaque à `chamber_target`.
///
/// C'est le 100 % de la barre.
fn reference() -> Option<f32> {
    let settings = settings::get();
    ratio(settings.ipa_heater_target, settings.chamber_target)
}

/// Place une sursaturation sur l'échelle de la barre, dans `0.0..=1.0`.
///
/// Prend le rapport déjà calculé plutôt que les deux températures : l'écran
/// affiche la valeur brute à côté de la barre, et les faire sortir du même
/// appel à [`ratio`] est la seule façon de garantir qu'elles ne puissent
/// pas se contredire.
///
/// L'échelle démarre à S = 1 et non à S = 0 : S = 1 c'est « saturé, pas
/// encore sursaturé », le vrai zéro de l'échelle utile.
///
/// Linéaire, donc en retard sur la durée écoulée — à −35 °C, seuil de fin
/// de la phase de saturation, la barre n'est qu'à 58 %. C'est exact, pas un
/// défaut d'échelle : la sursaturation croît exponentiellement en
/// refroidissant, donc l'essentiel se joue effectivement sur les derniers
/// degrés. Une échelle logarithmique remplirait la barre plus régulièrement
/// mais afficherait 40 % à 0 °C, alors qu'aucune trace n'est possible.
pub fn scale(supersaturation: f32) -> Option<f32> {
    let reference = reference()?;
    // Consignes incohérentes (feutre plus froid que la plaque) : pas de
    // référence utilisable, donc pas de barre — plutôt qu'une division par
    // un nombre négatif qui remplirait la barre à l'envers.
    (reference > 1.0).then(|| ((supersaturation - 1.0) / (reference - 1.0)).clamp(0.0, 1.0))
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::settings::with_isolated_settings;

    /// Point d'ancrage physique : l'isopropanol bout à 82.6 °C sous 1 atm,
    /// donc `p_sat` doit y valoir 760 mmHg. Vérifie la corrélation contre le
    /// monde réel, pas contre elle-même. 5 % de tolérance : c'est l'ordre de
    /// l'écart d'une équation d'Antoine loin de sa plage d'ajustement.
    #[test]
    fn p_sat_matches_the_boiling_point() {
        let p = p_sat_ipa(Celsius(82.6)).unwrap();
        assert!((p - 760.0).abs() / 760.0 < 0.05, "p_sat(82.6) = {p} mmHg");
    }

    #[test]
    fn a_missing_probe_gives_no_ratio() {
        assert_eq!(ratio(Celsius(40.0), Celsius(f32::NAN)), None);
        assert_eq!(ratio(Celsius(f32::NAN), Celsius(-40.0)), None);
    }

    #[test]
    fn no_gradient_means_an_empty_bar() {
        with_isolated_settings(|| {
            assert_eq!(ratio(Celsius(20.0), Celsius(20.0)), Some(1.0));
            assert_eq!(scale(1.0), Some(0.0));
        });
    }

    /// Fixe la définition de la référence : aux deux consignes par défaut,
    /// la barre est pleine. Ce test casse si `reference()` change de sens.
    #[test]
    fn the_working_point_fills_the_bar() {
        with_isolated_settings(|| {
            let settings = settings::get();
            let full = ratio(settings.ipa_heater_target, settings.chamber_target).and_then(scale);
            assert_eq!(full, Some(1.0));
        });
    }
}
