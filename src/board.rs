//! Mise en route de la carte : horloges, timer, GPIO — et les quatre façons
//! de configurer une broche dont tout le firmware se sert.
//!
//! # Pourquoi ce module existe
//!
//! Les sept binaires de bring-up (`src/bin/`) et `main.rs` ouvraient tous
//! sur le même prologue recopié : `Peripherals::take`, `Watchdog::new`,
//! `init_clocks_and_plls` avec exactement les mêmes arguments, `Timer::new`,
//! `Sio::new`, `Pins::new`. Et quatre d'entre eux portaient leur propre
//! copie de `configure_output_pin`, trois de `configure_input_pin`, deux de
//! `configure_relay_pin`.
//!
//! Ce n'est pas qu'une question de volume. Une de ces copies a déjà
//! introduit un vrai bug : `configure_output_pin` a été corrigée dans
//! `main.rs` et pas dans `relay_test.rs`, qui a cessé de compiler sans que
//! personne ne s'en aperçoive — le binaire flashé restait celui d'avant, et
//! la force de commande à 8 mA attendue sur les optocoupleurs n'était
//! jamais écrite. Une seule définition supprime la classe entière de ce
//! problème.
//!
//! # Utilisation
//!
//! ```text
//! let mut board = board::init();
//! let mut relay = board::configure_relay_pin(PIN_HV_RELAY);
//! board.timer.delay_ms(500);
//! ```
//!
//! [`Board`] rend aussi les périphériques que la mise en route n'a pas
//! consommés mais dont les appelants ont besoin ensuite (`resets`, `spi0`,
//! `i2c0`, `psm`, `ppb`, `fifo`). La liste est volontairement celle des
//! usages réels d'aujourd'hui, pas tout `pac::Peripherals` : un binaire qui
//! aurait besoin de PIO ajoute son champ ici, explicitement.
//!
//! # `BOOT2`
//!
//! La seconde étape du bootloader vit ici, une seule fois. Elle n'est
//! référencée par aucun code — c'est `KEEP(*(.boot2))` dans `rp2040.x` qui
//! la retient, et l'objet de ce module est bien tiré de l'archive puisque
//! chaque binaire appelle [`init`]. Vérifié sur l'ELF produit : section
//! `.boot2` de 256 octets à `0x10000000`, table CRC en fin de bloc.
//!
//! RP2040 uniquement — le module est derrière `#[cfg(all(rp2040,
//! target_arch = "arm"))]` dans `lib.rs`, comme les implémentations
//! matérielles de `cloud_chamber_hal::timer`.

use rp2040_hal::{
    self as hal, Sio, Watchdog,
    clocks::{ClocksManager, init_clocks_and_plls},
    gpio::{
        DynBankId, DynPinId, DynPullType, FunctionSio, OutputDriveStrength, Pin, Pins, SioInput,
        SioOutput, new_pin,
    },
    pac,
    sio::SioFifo,
};

use embedded_hal::digital::OutputPin;

/// Fréquence du cristal externe du Pico — cf. `hal::clocks::init_clocks_and_plls`.
pub const XOSC_CRYSTAL_FREQ: u32 = 12_000_000;

/// Seconde étape du bootloader, en tête de flash. Cf. la doc de module.
#[unsafe(link_section = ".boot2")]
#[used]
static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

/// Force de commande des sorties pilotant les optocoupleurs MOC3043.
///
/// **Le RP2040 démarre à 4 mA** (champ `DRIVE` de `PADS_BANK0.GPIO`, valeur
/// de reset `0x56`), ce qui est insuffisant ici : il faut au moins 5 mA
/// dans la LED du MOC3043 pour garantir l'amorçage (`IFT` max), et on vise
/// en pratique ~10 mA de marge.
///
/// Attention au sens de ce réglage : ce n'est pas une limite de courant,
/// c'est la capacité de la sortie. Le courant réel est fixé par la
/// résistance série ; ce champ décide seulement à partir de quel courant la
/// tension de sortie s'effondre. À 4 mA, tirer ~10 mA fait chuter `VOH`
/// assez pour que l'amorçage devienne aléatoire — d'où ce passage à 8 mA.
///
/// Si la résistance série vise franchement plus de 8 mA, passer à
/// [`OutputDriveStrength::TwelveMilliAmps`] : c'est le seul changement à
/// faire, toutes les sorties de puissance passent par cette constante.
pub const RELAY_DRIVE_STRENGTH: OutputDriveStrength = OutputDriveStrength::EightMilliAmps;

