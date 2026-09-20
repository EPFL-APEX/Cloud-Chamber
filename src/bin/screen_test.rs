//! Sanity check de bring-up : initialise l'écran ILI9341 et affiche
//! "Hello, world!" — vérifie le câblage SPI/DC/RESET/CS et l'orientation
//! avant de brancher une vraie UI dessus.
//!
//! # Il monte l'écran exactement comme le firmware
//!
//! Le montage passe par
//! [`ui::console::init_display`](cloud_chamber_firmware::ui::console::init_display),
//! la même fonction qu'appellent `main.rs` et `ui_test`. Ce n'était pas le
//! cas avant : ce bin portait sa propre copie et tournait à **16 MHz**
//! pendant que le firmware attaquait l'écran à 32 MHz. Il ne pouvait donc
//! pas, par construction, reproduire le bruit visuel qui est justement le
//! symptôme d'une fréquence trop haute — le bin censé valider l'écran
//! validait un autre écran que celui du firmware. Cf. la doc de module de
//! `console`.
//!
//! Conséquence visible ici : le texte passe par le framebuffer RAM et son
//! transfert par bandes, comme tout le reste du firmware. C'est le chemin
//! qu'on veut valider, pas un chemin direct qui n'existe nulle part
//! ailleurs.
//!
//! # SCK/MOSI vs CS/DC/RESET : deux natures différentes
//!
//! SCK et MOSI (Tx) sont câblées en dur sur SPI0 *ou* SPI1 dans un rôle
//! précis (table fixe du datasheet) — `console::init_display` le vérifie à
//! l'exécution via `ValidatedPinTx`/`ValidatedPinSck` et panique
//! explicitement si le câblage de `config::wiring` ne correspond pas.
//!
//! CS, DC et RESET, elles, ne passent pas par le périphérique SPI : ce sont
//! de simples GPIO pilotés en logiciel (`ExclusiveDevice` gère CS autour de
//! chaque transaction). N'importe quel GPIO convient.
//!
//! RP2040 uniquement, derrière la feature `bin-screen-test` (désactivée par
//! défaut) pour ne pas être construit par les jobs CI `cargo check` sur les
//! autres cibles :
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
use rp2040_hal::{Clock, self as hal};

use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::*,
    text::Text,
};

use cloud_chamber_firmware::board;
use cloud_chamber_firmware::ui::console;

#[hal::entry]
fn main() -> ! {
    let mut board = board::init();

    let mut display = console::init_display(
        board.spi0,
        &mut board.resets,
        board.clocks.peripheral_clock.freq(),
        &mut board.timer,
    );

    defmt::info!("screen_test : ecran initialise, affichage de Hello, world!");

    // `render` rappelle la fermeture une fois par bande et lui donne un
    // `DrawTarget` clippé ; elle dessine « l'écran entier » à chaque appel
    // sans avoir à savoir quelle bande est en cours.
    // `Infallible` : le `DrawTarget` du framebuffer est une écriture en RAM,
    // il ne peut pas échouer. L'annotation est nécessaire parce que `render`
    // est générique sur le type d'erreur de la fermeture.
    let style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let _: Result<(), core::convert::Infallible> = display.render(|target| {
        target.clear(Rgb565::BLACK)?;
        Text::new("Hello, world!", Point::new(20, 30), style).draw(target)?;
        Ok(())
    });

    // Battement de vie : confirme que le programme tourne toujours sans
    // redessiner (le contrôleur ILI9341 garde l'image en GRAM).
    loop {
        defmt::info!("screen_test vivant");
        board.timer.delay_ms(1_000);
    }
}
