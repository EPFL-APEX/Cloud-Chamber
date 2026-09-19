//! Sanity check de bring-up : initialise l'écran ILI9341 (SPI0, 320x240) et
//! affiche "Hello, world!" — vérifie le câblage SPI/DC/RESET/CS et
//! l'orientation avant de brancher un vrai écran UI dessus.
//!
//! Broches tirées directement de `config::wiring` (`PIN_SCREEN_SCK`,
//! `PIN_SCREEN_MOSI`, `PIN_SCREEN_CS`, `PIN_SCREEN_DC`,
//! `PIN_SCREEN_RESET`) — cf. `board` pour la mise en route et le choix de
//! l'API dynamique de `rp2040-hal`.
//!
//! # SCK/MOSI vs CS/DC/RESET : deux natures différentes
//!
//! Comme pour l'I²C, SCK et MOSI (Tx) sont câblées en dur sur le
//! périphérique SPI0 *ou* SPI1 du RP2040, dans un rôle précis (table fixe
//! du datasheet) — vérifié à l'exécution via
//! `ValidatedPinTx`/`ValidatedPinSck::validate(pin, &peripherique)`, avec
//! panic explicite si la validation échoue plutôt qu'un `.unwrap()` nu.
//!
//! CS, DC et RESET en revanche ne passent pas par le périphérique SPI
//! matériel : ce sont de simples broches GPIO pilotées en logiciel
//! (`embedded_hal_bus::spi::ExclusiveDevice` gère CS lui-même autour de
//! chaque transaction). N'importe quel GPIO convient, configuré comme les
//! sorties de `relay_test`.
//!
//! RP2040 uniquement pour l'instant, même limite que les autres bins de
//! bring-up. Derrière la feature `bin-screen-test` (désactivée par
//! défaut) pour ne pas être construit par les jobs CI `cargo check` sur
//! les autres cibles :
//!
//! ```text
//! cargo run --target thumbv6m-none-eabi --features bin-screen-test \
//!     --bin screen_test
//! ```
#![no_std]
#![no_main]

use defmt_rtt as _;
use panic_probe as _;

use embedded_hal::delay::DelayNs;
use embedded_hal::spi::MODE_0;
use rp2040_hal::{
    Clock, self as hal,
    fugit::RateExtU32,
    gpio::{DynBankId, DynPinId, FunctionSpi, new_pin},
    spi::{Spi, ValidatedPinSck, ValidatedPinTx},
};

use display_interface_spi::SPIInterface;
use embedded_hal_bus::spi::ExclusiveDevice;
use ili9341::{DisplaySize240x320, Ili9341, Orientation};

use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::*,
    text::Text,
};

use cloud_chamber_firmware::board;
use cloud_chamber_firmware::config::wiring::{
    PIN_SCREEN_CS, PIN_SCREEN_DC, PIN_SCREEN_MOSI, PIN_SCREEN_RESET, PIN_SCREEN_SCK,
};

#[hal::entry]
fn main() -> ! {
    let mut board = board::init();

    // Safety : seule construction de `Pin` pour ces broches dans le
    // programme (le champ typé correspondant de `_pins` n'est ni lu ni
    // écrit) — même raisonnement que dans les autres bins de bring-up.
    let tx = unsafe { new_pin(DynPinId { bank: DynBankId::Bank0, num: PIN_SCREEN_MOSI }) }
        .try_into_function::<FunctionSpi>()
        .ok()
        .expect("SPI est une fonction valide sur toute broche de Bank0");
    let sck = unsafe { new_pin(DynPinId { bank: DynBankId::Bank0, num: PIN_SCREEN_SCK }) }
        .try_into_function::<FunctionSpi>()
        .ok()
        .expect("SPI est une fonction valide sur toute broche de Bank0");

    // Vérifie que PIN_SCREEN_MOSI/PIN_SCREEN_SCK correspondent vraiment aux
    // rôles Tx/Sck câblés en dur pour SPI0 sur ce silicium — cf. doc de module.
    let tx = ValidatedPinTx::validate(tx, &board.spi0).unwrap_or_else(|_| {
        panic!(
            "PIN_SCREEN_MOSI (GP{}) n'est pas une broche Tx/MOSI valide pour SPI0",
            PIN_SCREEN_MOSI
        )
    });
    let sck = ValidatedPinSck::validate(sck, &board.spi0).unwrap_or_else(|_| {
        panic!(
            "PIN_SCREEN_SCK (GP{}) n'est pas une broche Sck valide pour SPI0",
            PIN_SCREEN_SCK
        )
    });

    // Turbofish DS=8 (taille de trame en bits) : plusieurs impls existent
    // (4/5/8...), rien ne force le choix sans cette annotation explicite.
    let spi = Spi::<_, _, _, 8>::new(board.spi0, (tx, sck)).init(
        &mut board.resets,
        board.clocks.peripheral_clock.freq(),
        16_000_000u32.Hz(),
        MODE_0,
    );

    let cs = board::configure_output_pin(PIN_SCREEN_CS);
    let dc = board::configure_output_pin(PIN_SCREEN_DC);
    let rst = board::configure_output_pin(PIN_SCREEN_RESET);

    // CS::Error = Infallible (broche GPIO simple) : ne peut pas échouer en pratique.
    let spi_device = ExclusiveDevice::new_no_delay(spi, cs).unwrap();
    let iface = SPIInterface::new(spi_device, dc);

    defmt::info!(
        "screen_test demarre — SCK=GP{} MOSI=GP{} CS=GP{} DC=GP{} RESET=GP{}",
        PIN_SCREEN_SCK,
        PIN_SCREEN_MOSI,
        PIN_SCREEN_CS,
        PIN_SCREEN_DC,
        PIN_SCREEN_RESET
    );

    let mut display = match Ili9341::new(
        iface,
        rst,
        &mut board.timer,
        Orientation::Landscape,
        DisplaySize240x320,
    ) {
        Ok(display) => display,
        Err(e) => {
            defmt::error!("echec init ecran : {}", defmt::Debug2Format(&e));
            loop {
                board.timer.delay_ms(1_000);
            }
        }
    };

    defmt::info!("ecran initialise, affichage de Hello, world!");

    if display.clear(Rgb565::BLACK).is_err() {
        defmt::error!("echec clear ecran");
    }

    let style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    if Text::new("Hello, world!", Point::new(20, 30), style).draw(&mut display).is_err() {
        defmt::error!("echec affichage texte");
    }

    // Battement de vie : confirme que le programme tourne toujours sans
    // redessiner l'écran (le contrôleur ILI9341 garde l'image en GRAM).
    loop {
        defmt::info!("screen_test vivant");
        board.timer.delay_ms(1_000);
    }
}
