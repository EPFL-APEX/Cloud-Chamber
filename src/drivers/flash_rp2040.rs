//! [`FlashOps`] sur la flash QSPI du RP2040, via les routines de la ROM.
//!
//! # ⚠ Ce fichier n'a pas été validé sur carte
//!
//! Il compile, il linke, et `scripts/check_ram_funcs.py` confirme sur l'ELF
//! — en debug comme en release — qu'aucune des routines résidentes ne saute
//! vers la flash. Mais **personne ne l'a encore fait tourner sur un Pico**.
//! Tant que ce paragraphe est là, traitez-le comme du code à valider au
//! banc, pas comme un acquis : une erreur ici ne donne pas un test rouge,
//! elle donne une carte qui ne redémarre plus.
//!
//! Après toute modification de ce fichier :
//!
//! ```text
//! cargo build --target thumbv6m-none-eabi --features bin-cloud-chamber --bin cloud_chamber
//! python3 scripts/check_ram_funcs.py target/thumbv6m-none-eabi/debug/cloud_chamber
//! ```
//!
//! Ce n'est pas une précaution théorique : deux versions successives de ce
//! module ont échoué à cette vérification, d'abord sur
//! `cortex_m::interrupt::free` et sa fermeture, puis sur
//! `core::ptr::read_volatile` non inliné en debug. Les deux compilaient sans
//! le moindre avertissement.
//!
//! # Pourquoi c'est délicat
//!
//! Le RP2040 n'a pas de flash interne : le code vit dans la puce QSPI
//! soudée à côté et s'exécute en place (XIP). Pour écrire dedans, les
//! routines ROM sortent la QSPI du mode XIP et lui parlent en commandes
//! brutes. Pendant cette fenêtre, **aucune instruction en flash ne peut être
//! lue, sur aucun des deux cœurs**. D'où trois contraintes, toutes
//! obligatoires :
//!
//! 1. la fonction qui exécute la séquence doit être **en RAM** —
//!    `#[unsafe(link_section = ".data.ram_func")]` la place dans `.data`,
//!    que `cortex-m-rt` recopie en RAM au démarrage, et `#[inline(never)]`
//!    l'empêche d'être fondue dans un appelant resté en flash ;
//! 2. elle ne doit **appeler aucune fonction restée en flash** — d'où les
//!    accès registres écrits à la main plutôt que par l'API du HAL, et les
//!    pointeurs de fonction ROM lus *avant* de couper le XIP ;
//! 3. **aucune interruption** ne doit se déclencher pendant ce temps, sur
//!    aucun cœur : leurs vecteurs pointent dans la flash.
//!
//! # Le second cœur
//!
//! `critical_section::with` coupe les interruptions du cœur courant. Il
//! **n'arrête pas l'autre cœur**, qui continuerait d'exécuter la boucle de
//! contrôle depuis la flash — hard fault immédiat.
//!
//! D'où [`CoreLockout`] : le cœur 0 demande par la FIFO inter-cœurs, le
//! cœur 1 saute dans une boucle d'attente **elle aussi en RAM**
//! ([`park_until_resumed`]) où il tourne en rond interruptions coupées, et
//! n'en sort qu'au mot de reprise. C'est `flash_safe_execute` du SDK C,
//! réécrit ici parce que `rp2040-hal` ne le fournit pas.
//!
//! Le cœur 1 ne « dort » pas : il exécute activement une boucle vide. Il ne
//! sonde plus les capteurs, ne fait plus tourner `tick()`, ne surveille plus
//! rien. C'est pourquoi `logic::persistence` n'autorise l'effacement — la
//! seule opération assez longue pour que ça compte — qu'à l'arrêt.
//!
//! # Procédure de validation au banc
//!
//! 1. `cargo run --features bin-cloud-chamber --bin cloud_chamber`, puis
//!    vérifier dans les traces `defmt` la ligne `secteur reglages a
//!    0x1ff000` au démarrage.
//! 2. Modifier un réglage à l'écran, sauvegarder. Attendre la trace
//!    `reglages sauvegardes`.
//! 3. **Couper l'alimentation**, rallumer : le réglage modifié doit être
//!    relu au démarrage (`reglages relus depuis la flash`).
//! 4. Répéter 16 fois, machine à l'arrêt, pour provoquer l'effacement de
//!    secteur. La 16e sauvegarde doit prendre visiblement plus longtemps
//!    (dizaines à centaines de millisecondes) et les réglages doivent
//!    toujours se relire après coupure.
//! 5. Relancer un cycle, puis sauvegarder jusqu'à retomber sur un secteur
//!    plein : la demande doit être **différée**, l'écran de réglages
//!    afficher « en attente » sur la ligne *Save to flash*, et l'écriture
//!    ne se produire qu'au retour à l'arrêt.
//! 6. Vérifier qu'aucune de ces opérations n'a perturbé la boucle de
//!    contrôle : les horodatages de mesure ne doivent pas montrer de trou
//!    supérieur à quelques millisecondes hors effacement.

