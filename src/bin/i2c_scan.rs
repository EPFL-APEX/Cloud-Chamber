//! Scan I²C — affiche toutes les adresses qui répondent sur GP4/GP5.
//!
//! Flasher  : cargo build --bin i2c_scan  puis copier le .uf2
//! Moniteur : port USB série, 115200 baud

#![no_std]
#![no_main]

use core::fmt::Write as FmtWrite;

use rp2040_hal as hal;
use hal::pac;
use hal::Clock;
use embedded_hal::i2c::I2c;

use usb_device::{class_prelude::*, prelude::*};
use usbd_serial::SerialPort;
use heapless::String;

use defmt_rtt as _;
use panic_halt as _;

#[link_section = ".boot2"]
#[used]
pub static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

fn usb_write(serial: &mut SerialPort<hal::usb::UsbBus>, data: &[u8]) {
    let mut pos = 0;
    while pos < data.len() {
        match serial.write(&data[pos..]) {
            Ok(n)  => pos += n,
            Err(_) => break,
        }
    }
}

fn wait_ms(
    timer:   &hal::Timer,
    ms:      u64,
    usb_dev: &mut UsbDevice<hal::usb::UsbBus>,
    serial:  &mut SerialPort<hal::usb::UsbBus>,
) {
    let end = timer.get_counter().ticks() + ms * 1_000;
    while timer.get_counter().ticks() < end {
        if usb_dev.poll(&mut [serial]) {
            let mut buf = [0u8; 64];
            serial.read(&mut buf).ok();
        }
    }
}

#[hal::entry]
fn main() -> ! {
    let mut pac      = pac::Peripherals::take().unwrap();
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);

    let clocks = hal::clocks::init_clocks_and_plls(
        12_000_000u32,
        pac.XOSC, pac.CLOCKS, pac.PLL_SYS, pac.PLL_USB,
        &mut pac.RESETS, &mut watchdog,
    ).ok().unwrap();

    let timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

    let sio  = hal::Sio::new(pac.SIO);
    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0, pac.PADS_BANK0, sio.gpio_bank0, &mut pac.RESETS,
    );

    static mut USB_BUS: Option<UsbBusAllocator<hal::usb::UsbBus>> = None;
    let usb_alloc = unsafe {
        USB_BUS = Some(UsbBusAllocator::new(hal::usb::UsbBus::new(
            pac.USBCTRL_REGS, pac.USBCTRL_DPRAM,
            clocks.usb_clock, true, &mut pac.RESETS,
        )));
        (*core::ptr::addr_of!(USB_BUS)).as_ref().unwrap()
    };

    let mut serial  = SerialPort::new(usb_alloc);
    let mut usb_dev = UsbDeviceBuilder::new(usb_alloc, UsbVidPid(0x2e8a, 0x0005))
        .device_class(0x02)
        .build();

    use rp2040_hal::fugit::RateExtU32;
    let mut i2c = hal::I2C::new_controller(
        pac.I2C0,
        pins.gpio4.into_function::<hal::gpio::FunctionI2C>(),
        pins.gpio5.into_function::<hal::gpio::FunctionI2C>(),
        100u32.kHz(),
        &mut pac.RESETS,
        clocks.system_clock.freq(),
    );

    wait_ms(&timer, 5_000, &mut usb_dev, &mut serial);
    usb_write(&mut serial, b"=== Scan I2C (GP4=SDA GP5=SCL) ===\r\n\r\n");

    let mut found = 0u8;
    for addr in 0x08u8..=0x77 {
        let mut buf = [0u8; 1];
        if i2c.read(addr, &mut buf).is_ok() {
            // Lire le chip ID (registre 0xD0) pour identifier le composant
            let chip_id_str: &str = {
                let reg = [0xD0u8];
                let mut id = [0u8; 1];
                if i2c.write_read(addr, &reg, &mut id).is_ok() {
                    match id[0] {
                        0x60 => "BME280 (OK)",
                        0x58 => "BMP280 (pas de BME280!)",
                        0x56 | 0x57 => "BMP280 sample",
                        _ => "chip ID inconnu",
                    }
                } else {
                    "lecture ID echouee"
                }
            };

            let mut msg: String<64> = String::new();
            let _ = write!(msg, "  0x{:02X}  {}\r\n", addr, chip_id_str);
            usb_write(&mut serial, msg.as_bytes());
            found += 1;
        }
        usb_dev.poll(&mut [&mut serial]);
    }

    if found == 0 {
        usb_write(&mut serial, b"  aucun peripherique detecte\r\n");
        usb_write(&mut serial, b"  -> verifier VCC/GND/SDA/SCL et pull-ups\r\n");
    }

    let mut summary: String<48> = String::new();
    let _ = write!(summary, "\r\n{} peripherique(s) trouve(s)\r\n", found);
    usb_write(&mut serial, summary.as_bytes());

    loop { usb_dev.poll(&mut [&mut serial]); }
}
