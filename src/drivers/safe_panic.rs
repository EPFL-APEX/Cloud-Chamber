//! Gestionnaire de panique qui coupe les sorties de puissance avant tout.
//!
//! # Le défaut qu'il corrige
//!
//! Avec `panic_probe`, une panique du cœur 1 (la boucle de contrôle) laissait
//! les relais **en l'état** — haute tension éventuellement enclenchée,
//! compresseur éventuellement en marche — pendant que le cœur 0 continuait
//! d'afficher une machine normale. Personne ne pilote plus rien, personne ne
//! ne le voit : c'est le pire des deux mondes, et le README le classait
//! manque structurant n°1.
//!
//! Ce module fournit le cœur de la réponse :
//!
//! 1. [`cut_power_outputs`] force **toutes** les broches relais au niveau
//!    bas, par accès registre direct — pas par les objets `Pin` du HAL,
//!    devenus inaccessibles depuis un `panic_handler` (ils appartiennent à
//!    la boucle de contrôle, et le gestionnaire n'a pas de moyen de les
//!    atteindre) ;
//! 2. [`mark_core1_panicked`] publie le drapeau dans `SHARED_STATE`, pour
//!    que le cœur 0 affiche l'erreur au lieu d'une machine saine.
//!
//! # Pourquoi du registre brut plutôt que le HAL
//!
//! Le `panic_handler` ne reçoit que `&PanicInfo` : aucun état, aucune
//! `Pin`, aucun périphérique. Reste l'adresse des registres SIO, qui ne
//! dépend de rien. C'est la même commodité que `drivers::flash_rp2040` dans
//! sa boucle d'attente, et pour la même raison : ici, ne dépendre de rien
//! est la propriété recherchée.
//!
//! `GPIO_OUT_W1TC` (« write-1-to-clear ») est fait pour ça : écrire le
//! masque des broches à couper coupe tout seul la sortie du SIO, sans
//! toucher à la direction ni aux pulls. L'état « relais » de chaque
//! broche correspond au niveau bas (optocoupleurs pilotés à l'état bas
//! coupés — cf. `board::configure_relay_pin` qui force le bas avant tout).
//!
//! # Ce que ce module n'est pas
//!
//! Pas une récupération : une fois la panique consommée, le cœur ne
//! reprendra pas. Les sorties sont sûres, l'opérateur est prévenu, la
//! sonde (defmt) garde la trace — le redémarrage reste l'affaire d'un
//! reflash ou d'un reset.

use crate::config::wiring::{
    PIN_COMPRESSOR_RELAY, PIN_HV_RELAY, PIN_ISO_HEATER_RELAY, PIN_LIGHTS_RELAY,
    PIN_PUMP_RELAY, PIN_WINDOW_HEATER_RELAY,
};

/// Base du bloc SIO, seule dépendance de ce module.
const SIO_BASE: u32 = 0xd000_0000;
/// `SIO.GPIO_OUT_W1TC` : écriture « 1 → clear » du registre des sorties.
const GPIO_OUT_W1TC: u32 = SIO_BASE + 0x04;

/// Toutes les broches de sortie de puissance, coupées ensemble.
const POWER_OUTPUT_PINS: [u8; 6] = [
    PIN_HV_RELAY,
    PIN_COMPRESSOR_RELAY,
    PIN_ISO_HEATER_RELAY,
    PIN_PUMP_RELAY,
    PIN_LIGHTS_RELAY,
    PIN_WINDOW_HEATER_RELAY,
];

/// Force toutes les sorties de puissance au niveau bas.
///
/// # Safety
///
/// `GPIO_OUT_W1TC` est un registre MMIO aligné sur 4 du SIO ; l'adresse est
/// une constante du RP2040. Le `asm!` (plutôt que `ptr::write_volatile`)
/// garantit qu'aucun appel de fonction n'est généré — le contexte d'un
/// gestionnaire de panique n'offre aucune garantie sur ce qui est encore
/// appelable, et `asm!` ne peut pas devenir un appel par construction.
#[cfg(any(target_arch = "arm", target_arch = "riscv32"))]
pub fn cut_power_outputs() {
    let mut mask: u32 = 0;
    for pin in POWER_OUTPUT_PINS {
        mask |= 1 << (pin as u32);
    }
    unsafe {
        core::arch::asm!(
            "str {v}, [{a}]",
            v = in(reg) mask,
            a = in(reg) GPIO_OUT_W1TC,
            options(nostack, preserves_flags),
        );
    }
}

/// Publie la mort du cœur de contrôle pour la boucle d'affichage.
///
/// Écrit dans `SHARED_STATE` sous section critique, comme tout le monde —
/// le spinlock n'a pas de raison d'être tenu par qui que ce soit au moment
/// d'une panique, et le cœur 0 ne lit cette case qu'en début de tour.
/// Best-effort assumé : si le verrou était déjà perdu, l'écran restera
/// trompeur, mais les sorties sont déjà coupées — c'est l'ordre des deux
/// opérations dans le gestionnaire qui garantit la hiérarchie.
pub fn mark_core1_panicked() {
    critical_section::with(|cs| {
        crate::shared::data::SHARED_STATE.borrow_ref_mut(cs).core1_panicked = true;
    });
}
