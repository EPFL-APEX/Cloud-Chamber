//! Sanity check de bring-up : câble l'écran ILI9341 et l'encodeur rotatif
//! réels sur `ui::app::UiApp` — la vraie UI, pas un mock — pour la
//! vérifier de bout en bout (tourner/cliquer, écrans qui s'affichent,
//! démarrage d'un cycle) avant intégration dans `logic::control_loop`.
//!
//! # Répartition du travail
//!
//! Ce fichier ne contient presque plus rien. Le montage matériel — écran,
//! encodeur, interruption de scrutation, file d'événements — vit dans
//! [`ui::console`](cloud_chamber_firmware::ui::console), partagé avec
//! `main.rs` ; tout ce qui est décidable sans matériel vit dans `ui::app`,
//! testé sur hôte. Ce qui reste ici est la seule chose propre à ce bin : la
//! boucle, sans cœur 1, sans capteurs, sans flash.
//!
//! C'est délibéré et c'est tout l'intérêt du bin : **il exerce exactement
//! le chemin du firmware**, même fréquence SPI, même framebuffer, même
//! interruption. Quand les trois bins de bring-up portaient chacun leur
//! copie, `screen_test` tournait à 16 MHz pendant que le firmware en
//! faisait 32 — voir la doc de module de `console` pour ce que cette
//! divergence coûtait.
//!
//! # Deux problèmes de performance, et ce qu'il a fallu pour les corriger
//!
//! ## Rendu lent (plusieurs secondes pour un menu)
//!
//! Résolu par `drivers::display::FramebufferedDisplay` : dessin dans un
//! framebuffer RAM bandé, puis une seule transaction SPI par bande au lieu
//! d'une par pixel.
//!
//! ## Rotation perdue pendant un dessin
//!
//! Le rendu bloque le cœur. Avec un `encoder.poll()` appelé depuis la
//! boucle, toute rotation survenant pendant ce blocage n'était pas
//! retardée mais **perdue** : deux rotations rapprochées ne comptaient que
//! pour une.
//!
//! Scruter depuis `TIMER_IRQ_0` était nécessaire mais pas suffisant. Tant
//! que l'ISR appliquait elle-même la navigation, `UiApp` devait être un
//! static partagé, donc le dessin se faisait section critique prise — et
//! une section critique masque `TIMER_IRQ_0`. La partie la plus coûteuse
//! du rendu se déroulait donc interruptions coupées, et le bug revenait
//! par l'autre bout. Le découplage par file d'événements est ce qui ferme
//! vraiment le problème ; `console` l'impose à ses deux appelants.
//!
//! # Écrans pas encore construits
//!
//! Les 6 items du menu principal sont tous sûrs à ouvrir, y compris en
//! tournant et en cliquant dedans. **Données** et **Info** n'ont pas encore
//! d'écran réel : ils affichent le carton d'attente de
//! `ui::screens::placeholder`, dont un clic ressort.
//!
//! # Démarrage d'un cycle
//!
//! Cliquer sur le premier item du menu écrit
//! `SystemTask::Cooling(SensorCheck)` dans `SHARED_STATE.task` et bascule
//! sur l'écran de suivi. Ce bin n'exécute **pas**
//! `logic::control_loop::run()` (pas de capteurs ni d'actionneurs réels
//! câblés ici) : l'état écrit reste donc figé sur `SensorCheck` et aucune
//! phase n'avance. C'est le comportement attendu — il vérifie le chemin
//! UI → état partagé, pas la machine à états.
//!
//! RP2040 uniquement, derrière la feature `bin-ui-test` (désactivée par
//! défaut) pour ne pas être construit par les jobs CI `cargo check` sur les
//! autres cibles :
//!
//! ```text
//! cargo run --release --target thumbv6m-none-eabi --features bin-ui-test \
//!     --bin ui_test
//! ```
#![no_std]
#![no_main]

use defmt_rtt as _;
use panic_probe as _;

use rp2040_hal::{Clock, self as hal};

use cloud_chamber_firmware::board;
use cloud_chamber_firmware::shared::data::SHARED_STATE;
use cloud_chamber_firmware::ui::app::UiApp;
use cloud_chamber_firmware::ui::console;

#[hal::entry]
fn main() -> ! {
    let mut board = board::init();

    let mut console = console::init(
        board.spi0,
        &mut board.resets,
        board.clocks.peripheral_clock.freq(),
        &mut board.timer,
    );

    // `UiApp` appartient à cette fonction — c'est tout l'objet du
    // découplage : il n'est partagé avec personne, donc le rendu ne prend
    // aucune section critique et l'encodeur reste scruté pendant le dessin.
    let mut app = UiApp::new();

    defmt::info!("ui_test demarre — premier rendu (MainMenu)");

    let initial_state = critical_section::with(|cs| *SHARED_STATE.borrow_ref(cs));
    console.redraw(&app, &initial_state);
    app.take_redraw_request();

    loop {
        console.pump(&mut app);

        // Copie de l'état sous verrou court ; le dessin travaille dessus,
        // hors verrou.
        let state = critical_section::with(|cs| *SHARED_STATE.borrow_ref(cs));

        if app.take_redraw_request() {
            console.redraw(&app, &state);
        }
    }
}