use rp2040_hal::{rom_data, sio::SioFifo};

use crate::config::settings::StoreError;
use crate::drivers::flash_store::{FlashOps, PAGE_SIZE, SECTOR_SIZE};

// ─── Position du secteur ─────────────────────────────────────────────────────

unsafe extern "C" {
    /// Décalage du secteur des réglages depuis le début de la flash, posé
    /// par `rp2040.x`. C'est l'*adresse* du symbole qui porte la valeur,
    /// comme pour tout symbole de script de link.
    static __settings_flash_offset: u8;
    static __settings_flash_len: u8;
}

/// Décalage du secteur des réglages, tel que le script de link l'a réservé.
///
/// Ne vient pas d'une constante Rust exprès : seul `rp2040.x` connaît la
/// taille de flash de la carte, et c'est lui qui garantit qu'aucune section
/// n'atterrit dans ce secteur.
pub fn settings_offset() -> u32 {
    core::ptr::addr_of!(__settings_flash_offset) as u32
}

/// Taille réservée, pour vérifier au démarrage qu'elle vaut bien un secteur.
pub fn settings_len() -> u32 {
    core::ptr::addr_of!(__settings_flash_len) as u32
}

/// Base de la fenêtre XIP : c'est par là qu'on *lit* la flash, alors que les
/// routines ROM veulent des décalages depuis 0.
const XIP_BASE: u32 = 0x1000_0000;

// ─── Verrou inter-cœurs ──────────────────────────────────────────────────────

/// Mots échangés sur la FIFO inter-cœurs. Valeurs choisies improbables : un
/// mot inconnu est ignoré des deux côtés, donc un reliquat d'un échange
/// précédent ne peut pas être pris pour un ordre.
const REQUEST_PARK: u32 = 0x5041_524B; // "PARK"
const ACK_PARKED: u32 = 0x4F4B_4159; // "OKAY"
const RESUME: u32 = 0x474F_4F4E; // "GOON"

/// Registres SIO touchés depuis la boucle d'attente en RAM. Accès bruts :
/// passer par `SioFifo` appellerait du code resté en flash.
const SIO_BASE: u32 = 0xd000_0000;
const FIFO_ST: u32 = SIO_BASE + 0x50;
const FIFO_WR: u32 = SIO_BASE + 0x54;
const FIFO_RD: u32 = SIO_BASE + 0x58;
/// `FIFO_ST` bit 0 : des données sont disponibles en lecture.
const FIFO_ST_VLD: u32 = 1 << 0;
/// `FIFO_ST` bit 1 : il y a de la place en écriture.
const FIFO_ST_RDY: u32 = 1 << 1;

/// Lecture d'un registre 32 bits, garantie sans appel.
///
/// `core::ptr::read_volatile` ferait l'affaire ailleurs, mais en build
/// debug il n'est pas inliné : il devient un appel vers la flash, ce qui est
/// fatal dans la boucle d'attente. Vu dans l'ELF sous forme de
/// `__Thumbv6MABSLongThunk__ZN4core3ptr13read_volatile`. `asm!` ne peut pas
/// devenir un appel, par construction.
///
/// # Safety
/// `addr` doit être une adresse MMIO alignée sur 4.
#[inline(always)]
unsafe fn mmio_read(addr: u32) -> u32 {
    let value: u32;
    unsafe {
        core::arch::asm!("ldr {v}, [{a}]", v = out(reg) value, a = in(reg) addr,
                         options(nostack, preserves_flags));
    }
    value
}

/// Écriture d'un registre 32 bits, même raison que [`mmio_read`].
///
/// # Safety
/// `addr` doit être une adresse MMIO alignée sur 4.
#[inline(always)]
unsafe fn mmio_write(addr: u32, value: u32) {
    unsafe {
        core::arch::asm!("str {v}, [{a}]", v = in(reg) value, a = in(reg) addr,
                         options(nostack, preserves_flags));
    }
}

