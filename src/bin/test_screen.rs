//! Test écran TFT ILI9341 — aucun capteur requis.
//!
//! Initialise l'écran avec mipidsi (même config que main.rs) et affiche
//! un fond + les valeurs factices du layout réel.
//! Résultat sur USB : "SCREEN OK" ou "SCREEN INIT FAILED".
//!
//! Flash : cargo build --release --target thumbv6m-none-eabi --bin test_screen
//!         elf2uf2-rs target/thumbv6m-none-eabi/release/test_screen test_screen.uf2

#![no_std]
#![no_main]

use rp2040_hal as hal;
use hal::{pac, Clock};
use embedded_hal::delay::DelayNs;

use usb_device::{class_prelude::*, prelude::*};
use usbd_serial::SerialPort;

use mipidsi::{Builder, interface::SpiInterface, models::ILI9341Rgb565, options::ColorOrder};
use embedded_hal_bus::spi::ExclusiveDevice;

use cloud_chamber_firmware::{
    control::{output::ControlOutput, target::TargetState},
    data::SystemState,
    display,
};

use defmt_rtt as _;
use panic_halt as _;

#[unsafe(link_section = ".boot2")]
#[used]
pub static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

struct CortexDelay;
impl DelayNs for CortexDelay {
    fn delay_ns(&mut self, ns: u32) { cortex_m::asm::delay(ns / 32 + 1); }
}

fn ow_wait(timer: &hal::Timer, us: u64) {
    let end = timer.get_counter().ticks() + us;
    while timer.get_counter().ticks() < end {}
}

fn usb_write(
    timer:   &hal::Timer,
    usb_dev: &mut UsbDevice<hal::usb::UsbBus>,
    serial:  &mut SerialPort<hal::usb::UsbBus>,
    data:    &[u8],
) {
    let deadline = timer.get_counter().ticks() + 10_000;
    let mut pos = 0;
    while pos < data.len() {
        match serial.write(&data[pos..]) {
            Ok(n) => pos += n,
            Err(UsbError::WouldBlock) => {
                usb_dev.poll(&mut [serial]);
                if timer.get_counter().ticks() >= deadline { break; }
            }
            Err(_) => break,
        }
    }
}

#[hal::entry]
fn main() -> ! {
    let mut pac      = pac::Peripherals::take().unwrap();
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);
    let clocks = hal::clocks::init_clocks_and_plls(
        12_000_000u32, pac.XOSC, pac.CLOCKS, pac.PLL_SYS, pac.PLL_USB,
        &mut pac.RESETS, &mut watchdog,
    ).ok().unwrap();

    let timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);
    let sio   = hal::Sio::new(pac.SIO);
    let pins  = hal::gpio::Pins::new(pac.IO_BANK0, pac.PADS_BANK0, sio.gpio_bank0, &mut pac.RESETS);

    // USB
    static mut USB_BUS: Option<UsbBusAllocator<hal::usb::UsbBus>> = None;
    let usb_alloc = unsafe {
        USB_BUS = Some(UsbBusAllocator::new(hal::usb::UsbBus::new(
            pac.USBCTRL_REGS, pac.USBCTRL_DPRAM, clocks.usb_clock, true, &mut pac.RESETS)));
        (*core::ptr::addr_of!(USB_BUS)).as_ref().unwrap()
    };
    let mut serial  = SerialPort::new(usb_alloc);
    let mut usb_dev = UsbDeviceBuilder::new(usb_alloc, UsbVidPid(0x2e8a, 0x0005))
        .device_class(0x02).build();

    // Attendre l'énumération USB (2 s)
    let end = timer.get_counter().ticks() + 2_000_000;
    while timer.get_counter().ticks() < end { usb_dev.poll(&mut [&mut serial]); }

    usb_write(&timer, &mut usb_dev, &mut serial, b"TEST SCREEN\r\n");

    // SPI1 — mêmes broches que main.rs
    use rp2040_hal::fugit::RateExtU32;
    let _mosi = pins.gpio11.into_function::<hal::gpio::FunctionSpi>();
    let _miso = pins.gpio12.into_function::<hal::gpio::FunctionSpi>();
    let _sck  = pins.gpio10.into_function::<hal::gpio::FunctionSpi>();
    let spi1 = hal::Spi::<_, _, _, 8>::new(pac.SPI1, (_mosi, _miso, _sck))
        .init(&mut pac.RESETS, clocks.peripheral_clock.freq(),
              10_000_000u32.Hz(), embedded_hal::spi::MODE_0);

    let disp_dc  = pins.gpio8.into_push_pull_output();
    let disp_rst = pins.gpio7.into_push_pull_output();
    let disp_cs  = pins.gpio9.into_push_pull_output();

    let spi_dev = ExclusiveDevice::new_no_delay(spi1, disp_cs).unwrap();
    let mut spi_tx_buf = [0u8; 512];
    let di = SpiInterface::new(spi_dev, disp_dc, &mut spi_tx_buf);

    let mut disp_opt = Builder::new(ILI9341Rgb565, di)
        .reset_pin(disp_rst)
        .display_size(240, 320)
        .color_order(ColorOrder::Bgr)
        .init(&mut CortexDelay)
        .ok();

    if let Some(d) = disp_opt.as_mut() {
        usb_write(&timer, &mut usb_dev, &mut serial, b"SCREEN OK - dessin du layout...\r\n");

        // Fond statique (labels, séparateurs) — compresseur bloqué, cycle inactif
        display::draw_static(d, false, false);

        // Valeurs factices pour tester le rendu complet
        let mut state  = SystemState::new();
        let target     = TargetState::default();
        let output     = ControlOutput { compressor: true, isopropanol_heater_duty: 0.0,
                                        high_voltage: false, safety_override: false };

        // Simuler 3 capteurs DS valides
        for i in 0..3usize {
            state.temperatures[i].valid = true;
            state.temperatures[i].value = 20.0 - i as f32 * 5.0;
        }
        // BME280 simulé
        state.bme280.valid        = true;
        state.bme280.temp_c       = 22.5;
        state.bme280.pressure_hpa = 1013.2;
        state.bme280.humidity_pct = 45.0;
        state.uptime_s            = 42;

        display::draw(d, &state, &target, &output, 3, None, None, false);

        usb_write(&timer, &mut usb_dev, &mut serial, b"SCREEN OK - layout affiche\r\n");
    } else {
        usb_write(&timer, &mut usb_dev, &mut serial, b"SCREEN INIT FAILED - verifier GP7/GP8/GP9/GP10/GP11\r\n");
    }

    loop {
        usb_dev.poll(&mut [&mut serial]);
        ow_wait(&timer, 10_000);
    }
}
