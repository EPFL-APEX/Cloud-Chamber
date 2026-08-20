//! Sanity check de bring-up : scanne le bus I²C, initialise le BME280
//! (température, pression, humidité) et journalise une mesure par seconde.
//!
//! Broches tirées directement de `config::wiring` (`PIN_I2C_SDA`,
//! `PIN_I2C_SCL`), sélectionnées via `gpio::new_pin`/`DynPinId` plutôt que
//! par l'API typée `pins.gpio<N>` — même raisonnement que les autres bins
//! de bring-up : pas de champ littéral à garder synchronisé à la main avec
//! ces constantes.
//!
//! # Un piège spécifique à l'I²C
//!
//! Contrairement à un GPIO simple, SDA et SCL ne sont pas interchangeables
//! au niveau matériel : chaque broche du RP2040 est câblée en dur sur le
//! bus I²C0 *ou* I²C1, en position SDA *ou* SCL, jamais les deux (table
//! fixe du datasheet). `rp2040-hal` encode cette contrainte et fournit
//! `ValidatedPinSda`/`ValidatedPinScl::validate(pin, &peripherique)` pour la
//! vérifier à l'exécution même quand la broche n'est connue qu'au runtime
//! (notre cas, puisqu'elle vient d'une constante `config::wiring`) : si la
//! broche ne correspond pas au bon rôle pour le bon bus, `validate` retourne
//! `Err` plutôt que de configurer silencieusement n'importe quoi — utilisé
//! ci-dessous, avec un panic explicite au lieu d'un `.unwrap()` nu pour
//! nommer le problème si jamais SDA/SCL sont inversées dans la config.
//!
//! # Scan avant init
//!
//! `Bme280Driver` code l'adresse `0x76` en dur (constante privée du
//! driver, non paramétrable). Beaucoup de breakouts BME280 répondent en
//! réalité sur `0x77` (broche SDO tirée au VCC plutôt qu'au GND — ça
//! dépend du câblage/du module précis). Avant de tenter l'init, ce bin
//! scanne tout le bus (0x08–0x77) et journalise chaque adresse qui
//! répond, pour lever le doute sans avoir à deviner.
//!
//! RP2040 uniquement pour l'instant, même limite que les autres bins de
//! bring-up. Derrière la feature `bin-bme-test` (désactivée par défaut)
//! pour ne pas être construit par les jobs CI `cargo check` sur les autres
//! cibles :
//!
//! ```text
//! cargo run --target thumbv6m-none-eabi --features bin-bme-test \
//!     --bin bme_test
//! ```
#![no_std]
#![no_main]

use defmt_rtt as _;
use panic_probe as _;

use embedded_hal::delay::DelayNs;
use embedded_hal::i2c::I2c as I2cTrait;
use rp2040_hal::{
    Clock, I2C, self as hal, Sio, Watchdog,
    clocks::init_clocks_and_plls,
    fugit::RateExtU32,
    gpio::{DynBankId, DynPinId, FunctionI2c, Pins, PullUp, new_pin},
    i2c::{ValidatedPinScl, ValidatedPinSda},
    pac,
};

use cloud_chamber_firmware::config::wiring::{PIN_I2C_SCL, PIN_I2C_SDA};
use cloud_chamber_firmware::drivers::bme280::Bme280Driver;

/// Fréquence du cristal externe du Pico — cf. `hal::clocks::init_clocks_and_plls`.
const XOSC_CRYSTAL_FREQ: u32 = 12_000_000;

#[unsafe(link_section = ".boot2")]
#[used]
static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