/// Côté cœur 1 : se gare si le cœur 0 le demande, sinon rend la main tout de
/// suite.
///
/// À appeler une fois par tour de boucle de contrôle. Le coût quand rien
/// n'est demandé est une lecture de registre.
///
/// # Safety
///
/// Ne doit être appelée que depuis le cœur 1, et hors de toute section
/// critique : la boucle d'attente coupe les interruptions et ne rend la main
/// qu'au mot de reprise.
pub fn park_if_requested() {
    // Lecture non destructive de l'état ; on ne dépile que si c'est bien
    // l'ordre attendu.
    let st = unsafe { mmio_read(FIFO_ST) };
    if st & FIFO_ST_VLD == 0 {
        return;
    }
    let word = unsafe { mmio_read(FIFO_RD) };
    if word != REQUEST_PARK {
        // Reliquat d'un échange précédent — sans effet.
        return;
    }
    park_until_resumed();
}

/// La boucle d'attente proprement dite. **Doit rester en RAM** : elle
/// s'exécute pendant que le XIP est coupé.
///
/// # Pourquoi tout est écrit à la main ici
///
/// Pas de `cortex_m::interrupt::free`, pas de fermeture, pas le moindre
/// appel. Une première version utilisait `interrupt::free(|_| …)` : la
/// fonction générique et la fermeture sont restées en flash, et la fonction
/// « en RAM » se réduisait à un tremplin de 10 octets sautant dehors. Ça se
/// voit dans l'ELF, pas à la relecture — d'où la vérification de placement
/// décrite dans la doc de module, à refaire après toute modification ici.
///
/// Les interruptions sont donc coupées par `cpsid i` directement, et la FIFO
/// lue par accès volatils : rien qui puisse devenir un appel.
#[unsafe(link_section = ".data.ram_func")]
#[inline(never)]
fn park_until_resumed() {
    unsafe {
        core::arch::asm!("cpsid i", options(nomem, nostack, preserves_flags));

        // Accuse réception : à partir de là, le cœur 0 se croit autorisé à
        // couper le XIP. Plus rien ici ne doit toucher la flash.
        while mmio_read(FIFO_ST) & FIFO_ST_RDY == 0 {}
        mmio_write(FIFO_WR, ACK_PARKED);

        // `wfe`/`sev` seraient plus économes, mais une boucle nue est plus
        // simple à relire et on parle de quelques centaines de
        // millisecondes au pire, une fois toutes les seize sauvegardes.
        loop {
            if mmio_read(FIFO_ST) & FIFO_ST_VLD != 0 && mmio_read(FIFO_RD) == RESUME {
                break;
            }
        }

        core::arch::asm!("cpsie i", options(nomem, nostack, preserves_flags));
    }
}

/// Côté cœur 0 : gare le cœur 1 le temps d'une opération flash, puis le
/// libère.
pub struct CoreLockout {
    fifo: SioFifo,
}

impl CoreLockout {
    /// `fifo` doit être celle du cœur 0, après le lancement du cœur 1.
    pub fn new(fifo: SioFifo) -> Self {
        Self { fifo }
    }

    /// Demande au cœur 1 de se garer et attend son accusé de réception.
    ///
    /// `spins` borne l'attente. Si le cœur 1 ne répond pas — il peut être en
    /// plein délai de conversion DS18B20, jusqu'à ~800 ms — on **renonce à
    /// écrire** plutôt que de couper le XIP sous ses pieds. Un réglage non
    /// sauvegardé est un désagrément ; un hard fault sur la boucle de
    /// sécurité, non.
    fn park_other_core(&mut self, spins: u32) -> Result<(), StoreError> {
        while !self.fifo.is_write_ready() {}
        self.fifo.write(REQUEST_PARK);

        for _ in 0..spins {
            if let Some(ACK_PARKED) = self.fifo.read() {
                return Ok(());
            }
        }
        // Le cœur 1 n'a pas répondu. Il peut encore se garer d'ici peu, sur
        // l'ordre qui traîne dans la FIFO : on lui envoie la reprise pour
        // qu'il n'y reste pas. S'il ne s'est jamais garé, ce mot sera lu et
        // ignoré au tour suivant.
        while !self.fifo.is_write_ready() {}
        self.fifo.write(RESUME);
        Err(StoreError::Write)
    }

    fn resume_other_core(&mut self) {
        while !self.fifo.is_write_ready() {}
        self.fifo.write(RESUME);
    }
}

// ─── Séquence ROM ────────────────────────────────────────────────────────────

/// Ce que la séquence doit faire une fois le XIP coupé.
#[derive(Clone, Copy)]
enum FlashOp {
    /// Efface `SECTOR_SIZE` octets à partir de `offset`.
    Erase { offset: u32 },
    /// Programme une page à `offset` depuis `data`.
    Program { offset: u32, data: *const u8 },
}

