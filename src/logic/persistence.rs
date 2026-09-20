//! Quand honorer une demande de sauvegarde des réglages.
//!
//! # Pourquoi il faut arbitrer
//!
//! Le RP2040 n'a pas de flash interne : le code vit dans une puce QSPI
//! externe et s'exécute en place (XIP). Pour écrire dedans, les routines ROM
//! sortent la QSPI du mode XIP, et pendant cette fenêtre **aucune
//! instruction en flash ne peut être lue, sur aucun des deux cœurs**. Le
//! cœur qui n'écrit pas doit donc se garer dans une boucle d'attente en RAM
//! — il ne fait plus rien du tout, pas de sondage capteur, pas de `tick()`,
//! pas de surveillance de sécurité.
//!
//! Sur cette machine, c'est le cœur 0 (UI) qui écrit et le cœur 1 (boucle de
//! contrôle) qui se gare. La question est donc : combien de temps peut-on
//! geler la boucle de contrôle, et quand ?
//!
//! # La réponse tient à l'asymétrie de la flash NOR
//!
//! Programmer ne sait que faire descendre des bits de 1 vers 0, par
//! injection d'électrons chauds — rapide et adressable à la page. Remonter
//! un bit à 1 demande un effet tunnel, plus lent par nature, et le circuit
//! haute tension qui le produit est câblé en commun sur tout un secteur :
//! on ne peut pas effacer moins de 4 Ko, et la puce boucle en
//! impulsion-vérification jusqu'à ce que la dernière des 32 768 cellules
//! ait lâché.
//!
//! | opération | typique | pire cas | fréquence |
//! |---|---|---|---|
//! | programmer une page | 0,7 ms | 3 ms | 15 fois sur 16 |
//! | effacer le secteur | 45 ms | 400 ms | 1 fois sur 16 |
//!
//! `drivers::flash_store` écrit en avançant sur seize emplacements, donc
//! quinze sauvegardes sur seize ne coûtent qu'une programmation de page :
//! moins de 3 ms de gel, soit moins qu'un créneau de lecture DS18B20. Elles
//! passent sans condition. Seule la seizième demande un effacement, et
//! celle-là attend que la machine soit à l'arrêt.
//!
//! # Ce que « différé » ne veut pas dire
//!
//! Les réglages modifiés sont **déjà appliqués** — ils vivent dans
//! `shared::settings`, que la boucle de contrôle relit à chaque tour. Ce qui
//! est différé, c'est uniquement leur survie à une coupure de courant.

use crate::config::settings::SaveCost;
use crate::shared::data::SystemTask;

/// Ce que l'appelant doit faire d'une demande de sauvegarde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveDecision {
    /// Écrire maintenant.
    Now,
    /// Garder la demande sous le coude et la représenter plus tard. Le
    /// réglage reste appliqué en attendant.
    Defer,
}

/// Arbitre une demande de sauvegarde.
///
/// La règle tient en une ligne : une écriture courte passe toujours, une
/// écriture longue attend l'arrêt.
///
/// `Tripped` compte comme « en marche » et non comme « à l'arrêt » : la
/// machine y est certes toutes sorties coupées, mais la boucle de contrôle
/// continue de surveiller, et c'est précisément le moment où on ne veut pas
/// l'aveugler 400 ms.
pub fn decide(cost: SaveCost, task: SystemTask) -> SaveDecision {
    match (cost, task) {
        (SaveCost::Cheap, _) => SaveDecision::Now,
        (SaveCost::Expensive, SystemTask::Idle) => SaveDecision::Now,
        (SaveCost::Expensive, _) => SaveDecision::Defer,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::cooling::CoolingPhase;
    use crate::logic::security::SafetyCause;
    use crate::logic::stopping::StoppingPhase;

    /// Tous les états de la machine, pour que les deux tests ci-dessous
    /// soient exhaustifs plutôt qu'illustratifs.
    const EVERY_TASK: [SystemTask; 6] = [
        SystemTask::Idle,
        SystemTask::Cooling(CoolingPhase::HighVoltage),
        SystemTask::Stabilising,
        SystemTask::Stopping(StoppingPhase::CutHighVoltage),
        SystemTask::Tripped(SafetyCause::CompressorOverheat),
        SystemTask::Cooling(CoolingPhase::SensorCheck),
    ];

    #[test]
    fn a_cheap_save_never_waits() {
        for task in EVERY_TASK {
            assert_eq!(
                decide(SaveCost::Cheap, task),
                SaveDecision::Now,
                "{task:?} : moins de 3 ms de gel, rien ne justifie d'attendre"
            );
        }
    }

    #[test]
    fn an_expensive_save_only_happens_at_rest() {
        assert_eq!(decide(SaveCost::Expensive, SystemTask::Idle), SaveDecision::Now);

        for task in EVERY_TASK.into_iter().filter(|t| *t != SystemTask::Idle) {
            assert_eq!(
                decide(SaveCost::Expensive, task),
                SaveDecision::Defer,
                "{task:?} : la boucle de controle ne doit pas etre gelee 400 ms ici"
            );
        }
    }

    /// `Tripped` n'est pas un état de repos du point de vue de la
    /// surveillance : la boucle continue de tourner et c'est le dernier
    /// moment où on veut l'aveugler.
    #[test]
    fn a_trip_is_not_a_rest_state() {
        assert_eq!(
            decide(SaveCost::Expensive, SystemTask::Tripped(SafetyCause::CompressorSensorLost)),
            SaveDecision::Defer,
        );
    }
}