/// Scanne les adresses I²C 7 bits usuelles (0x08–0x77, plage hors adresses
/// réservées) et journalise celles qui répondent. Purement diagnostique —
/// ne modifie aucun registre du côté esclave.
///
/// Sonde par une lecture d'un octet plutôt qu'une écriture de longueur
/// nulle : `rp2040-hal` (0.12) rejette toute écriture vide *avant* de
/// toucher au bus (`validate_buffer` refuse un buffer sans le moindre
/// octet à transmettre, avec `Error::InvalidWriteBufferLength`) — donc
/// `i2c.write(addr, &[])` renvoie systématiquement `Err`, sur chaque
/// adresse, sans jamais générer le moindre START/STOP réel. Un octet lu
/// (et jeté) déclenche bien une transaction complète, et l'ACK/NACK sur
/// l'octet d'adresse suffit à détecter la présence d'un périphérique,
/// indépendamment de ce que contient le registre lu.
fn scan_bus<I: I2cTrait>(i2c: &mut I) {
    defmt::info!("scan I2C (0x08-0x77)...");
    let mut found = 0u8;
    let mut probe = [0u8; 1];
    for addr in 0x08u8..=0x77 {
        if i2c.read(addr, &mut probe).is_ok() {
            defmt::info!("  adresse 0x{:02x} repond", addr);
            found += 1;
        }
    }
    if found == 0 {
        defmt::warn!("aucune adresse ne repond — verifier cablage/pull-up/alimentation");
    } else {
        defmt::info!("{} adresse(s) trouvee(s)", found);
    }
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

    // Safety : seule construction de `Pin` pour ces broches dans le
    // programme (le champ typé correspondant de `_pins` n'est ni lu ni
    // écrit) — même raisonnement que dans les autres bins de bring-up.
    let sda = unsafe { new_pin(DynPinId { bank: DynBankId::Bank0, num: PIN_I2C_SDA }) }
        .try_into_function::<FunctionI2c>()
        .ok()
        .expect("I2C est une fonction valide sur toute broche de Bank0")
        .into_pull_type::<PullUp>();
    let scl = unsafe { new_pin(DynPinId { bank: DynBankId::Bank0, num: PIN_I2C_SCL }) }
        .try_into_function::<FunctionI2c>()
        .ok()
        .expect("I2C est une fonction valide sur toute broche de Bank0")
        .into_pull_type::<PullUp>();

    // Vérifie que PIN_I2C_SDA/PIN_I2C_SCL correspondent vraiment aux rôles
    // SDA/SCL câblés en dur pour I2C0 sur ce silicium — cf. doc de module.
    let sda = ValidatedPinSda::validate(sda, &pac.I2C0).unwrap_or_else(|_| {
        panic!(
            "PIN_I2C_SDA (GP{}) n'est pas une broche SDA valide pour I2C0 — SDA/SCL sont peut-etre inversees dans config::wiring",
            PIN_I2C_SDA
        )
    });
    let scl = ValidatedPinScl::validate(scl, &pac.I2C0).unwrap_or_else(|_| {
        panic!(
            "PIN_I2C_SCL (GP{}) n'est pas une broche SCL valide pour I2C0 — SDA/SCL sont peut-etre inversees dans config::wiring",
            PIN_I2C_SCL
        )
    });

    // `new_controller` (pas le raccourci `I2C::i2c0`, qui exige `AnyPin` en
    // plus de `ValidPinSda`/`ValidPinScl` — incompatible avec le wrapper
    // `ValidatedPinSda`/`ValidatedPinScl` utilisé ci-dessus) : seules ces
    // deux bornes sont requises, exactement ce que la validation runtime
    // fournit.
    let mut i2c = I2C::new_controller(
        pac.I2C0,
        sda,
        scl,
        400.kHz(),
        &mut pac.RESETS,
        clocks.system_clock.freq(),
    );

    defmt::info!("bme_test demarre — SDA=GP{} SCL=GP{}", PIN_I2C_SDA, PIN_I2C_SCL);

    scan_bus(&mut i2c);

    let mut bme = Bme280Driver::new(i2c);

    match bme.init() {
        Ok(()) => defmt::info!("BME280 initialise (chip id OK)"),
        Err(e) => {
            defmt::error!("echec init BME280 (adresse 0x76 codee en dur dans le driver) : {}", defmt::Debug2Format(&e));
            loop {
                timer.delay_ms(1_000);
            }
        }
    }

    loop {
        match bme.measure(&mut timer) {
            Ok((temp_c, press_hpa, hum_pct)) => defmt::info!(
                "temperature={} C  pression={} hPa  humidite={} %",
                temp_c,
                press_hpa,
                hum_pct
            ),
            Err(e) => defmt::warn!("lecture invalide : {}", defmt::Debug2Format(&e)),
        }
        timer.delay_ms(1_000);
    }
}
