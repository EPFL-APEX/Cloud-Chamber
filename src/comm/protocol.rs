//! Protocole série : commandes reçues de l'hôte, réponses renvoyées.
//!
//! # Architecture
//!
//! Réécrit pour ne plus dépendre de la moindre structure de `logic/` : la
//! version d'avant importait `PhaseClock`, `SafetyMonitor` et des
//! `request_start`/`request_stop` qui n'existent plus, et ne compilait
//! plus depuis le refactor de `control_loop` (quatre erreurs, cf. l'état
//! du projet dans le README). La machine à états s'est déplacée vers une
//! réconciliation par `SHARED_STATE` — le protocole fait maintenant comme
//! l'UI : il lit l'état, décide, et écrit la demande dans `SHARED_STATE`,
//! que `control_loop::tick` adopte au tour suivant.
//!
//! Même séparation décision/application que partout ailleurs :
//!
//! - [`decide`] : commande texte → décision (`Option<SystemTask>` à
//!   écrire, réponse à renvoyer). Pure, testable sur hôte, aucun
//!   périphérique.
//! - [`handle_command`] : la même décision, appliquée à `SHARED_STATE`,
//!   puis la réponse écrite sur le port USB par l'appelant.
//!
//! # Garde de démarrage
//!
//! `CYCLE 1` n'est accordé que depuis `Idle` — la même garde que
//! `ui::router::take_task_request`, pour la même raison : `tick` adopte
//! par conception ce qu'on lui écrit et ne peut pas distinguer un
//! démarrage voulu d'un redémarrage accidentel.
//!
//! # Commandes
//!
//! ```text
//! CYCLE 1   démarre un cycle (depuis Idle uniquement)
//! CYCLE 0   arrête le cycle en cours (séquence d'arrêt) ; réarme aussi
//!           un déclenchement sécurité
//! STATE     renvoie l'état courant et les mesures
//! TARGET/HV/COMP   pas encore supportés — erreur explicite, jamais de
//!                  simulation silencieuse
//! ```

use heapless::String;

use crate::logic::cooling::CoolingPhase;
use crate::logic::stopping::StoppingPhase;
use crate::shared::data::{SystemTask, SHARED_STATE};

/// Ce que la commande veut de la machine, une fois la décision prise.
///
/// `None` : rien à demander — une commande d'état, une erreur, un cycle
/// refusé. La réponse texte dit déjà tout ce qu'il y a à dire.
pub enum CommandOutcome {
    /// État à écrire dans `SHARED_STATE.task` — le même canal que l'UI.
    Request(SystemTask),
    /// Rien à écrire — la réponse suffit.
    None,
}

/// Capacité de la plus grande réponse (`STATE` : nom de phase + mesures).
/// 96 caractères couvrent la ligne d'état la plus longue avec marge.
pub const REPLY_CAPACITY: usize = 96;

/// Réponse à renvoyer à l'hôte. `String` à capacité fixe : le protocole
/// tourne sur une cible sans heap, et une réponse qui ne rentre pas est
/// tronquée par `write!` — jamais une allocation qui échoue.
pub type Reply = String<REPLY_CAPACITY>;

/// Traduit une commande en décision, sans toucher au moindre `static`.
///
/// `current` est l'état de la machine au moment de la commande, lu par
/// l'appelant — en paramètre plutôt qu'en lecture interne pour que la
/// fonction reste testable isolément, comme `Screens::take_task_request`.
/// Retourne la réponse à renvoyer à l'hôte et, le cas échéant, l'état
/// demandé.
pub fn decide(cmd: &str, current: SystemTask) -> (CommandOutcome, Reply) {
    let cmd = cmd.trim();
    let (name, rest) = cmd.split_once(' ').unwrap_or((cmd, ""));
    let rest = rest.trim();

    match name {
        "CYCLE" => match rest {
            "1" => {
                if current == SystemTask::Idle {
                    (
                        CommandOutcome::Request(SystemTask::Cooling(CoolingPhase::SensorCheck)),
                        reply("OK CYCLE=1"),
                    )
                } else {
                    // Le même refus que l'UI, pour la même raison — et le
                    // même message d'ordre : la machine est occupée, pas
                    // cassée.
                    (CommandOutcome::None, reply("ERR CYCLE refuse - cycle deja en cours"))
                }
            }
            "0" => match current {
                // Arrêt propre : la séquence de `logic::stopping`, pas un
                // `Idle` sec en plein cycle (HV comprise). Même sémantique
                // que l'appui long de l'UI.
                SystemTask::Cooling(_) | SystemTask::Stabilising => (
                    CommandOutcome::Request(SystemTask::Stopping(StoppingPhase::CutHighVoltage)),
                    reply("OK CYCLE=0"),
                ),
                // Déjà à l'arrêt : rien à faire. Y compris en `Stopping`
                // (une séquence en cours ne se rallume pas) et en
                // `Tripped` (une séquence d'arrêt ne réarme rien —
                // l'acquittement est un `Idle`, cf. ci-dessous).
                _ => (CommandOutcome::None, reply("OK CYCLE=0 (deja inactif)")),
            },
            _ => (CommandOutcome::None, reply("ERR CYCLE needs 0 or 1")),
        },
        "ACK" => (
            CommandOutcome::Request(SystemTask::Idle),
            reply("OK ACK"),
        ),
        "STATE" => (CommandOutcome::None, state_line(current)),
        "TARGET" => (CommandOutcome::None, reply("ERR TARGET pas encore supporte")),
        "HV" => (CommandOutcome::None, reply("ERR HV pas encore supporte")),
        "COMP" => (CommandOutcome::None, reply("ERR COMP pas encore supporte")),
        _ => (CommandOutcome::None, reply("ERR unknown command")),
    }
}

