//! Sanity check de bring-up : bascule ensemble les trois relais haute
//! tension/compresseur/chauffage iso — allumés 3 s, éteints 3 s, en boucle
//! — pour vérifier le câblage et l'attaque des relais indépendamment du
//! reste de la logique de contrôle.
//!
//! Broches tirées directement de `config::wiring` (`PIN_COMPRESSOR_RELAY`,
//! `PIN_HV_RELAY`, `PIN_ISO_HEATER_RELAY`), sélectionnées via
//! `gpio::new_pin`/`DynPinId` (API dynamique de `rp2040-hal`) plutôt que
//! par l'API typée `pins.gpio<N>` : même raisonnement que
//! `configure_onewire_pin` dans `identify_temp_sensors`, pour ne jamais
//! avoir de champ littéral à garder synchronisé à la main avec ces
//! constantes.
//!
//! Suppose une sortie active à l'état haut (relais commandé "on" par
//! GPIO = 1) — à inverser ici si le module de relais utilisé est actif
//! bas.
//!
//! RP2040 uniquement pour l'instant, même limite que `identify_temp_sensors`
//! et `blinky`. Derrière la feature `bin-relay-test` (désactivée par
//! défaut) pour ne pas être construit par les jobs CI `cargo check` sur les
//! autres cibles :
//!
//! ```text
//! cargo run --target thumbv6m-none-eabi --features bin-relay-test \
//!     --bin relay_test
//! ```
#![no_std]
#![no_main]

use defmt_rtt as _;
use panic_probe as _;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use rp2040_hal::{
    self as hal, Sio, Watchdog,
    clocks::init_clocks_and_plls,
    gpio::{
        DynBankId, DynPinId, DynPullType, FunctionSio, OutputDriveStrength, Pin, Pins, SioOutput,
        new_pin,
    },
    pac,
};

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
const RELAY_DRIVE_STRENGTH: OutputDriveStrength = OutputDriveStrength::EightMilliAmps;

use cloud_chamber_firmware::config::wiring::{PIN_COMPRESSOR_RELAY, PIN_HV_RELAY, PIN_ISO_HEATER_RELAY, PIN_LIGHTS_RELAY, PIN_PUMP_RELAY};

/// Fréquence du cristal externe du Pico — cf. `hal::clocks::init_clocks_and_plls`.
const XOSC_CRYSTAL_FREQ: u32 = 12_000_000;

#[unsafe(link_section = ".boot2")]
#[used]
static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

const RELAY_PINS: [u8; 5] = [PIN_COMPRESSOR_RELAY, PIN_HV_RELAY, PIN_ISO_HEATER_RELAY, PIN_PUMP_RELAY, PIN_LIGHTS_RELAY];

/// Configure GP`pin` en sortie push-pull logicielle, démarrée à l'état bas
/// (relais éteint).
///
/// Garde la force de commande par défaut du RP2040 (4 mA) : les sorties de
/// puissance passent par [`configure_relay_pin`], qui l'élève ensuite à
/// [`RELAY_DRIVE_STRENGTH`].
///
/// Passe par `gpio::new_pin`/`DynPinId` plutôt que par l'API typée
/// `pins.gpio<N>` — cf. doc de module.
///
/// # Safety
/// `new_pin` exige qu'aucune autre instance de `Pin` pour cette broche
/// n'existe en parallèle. `Pins::new(...)` (appelé juste avant, pour ses
/// effets de bord de sortie de reset) réserve bien un champ typé
/// `pins.gpio<N>` pour ce même numéro, mais ce champ n'est ni lu ni écrit
/// nulle part dans ce fichier : aucun accès concurrent réel aux registres
/// n'en résulte.
fn configure_output_pin(pin: u8) -> Pin<DynPinId, FunctionSio<SioOutput>, DynPullType> {
    let id = DynPinId { bank: DynBankId::Bank0, num: pin };
    let raw = unsafe { new_pin(id) };

    let mut out = raw
        .try_into_function::<FunctionSio<SioOutput>>()
        .ok()
        .expect("SIO est une fonction valide sur toute broche de Bank0");
    out.set_pull_type(DynPullType::None);
    let _ = out.set_low();
    out
}

/// Configure GP`pin` en sortie de puissance : comme
/// [`configure_output_pin`], mais à [`RELAY_DRIVE_STRENGTH`] au lieu des
/// 4 mA par défaut du RP2040 — cf. la doc de cette constante pour le
/// pourquoi (amorçage des MOC3043).
///
/// # Safety
/// Même raisonnement que [`configure_output_pin`].
fn configure_relay_pin(pin: u8) -> Pin<DynPinId, FunctionSio<SioOutput>, DynPullType> {
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

#[hal::entry]
fn main() -> ! {
    let mut pac = pac::Peripherals::take().unwrap();
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
    .ok()
    .unwrap();

    let mut timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

    let sio = Sio::new(pac.SIO);
    // `Pins::new` reste nécessaire même si son API typée n'est pas utilisée
    // ensuite : c'est cet appel qui sort IO_BANK0/PADS_BANK0 de reset.
    let _pins = Pins::new(pac.IO_BANK0, pac.PADS_BANK0, sio.gpio_bank0, &mut pac.RESETS);

    let mut relays = RELAY_PINS.map(configure_relay_pin);

    defmt::info!(
        "relay_test demarre — GP{} (compresseur), GP{} (HV), GP{} (chauffage iso), GP{} (PUMP), GP{} (LIGHTS) 3s on / 3s off",
        PIN_COMPRESSOR_RELAY,
        PIN_HV_RELAY,
        PIN_ISO_HEATER_RELAY,
        PIN_PUMP_RELAY,
        PIN_LIGHTS_RELAY
    );

    let mut on = false;
    loop {
        on = !on;
        for relay in relays.iter_mut() {
            let _ = if on { relay.set_high() } else { relay.set_low() };
        }
        defmt::info!("relais = {}", on);
        timer.delay_ms(10_000);
    }
}