/// Ce que [`init`] rend à l'appelant.
///
/// `watchdog` est là bien que personne ne le réalimente aujourd'hui : il est
/// consommé par `init_clocks_and_plls`, donc sans ce champ il serait
/// définitivement perdu, et le jour où la boucle de contrôle voudra un chien
/// de garde il n'y aurait plus de moyen d'y accéder.
pub struct Board {
    pub clocks: ClocksManager,
    /// `Copy` : la même horloge peut servir de source monotone à la boucle
    /// de contrôle, d'horodatage aux mesures et de source de délai au
    /// bit-banging 1-Wire, sans mutex.
    pub timer: hal::Timer,
    pub watchdog: Watchdog,
    /// API typée des broches. Seul `blinky` s'en sert (`pins.gpio25`) ; les
    /// autres passent par les `configure_*_pin` ci-dessous, mais la valeur
    /// doit rester vivante — c'est `Pins::new` qui sort IO_BANK0 et
    /// PADS_BANK0 de reset.
    pub pins: Pins,
    /// File inter-cœurs du SIO, pour `Multicore::new`.
    pub fifo: SioFifo,
    pub resets: pac::RESETS,
    pub spi0: pac::SPI0,
    pub i2c0: pac::I2C0,
    pub psm: pac::PSM,
    pub ppb: pac::PPB,
}