/// Ligne d'état : `STATE <task> T<chambre> C<compresseur> I<ipa> P<pression>`.
///
/// Les mesures manquantes s'affichent `---` — même règle que l'écran
/// Données : rien d'inventé.
fn state_line(task: SystemTask) -> Reply {
    let mut s = String::new();
    let task_name = match task {
        SystemTask::Idle => "idle",
        SystemTask::Cooling(CoolingPhase::SensorCheck) => "cooling:sensor_check",
        SystemTask::Cooling(CoolingPhase::PreCoolingThePlate) => "cooling:precool",
        SystemTask::Cooling(CoolingPhase::StartingIpaCirculation) => "cooling:ipa_circulation",
        SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa) => "cooling:saturation",
        SystemTask::Cooling(CoolingPhase::HighVoltage) => "cooling:high_voltage",
        SystemTask::Cooling(CoolingPhase::FinalCheckBeforeStabilising) => "cooling:final_check",
        SystemTask::Stabilising => "stabilising",
        SystemTask::Stopping(StoppingPhase::CutHighVoltage) => "stopping:cut_hv",
        SystemTask::Stopping(StoppingPhase::CutIsoprop) => "stopping:cut_isoprop",
        SystemTask::Stopping(StoppingPhase::CutCompressor) => "stopping:cut_compressor",
        SystemTask::Stopping(StoppingPhase::WaitPressureEquilibrium) => "stopping:equalize",
        SystemTask::Tripped(_) => "tripped",
    };
    let _ = core::fmt::Write::write_fmt(&mut s, format_args!("STATE {task_name}\r\n"));
    s
}

/// Applique une commande : lit l'état, décide via [`decide`], écrit la
/// demande dans `SHARED_STATE` — le même canal que l'UI — et rend la
/// réponse à écrire sur le port.
///
/// L'écriture USB reste à l'appelant (`usb_write`), qui possède le port.
pub fn handle_command(cmd: &str) -> Reply {
    let current = critical_section::with(|cs| SHARED_STATE.borrow_ref(cs).task);
    let (outcome, reply) = decide(cmd, current);
    match outcome {
        CommandOutcome::Request(task) => {
            critical_section::with(|cs| SHARED_STATE.borrow_ref_mut(cs).task = task);
        }
        CommandOutcome::None => {}
    }
    reply
}

/// Lecture des mesures courantes — ici plutôt que dans [`state_line`]
/// pour que `decide` reste pure et testable sans `static`.
pub fn snapshot_line() -> Reply {
    use crate::cloud_chamber_hal::config::{CHAMBER_PRESSURE_IDX, PLATE_MID_IDX, COMPRESSOR_IDX, ISO_HEATER_IDX};
    let snapshot = critical_section::with(|cs| SHARED_STATE.borrow_ref(cs).snapshot);
    let mut s = String::new();
    let _ = core::fmt::Write::write_fmt(
        &mut s,
        format_args!(
            "MEAS T={} C={} I={} P={}\r\n",
            fmt_measure(snapshot.temps[PLATE_MID_IDX].map(|m| m.value.0), "C"),
            fmt_measure(snapshot.temps[COMPRESSOR_IDX].map(|m| m.value.0), "C"),
            fmt_measure(snapshot.temps[ISO_HEATER_IDX].map(|m| m.value.0), "C"),
            fmt_measure(snapshot.press[CHAMBER_PRESSURE_IDX].map(|m| m.value.0), "hPa"),
        ),
    );
    s
}