/// La séquence ROM complète, XIP coupé du début à la fin.
///
/// **Doit rester en RAM**, et n'appeler que les pointeurs ROM résolus par
/// l'appelant *avant* d'entrer ici : résoudre une fonction ROM lit une table
/// en ROM (pas en flash) mais passe par du code de `rom_data`, qui lui est en
/// flash.
///
/// # Safety
///
/// Le cœur 1 doit être garé, les interruptions du cœur 0 coupées, et
/// `offset` aligné comme l'opération l'exige.
#[unsafe(link_section = ".data.ram_func")]
#[inline(never)]
unsafe fn run_with_xip_off(op: FlashOp, rom: &RomFns) {
    unsafe {
        (rom.connect)();
        (rom.exit_xip)();
        match op {
            FlashOp::Erase { offset } => {
                // `block_size`/`block_cmd` : les valeurs du SDK C. Avec
                // `count` = 4 Ko, le chemin « gros bloc » ne peut pas se
                // déclencher (il demande count >= block_size), mais on passe
                // les mêmes valeurs que le code exercé par des millions de
                // cartes plutôt que des zéros qu'on n'a pas vérifiés.
                (rom.range_erase)(offset, SECTOR_SIZE, 1 << 16, 0xD8);
            }
            FlashOp::Program { offset, data } => {
                (rom.range_program)(offset, data, PAGE_SIZE);
            }
        }
        (rom.flush_cache)();
        (rom.enter_cmd_xip)();
    }
}

/// Pointeurs ROM résolus une fois pour toutes, hors fenêtre XIP.
struct RomFns {
    connect: unsafe extern "C" fn(),
    exit_xip: unsafe extern "C" fn(),
    range_erase: unsafe extern "C" fn(u32, usize, u32, u8),
    range_program: unsafe extern "C" fn(u32, *const u8, usize),
    flush_cache: unsafe extern "C" fn(),
    enter_cmd_xip: unsafe extern "C" fn(),
}

impl RomFns {
    fn resolve() -> Self {
        Self {
            connect: rom_data::connect_internal_flash::ptr(),
            exit_xip: rom_data::flash_exit_xip::ptr(),
            range_erase: rom_data::flash_range_erase::ptr(),
            range_program: rom_data::flash_range_program::ptr(),
            flush_cache: rom_data::flash_flush_cache::ptr(),
            enter_cmd_xip: rom_data::flash_enter_cmd_xip::ptr(),
        }
    }
}

// ─── Implémentation de `FlashOps` ────────────────────────────────────────────

/// Accès à la flash QSPI, avec mise en attente du second cœur.
pub struct Rp2040Flash {
    lockout: CoreLockout,
    /// Borne d'attente de l'accusé de réception du cœur 1. Exprimée en
    /// tours de boucle et non en millisecondes : on n'a pas d'horloge
    /// disponible ici, et l'ordre de grandeur suffit — il s'agit de
    /// distinguer « occupé quelques microsecondes » de « ne répond pas ».
    park_spins: u32,
}

/// ~1 s à 125 MHz, largement au-delà du plus long blocage attendu du cœur 1
/// (la conversion 12 bits d'un DS18B20, 750 ms).
pub const DEFAULT_PARK_SPINS: u32 = 20_000_000;

impl Rp2040Flash {
    pub fn new(fifo: SioFifo) -> Self {
        Self { lockout: CoreLockout::new(fifo), park_spins: DEFAULT_PARK_SPINS }
    }

    /// Gare le cœur 1, coupe les interruptions du cœur 0, exécute `op`,
    /// puis relâche les deux.
    fn with_flash_stopped(&mut self, op: FlashOp) -> Result<(), StoreError> {
        let rom = RomFns::resolve();
        self.lockout.park_other_core(self.park_spins)?;
        critical_section::with(|_| unsafe { run_with_xip_off(op, &rom) });
        self.lockout.resume_other_core();
        Ok(())
    }
}

impl FlashOps for Rp2040Flash {
    /// La lecture passe par le XIP : c'est un accès mémoire ordinaire, sans
    /// aucune des précautions ci-dessus.
    fn read(&self, offset: u32, buf: &mut [u8]) {
        let src = (XIP_BASE + offset) as *const u8;
        unsafe { core::ptr::copy_nonoverlapping(src, buf.as_mut_ptr(), buf.len()) };
    }

    fn erase_sector(&mut self, offset: u32) -> Result<(), StoreError> {
        debug_assert_eq!(offset as usize % SECTOR_SIZE, 0);
        self.with_flash_stopped(FlashOp::Erase { offset })
    }

    fn program_page(&mut self, offset: u32, page: &[u8; PAGE_SIZE]) -> Result<(), StoreError> {
        debug_assert_eq!(offset as usize % PAGE_SIZE, 0);
        self.with_flash_stopped(FlashOp::Program { offset, data: page.as_ptr() })
    }
}
