//! Point d'entrée du programme — Core0 : communication, affichage, logging.
//!
//! # Architecture deux cœurs
//!
//! ```text
//! Core0 (ce fichier)           Core1 (core1.rs)
//! ──────────────────           ───────────────
//! init des périphériques  →    spawn_core1(core1_task)
//! boucle UI / logging          boucle de sécurité (100 Hz)
//!      ↕ SHARED (critique)          ↕ SHARED (critique)
//! ```
//!
//! # `#![no_std]` et `#![no_main]`
//!
//! - `#![no_std]` : n'utilise pas la bibliothèque standard Rust (`std`).
//!   Sur un microcontrôleur sans OS, `std` n'est pas disponible.
//!   On utilise `core` (sous-ensemble de `std` sans heap ni OS).
//!
//! - `#![no_main]` : le point d'entrée n'est pas `fn main()` standard.
//!   Le macro `#[entry]` du HAL définit la vraie fonction de démarrage.
//!
//! Ces deux attributs sont désactivés en mode test (`cargo test`)
//! via `cfg_attr`, ce qui permet de tester le code sur desktop.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

pub mod cloud_chamber_hal;
pub mod core1;
pub mod drivers;
pub mod security_loop;
pub mod shared;
pub mod ui;

#[cfg(target_arch = "arm")]
use defmt::info;

#[cfg(target_arch = "arm")]
use defmt_rtt as _;

#[cfg(target_arch = "arm")]
use panic_probe as _;

#[cfg(target_arch = "riscv32")]
use panic_halt as _;

// Alias `hal` selon la cible compilée.
// On ajoute la garde `target_arch` pour éviter des résolutions sur desktop
// (build.rs émet `rp2350` même quand .pico-rs est absent).
#[cfg(all(rp2040, target_arch = "arm"))]
use rp2040_hal as hal;

#[cfg(all(rp2350, any(target_arch = "arm", target_arch = "riscv32")))]
use rp235x_hal as hal;

/// Bootloader en ROM (RP2040 uniquement).
#[unsafe(link_section = ".boot2")]
#[used]
#[cfg(all(rp2040, target_arch = "arm"))]
pub static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

/// Signature de l'image (RP2350 uniquement).
#[unsafe(link_section = ".start_block")]
#[used]
#[cfg(all(rp2350, any(target_arch = "arm", target_arch = "riscv32")))]
pub static IMAGE_DEF: hal::block::ImageDef = hal::block::ImageDef::secure_exe();

/// Fréquence du cristal externe (12 MHz sur Pico / Pico 2).
const XTAL_FREQ_HZ: u32 = 12_000_000;

#[cfg(any(target_arch = "arm", target_arch = "riscv32"))]
#[hal::entry]
fn main() -> ! {
    info!("Cloud Chamber démarrage…");

    let mut pac = hal::pac::Peripherals::take().unwrap();
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);

    let clocks = hal::clocks::init_clocks_and_plls(
        XTAL_FREQ_HZ,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .unwrap();

    #[cfg(rp2040)]
    let timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);
    #[cfg(rp2350)]
    let timer = hal::Timer::new_timer0(pac.TIMER0, &mut pac.RESETS, &clocks);

    let sio = hal::Sio::new(pac.SIO);
    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // SAFETY: CORE1_STACK n'est accédé que par spawn_core1 (une seule fois).
    let mut mc = hal::multicore::Multicore::new(&mut pac.PSM, &mut pac.PPB, &mut sio.fifo);
    let cores = mc.cores();
    let core1 = &mut cores[1];
    let _ = core1.spawn(unsafe { &mut core1::CORE1_STACK }, move || {
        core1::core1_task()
    });

    info!("Core1 lancé — boucle de sécurité active");

    let mut led = pins.gpio25.into_push_pull_output();
    let _ = timer;

    loop {
        let (snapshot, state, new_data) = critical_section::with(|cs| {
            let mut shared = shared::data::SHARED.borrow(cs).borrow_mut();
            let snap = shared.snapshot;
            let st = shared.system_state;
            let nd = shared.new_data;
            shared.new_data = false;
            (snap, st, nd)
        });

        if new_data {
            info!(
                "État: {:?} | T0={:?}°C | Chambre: {:?}",
                state as u8,
                snapshot.temps[0] as i32,
                snapshot.is_closed
            );
        }

        led.set_high().ok();
        cortex_m::asm::delay(6_000_000); // ~500 ms à 12 MHz
        led.set_low().ok();
        cortex_m::asm::delay(6_000_000);
    }
}

#[cfg(any(target_arch = "arm", target_arch = "riscv32"))]
#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [hal::binary_info::EntryAddr; 5] = [
    hal::binary_info::rp_cargo_bin_name!(),
    hal::binary_info::rp_cargo_version!(),
    hal::binary_info::rp_program_description!(c"Cloud Chamber Controller"),
    hal::binary_info::rp_cargo_homepage_url!(),
    hal::binary_info::rp_program_build_attribute!(),
];