/// Prend les périphériques, démarre les horloges sur le cristal 12 MHz,
/// arme le timer et sort les GPIO de reset.
///
/// # Panics
///
/// Panique si les périphériques ont déjà été pris (deux appels), ou si les
/// PLL ne verrouillent pas. Les deux sont des défaillances de démarrage : il
/// n'y a rien à faire d'utile ensuite, et `panic_probe` les rend visibles
/// sur la sonde.
pub fn init() -> Board {
    let mut pac = pac::Peripherals::take().expect("les peripheriques ne sont pris qu'une fois");
    let mut watchdog = Watchdog::new(pac.WATCHDOG);

    let clocks = init_clocks_and_plls(
        XOSC_CRYSTAL_FREQ,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .expect("les PLL doivent verrouiller sur le cristal 12 MHz");

    let timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

    let sio = Sio::new(pac.SIO);
    let pins = Pins::new(pac.IO_BANK0, pac.PADS_BANK0, sio.gpio_bank0, &mut pac.RESETS);

    Board {
        clocks,
        timer,
        watchdog,
        pins,
        fifo: sio.fifo,
        resets: pac.RESETS,
        spi0: pac.SPI0,
        i2c0: pac.I2C0,
        psm: pac.PSM,
        ppb: pac.PPB,
    }
}

// ─── Configuration des broches ───────────────────────────────────────────────
//
// Toutes passent par `gpio::new_pin`/`DynPinId` (API dynamique de
// `rp2040-hal`) plutôt que par l'API typée `pins.gpio<N>` : les numéros
// viennent de `config::wiring`, et un champ littéral `pins.gpio15` serait à
// resynchroniser à la main à chaque changement de câblage — sans que rien ne
// le signale. C'est déjà arrivé sur ce projet.
//
// # Safety (commune aux quatre)
//
// `new_pin` exige qu'aucune autre instance de `Pin` pour cette broche
// n'existe en parallèle. `Board::pins` réserve bien un champ typé
// `pins.gpio<N>` pour chaque numéro, mais aucun n'est lu ni écrit — sauf
// `gpio25` dans `blinky`, qui ne configure aucune broche de la chambre.
// Aucun accès concurrent réel aux registres n'en résulte. L'unicité des
// numéros est elle-même garantie à la compilation par `config::wiring`.

/// Configure GP`pin` en sortie push-pull logicielle, démarrée à l'état bas.
///
/// Garde la force de commande par défaut (4 mA). Convient aux broches
/// CS/DC/RESET de l'écran, qui n'attaquent que des entrées CMOS : y
/// augmenter la force ne servirait à rien et aggraverait les rebonds et le
/// rayonnement sur des signaux voisins d'un bus SPI à 32 MHz. Les sorties de
/// puissance passent par [`configure_relay_pin`].
pub fn configure_output_pin(pin: u8) -> Pin<DynPinId, FunctionSio<SioOutput>, DynPullType> {
    let id = DynPinId { bank: DynBankId::Bank0, num: pin };
    let raw = unsafe { new_pin(id) };

    let mut out = raw
        .try_into_function::<FunctionSio<SioOutput>>()
        .expect("SIO est une fonction valide sur toute broche de Bank0");
    out.set_pull_type(DynPullType::None);
    let _ = out.set_low();
    out
}

/// Configure GP`pin` en sortie de puissance : comme [`configure_output_pin`],
/// mais à [`RELAY_DRIVE_STRENGTH`] au lieu des 4 mA par défaut du RP2040 —
/// cf. la doc de cette constante pour le pourquoi (amorçage des MOC3043).
pub fn configure_relay_pin(pin: u8) -> Pin<DynPinId, FunctionSio<SioOutput>, DynPullType> {
    let mut out = configure_output_pin(pin);
    out.set_drive_strength(RELAY_DRIVE_STRENGTH);

    // Relecture du registre : le seul moyen de vérifier sur la puce réelle
    // que le champ `DRIVE` a bien pris, plutôt que de le supposer. Un
    // `warn` plutôt qu'un `panic` — une force de commande inattendue rend
    // l'amorçage douteux, pas le démarrage impossible, et l'opérateur doit
    // pouvoir voir l'anomalie plutôt que de se retrouver devant une carte
    // muette.
    let readback = out.get_drive_strength();
    if readback == RELAY_DRIVE_STRENGTH {
        defmt::debug!("GP{} : force de commande {}", pin, defmt::Debug2Format(&readback));
    } else {
        defmt::warn!(
            "GP{} : force de commande {} au lieu de {} — amorcage MOC3043 incertain",
            pin,
            defmt::Debug2Format(&readback),
            defmt::Debug2Format(&RELAY_DRIVE_STRENGTH),
        );
    }

    out
}

/// Configure GP`pin` en entrée avec pull-up interne (broches encodeur).
pub fn configure_input_pin(pin: u8) -> Pin<DynPinId, FunctionSio<SioInput>, DynPullType> {
    let id = DynPinId { bank: DynBankId::Bank0, num: pin };
    let raw = unsafe { new_pin(id) };

    let mut in_pin = raw
        .try_into_function::<FunctionSio<SioInput>>()
        .expect("SIO est une fonction valide sur toute broche de Bank0");
    in_pin.set_pull_type(DynPullType::Up);
    in_pin
}

/// Prépare GP`pin` pour l'usage 1-Wire : sortie niveau bas, puis entrée
/// flottante — c'est ensuite `Rp2040OpenDrain` qui pilote la direction
/// directement par les registres du SIO. Le pull-up 4.7 kΩ est externe.
///
/// La `Pin` typée est abandonnée à la fin, seule la configuration matérielle
/// persiste.
pub fn configure_onewire_pin(pin: u8) {
    let id = DynPinId { bank: DynBankId::Bank0, num: pin };
    let raw = unsafe { new_pin(id) };

    let mut out = raw
        .try_into_function::<FunctionSio<SioOutput>>()
        .expect("SIO est une fonction valide sur toute broche de Bank0");
    out.set_pull_type(DynPullType::None);
    let _ = out.set_low();

    let _floating = out
        .try_into_function::<FunctionSio<SioInput>>()
        .expect("SIO est une fonction valide sur toute broche de Bank0");
}
