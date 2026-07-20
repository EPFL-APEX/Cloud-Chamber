//! Diagnostic brut SPI display — sans mipidsi.
//! Envoie SWRESET + SLPOUT + DISPON + remplissage rouge directement.
//! Si l'écran change : SPI fonctionne.  Si reste blanc : MOSI pas connecté.

#![no_std]
#![no_main]

use rp2040_hal as hal;
use hal::{pac, Clock};
use embedded_hal::digital::OutputPin;
use embedded_hal::delay::DelayNs;
use embedded_hal::spi::SpiBus;

use defmt_rtt as _;
use panic_halt as _;

#[link_section = ".boot2"]
#[used]
pub static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

struct Delay;
impl DelayNs for Delay {
    fn delay_ns(&mut self, ns: u32) { cortex_m::asm::delay(ns / 32 + 1); }
}

#[hal::entry]
fn main() -> ! {
    let mut pac      = pac::Peripherals::take().unwrap();
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);
    let clocks = hal::clocks::init_clocks_and_plls(
        12_000_000u32, pac.XOSC, pac.CLOCKS, pac.PLL_SYS, pac.PLL_USB,
        &mut pac.RESETS, &mut watchdog,
    ).ok().unwrap();

    let sio  = hal::Sio::new(pac.SIO);
    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0, pac.PADS_BANK0, sio.gpio_bank0, &mut pac.RESETS,
    );
    let mut delay = Delay;

    let mosi = pins.gpio11.into_function::<hal::gpio::FunctionSpi>();
    let miso = pins.gpio12.into_function::<hal::gpio::FunctionSpi>();
    let sck  = pins.gpio10.into_function::<hal::gpio::FunctionSpi>();
    let mut cs  = pins.gpio9.into_push_pull_output();
    let mut dc  = pins.gpio8.into_push_pull_output();
    let mut rst = pins.gpio7.into_push_pull_output();

    use rp2040_hal::fugit::RateExtU32;
    let mut spi = hal::Spi::<_, _, _, 8>::new(pac.SPI1, (mosi, miso, sck))
        .init(&mut pac.RESETS, clocks.peripheral_clock.freq(),
              1_000_000u32.Hz(), embedded_hal::spi::MODE_0);

    cs.set_high().ok();
    dc.set_high().ok();

    // ── RST pulse ─────────────────────────────────────────────────────────────
    rst.set_high().ok(); delay.delay_ms(10);
    rst.set_low().ok();  delay.delay_ms(20);
    rst.set_high().ok(); delay.delay_ms(150);

    // ── Macro helpers locaux ───────────────────────────────────────────────────
    macro_rules! cmd {
        ($c:expr) => {{
            dc.set_low().ok();
            cs.set_low().ok();
            spi.write(&[$c]).ok();
            cs.set_high().ok();
        }};
    }
    macro_rules! dat {
        ($d:expr) => {{
            dc.set_high().ok();
            cs.set_low().ok();
            spi.write($d).ok();
            cs.set_high().ok();
        }};
    }

    // ── Calibration MADCTL : moitié gauche ROUGE, moitié droite VERTE ────────
    // Bande blanche sur le bord gauche (x 0-4), bande bleue en haut (y 0-4).
    // Observer l'écran :
    //   → bande BLANCHE à GAUCHE physique  &  bande BLEUE en HAUT  → valeur ok
    //   → bande blanche à DROITE           → inverser bit 6 (MX)
    //   → bande bleue en BAS               → inverser bit 7 (MY)
    //
    // Valeurs courantes à tester (changer const et reflasher) :
    //   0x08  no flip  (BGR)
    //   0x48  MX flip  (BGR)
    //   0x88  MY flip  (BGR)
    //   0xC8  MX+MY    (BGR)
    const MADCTL_TEST: u8 = 0x08;

    cmd!(0x01); delay.delay_ms(150); // SWRESET
    cmd!(0x11); delay.delay_ms(120); // SLPOUT
    cmd!(0x3A); dat!(&[0x55]);       // COLMOD 16-bit RGB565
    cmd!(0x36); dat!(&[MADCTL_TEST]);
    cmd!(0x29); delay.delay_ms(20);  // DISPON

    // moitié gauche ROUGE (x 0..119, y 0..319)
    cmd!(0x2A); dat!(&[0x00, 0x00, 0x00, 0x77]);
    cmd!(0x2B); dat!(&[0x00, 0x00, 0x01, 0x3F]);
    cmd!(0x2C);
    dc.set_high().ok(); cs.set_low().ok();
    for _ in 0u32..(120 * 320) { spi.write(&[0xF8, 0x00]).ok(); }
    cs.set_high().ok();

    // moitié droite VERTE (x 120..239, y 0..319)
    cmd!(0x2A); dat!(&[0x00, 0x78, 0x00, 0xEF]);
    cmd!(0x2B); dat!(&[0x00, 0x00, 0x01, 0x3F]);
    cmd!(0x2C);
    dc.set_high().ok(); cs.set_low().ok();
    for _ in 0u32..(120 * 320) { spi.write(&[0x07, 0xE0]).ok(); }
    cs.set_high().ok();

    // bande BLANCHE sur le bord gauche (x 0..4, y 0..319)
    cmd!(0x2A); dat!(&[0x00, 0x00, 0x00, 0x04]);
    cmd!(0x2B); dat!(&[0x00, 0x00, 0x01, 0x3F]);
    cmd!(0x2C);
    dc.set_high().ok(); cs.set_low().ok();
    for _ in 0u32..(5 * 320) { spi.write(&[0xFF, 0xFF]).ok(); }
    cs.set_high().ok();

    // bande BLEUE en haut (x 0..239, y 0..4)
    cmd!(0x2A); dat!(&[0x00, 0x00, 0x00, 0xEF]);
    cmd!(0x2B); dat!(&[0x00, 0x00, 0x00, 0x04]);
    cmd!(0x2C);
    dc.set_high().ok(); cs.set_low().ok();
    for _ in 0u32..(240 * 5) { spi.write(&[0x00, 0x1F]).ok(); }
    cs.set_high().ok();

    loop { cortex_m::asm::wfi(); }
}