/// `---` pour une mesure absente ou NaN — même règle que l'écran Données.
fn fmt_measure(value: Option<f32>, unit: &str) -> String<16> {
    let mut s = String::new();
    match value {
        Some(v) if !v.is_nan() => {
            let _ = core::fmt::Write::write_fmt(&mut s, format_args!("{v:.1}{unit}"));
        }
        _ => {
            let _ = core::fmt::Write::write_fmt(&mut s, format_args!("---"));
        }
    }
    s
}

/// Réponse littérale — le cas de loin le plus fréquent, en `String` fixe
/// pour un seul type de retour.
fn reply(text: &str) -> Reply {
    let mut s = String::new();
    let _ = s.push_str(text);
    s
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::cooling::CoolingPhase;
    use crate::logic::security::SafetyCause;
    use crate::logic::stopping::StoppingPhase;

    #[test]
    fn cycle_1_is_granted_from_idle() {
        let (outcome, reply) = decide("CYCLE 1", SystemTask::Idle);
        assert!(matches!(outcome, CommandOutcome::Request(SystemTask::Cooling(CoolingPhase::SensorCheck))));
        assert!(reply.starts_with("OK"));
    }

    #[test]
    fn cycle_1_is_refused_mid_cycle() {
        let running = SystemTask::Cooling(CoolingPhase::HighVoltage);
        let (outcome, reply) = decide("CYCLE 1", running);
        assert!(matches!(outcome, CommandOutcome::None));
        assert!(reply.starts_with("ERR"));
    }

    #[test]
    fn cycle_0_stops_a_running_cycle() {
        for running in [
            SystemTask::Cooling(CoolingPhase::HighVoltage),
            SystemTask::Stabilising,
        ] {
            let (outcome, _) = decide("CYCLE 0", running);
            assert!(
                matches!(outcome, CommandOutcome::Request(SystemTask::Stopping(StoppingPhase::CutHighVoltage))),
                "{running:?}"
            );
        }
    }

    #[test]
    fn cycle_0_on_an_idle_machine_is_a_no_op() {
        let (outcome, reply) = decide("CYCLE 0", SystemTask::Idle);
        assert!(matches!(outcome, CommandOutcome::None));
        assert!(reply.contains("deja inactif"));
    }

    /// Un déclenchement ne se réarme pas par `CYCLE 0` : une séquence
    /// d'arrêt ne réarme rien, l'acquittement est une demande `Idle`
    /// explicite (`ACK`) — le même geste que le clic sur l'écran de suivi.
    #[test]
    fn cycle_0_does_not_rearm_a_trip() {
        let tripped = SystemTask::Tripped(SafetyCause::CompressorOverheat);
        let (outcome, _) = decide("CYCLE 0", tripped);
        assert!(matches!(outcome, CommandOutcome::None));
    }

    #[test]
    fn ack_writes_idle() {
        let tripped = SystemTask::Tripped(SafetyCause::CompressorOverheat);
        let (outcome, _) = decide("ACK", tripped);
        assert!(matches!(outcome, CommandOutcome::Request(SystemTask::Idle)));
    }

    #[test]
    fn unknown_command_is_an_error() {
        let (outcome, reply) = decide("FROBNICATE", SystemTask::Idle);
        assert!(matches!(outcome, CommandOutcome::None));
        assert!(reply.starts_with("ERR"));
    }

    /// Exhaustivité : chaque état de la machine doit avoir un nom — c'est
    /// ce qui garantit que l'opérateur côté série voit la même machine que
    /// l'opérateur côté écran.
    #[test]
    fn state_line_names_every_task() {
        for task in [
            SystemTask::Idle,
            SystemTask::Cooling(CoolingPhase::SensorCheck),
            SystemTask::Cooling(CoolingPhase::PreCoolingThePlate),
            SystemTask::Cooling(CoolingPhase::StartingIpaCirculation),
            SystemTask::Cooling(CoolingPhase::SaturatingAirWithIpa),
            SystemTask::Cooling(CoolingPhase::HighVoltage),
            SystemTask::Cooling(CoolingPhase::FinalCheckBeforeStabilising),
            SystemTask::Stabilising,
            SystemTask::Stopping(StoppingPhase::CutHighVoltage),
            SystemTask::Stopping(StoppingPhase::CutIsoprop),
            SystemTask::Stopping(StoppingPhase::CutCompressor),
            SystemTask::Stopping(StoppingPhase::WaitPressureEquilibrium),
            SystemTask::Tripped(SafetyCause::CompressorOverheat),
        ] {
            let line = state_line(task);
            assert!(line.starts_with("STATE "), "{task:?}");
            assert!(line.len() > "STATE \r\n".len(), "{task:?} : nom vide");
        }
    }
}
