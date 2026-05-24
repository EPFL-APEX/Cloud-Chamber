//! Test combiné DS18B20 (1-Wire GP15) + BME280 (I²C GP4/GP5).
//! Sortie via USB série (CDC ACM).
//!
//! Branchements :
//!   DS18B20 : VCC→3V3, GND→GND, DATA→GP15, pull-up 4.7kΩ entre DATA et 3V3
//!   BME280  : VCC→3V3, GND→GND, SDA→GP4, SCL→GP5, SDO→GND, CSB→VCC
//!
//! Flasher  : cargo run --bin test_capteurs  (glisser-déposer UF2)
//! Moniteur : PuTTY, minicom, screen…

#![no_std]
#![no_main]

use core::fmt::Write as FmtWrite;

use rp2040_hal as hal;
use hal::pac;
use hal::Clock;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{ErrorType, InputPin, OutputPin};
use core::convert::Infallible;

use usb_device::{class_prelude::*, prelude::*};
use usbd_serial::SerialPort;
use heapless::String;

use cloud_chamber::drivers::ds18b20::Ds18b20Bus;
use cloud_chamber::drivers::bme280::Bme280Driver;

use panic_probe as _;
use defmt_rtt as _;

// ════════════════════════════════════════════════════════════════════════════
// Boot2 — obligatoire RP2040
// ════════════════════════════════════════════════════════════════════════════

#[unsafe(link_section = ".boot2")]
#[used]
pub static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

// ════════════════════════════════════════════════════════════════════════════
// Délai matériel (Timer hardware RP2040)
//
// hal::Timer::get_counter() → compteur 64 bits à 1 MHz (1 tick = 1 µs).
// Beaucoup plus précis que cortex_m::asm::delay dont la durée dépend du pipeline.
// ════════════════════════════════════════════════════════════════════════════

struct TimerDelay<'a> {
    timer: &'a hal::Timer,
}

impl<'a> TimerDelay<'a> {
    fn new(timer: &'a hal::Timer) -> Self { Self { timer } }
}

impl<'a> DelayNs for TimerDelay<'a> {
    fn delay_ns(&mut self, ns: u32) {
        let us = ((ns as u64) + 999) / 1000;
        if us == 0 { return; }
        let end = self.timer.get_counter().ticks() + us;
        while self.timer.get_counter().ticks() < end {
            cortex_m::asm::nop();
        }
    }
}

// CortexDelay pour le BME280 (délai ~15 ms, précision non critique)
struct CortexDelay;

impl DelayNs for CortexDelay {
    fn delay_ns(&mut self, ns: u32) {
        // RP2040 @ 125 MHz : asm::delay(N) ≈ N×4 cycles ≈ N×32 ns
        cortex_m::asm::delay(ns / 32 + 1);
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Broche open-drain GP15 pour le protocole 1-Wire
//
// Le RP2040 n'a pas de GPIO open-drain natif.
//   set_low()  → OE=1, OUT=0 → tire la ligne à GND
//   set_high() → OE=0        → haute-impédance, pull-up remonte la ligne
//   is_high/low() → lit GPIO_IN (buffer d'entrée toujours actif)
// ════════════════════════════════════════════════════════════════════════════

const OW_PIN: u8 = 15;

struct OpenDrainPin {
    mask: u32,
    _owner: hal::gpio::Pin<
        hal::gpio::bank0::Gpio15,
        hal::gpio::FunctionSio<hal::gpio::SioOutput>,
        hal::gpio::PullNone,
    >,
}

impl OpenDrainPin {
    fn new(
        pin: hal::gpio::Pin<
            hal::gpio::bank0::Gpio15,
            hal::gpio::FunctionSio<hal::gpio::SioInput>,
            hal::gpio::PullNone,
        >,
    ) -> Self {
        let out_pin = pin.into_push_pull_output();
        let mask = 1u32 << OW_PIN;
        unsafe {
            let sio = &*pac::SIO::ptr();
            sio.gpio_out_clr().write(|w| w.bits(mask));
            sio.gpio_oe_clr().write(|w| w.bits(mask));
        }
        Self { mask, _owner: out_pin }
    }
}

impl ErrorType for OpenDrainPin { type Error = Infallible; }

impl OutputPin for OpenDrainPin {
    fn set_high(&mut self) -> Result<(), Infallible> {
        unsafe { (*pac::SIO::ptr()).gpio_oe_clr().write(|w| w.bits(self.mask)) };
        Ok(())
    }
    fn set_low(&mut self) -> Result<(), Infallible> {
        unsafe { (*pac::SIO::ptr()).gpio_oe_set().write(|w| w.bits(self.mask)) };
        Ok(())
    }
}

impl InputPin for OpenDrainPin {
    fn is_high(&mut self) -> Result<bool, Infallible> {
        Ok(unsafe { (*pac::SIO::ptr()).gpio_in().read().bits() } & self.mask != 0)
    }
    fn is_low(&mut self) -> Result<bool, Infallible> {
        Ok(unsafe { (*pac::SIO::ptr()).gpio_in().read().bits() } & self.mask == 0)
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Utilitaires 1-Wire bruts — accès direct aux registres SIO
// ════════════════════════════════════════════════════════════════════════════

const OW_MASK: u32 = 1u32 << 15;

#[inline(always)]
fn ow_wait(timer: &hal::Timer, us: u64) {
    let end = timer.get_counter().ticks() + us;
    while timer.get_counter().ticks() < end { cortex_m::asm::nop(); }
}

fn ow_reset_raw(timer: &hal::Timer) -> bool {
    unsafe {
        let sio = &*pac::SIO::ptr();
        sio.gpio_oe_clr().write(|w| w.bits(OW_MASK));
        ow_wait(timer, 5);
        sio.gpio_oe_set().write(|w| w.bits(OW_MASK));
        ow_wait(timer, 480);
        sio.gpio_oe_clr().write(|w| w.bits(OW_MASK));
        ow_wait(timer, 70);
        let presence = (sio.gpio_in().read().bits() >> 15) & 1 == 0;
        ow_wait(timer, 410);
        presence
    }
}

fn ow_write_byte_raw(timer: &hal::Timer, byte: u8) {
    unsafe {
        let sio = &*pac::SIO::ptr();
        for i in 0..8u32 {
            let bit = (byte >> i) & 1;
            sio.gpio_oe_set().write(|w| w.bits(OW_MASK));
            if bit == 1 {
                ow_wait(timer, 6);
                sio.gpio_oe_clr().write(|w| w.bits(OW_MASK));
                ow_wait(timer, 64);
            } else {
                ow_wait(timer, 60);
                sio.gpio_oe_clr().write(|w| w.bits(OW_MASK));
                ow_wait(timer, 10);
            }
        }
    }
}

fn ow_read_byte_raw(timer: &hal::Timer) -> u8 {
    let mut byte = 0u8;
    unsafe {
        let sio = &*pac::SIO::ptr();
        for i in 0..8u32 {
            sio.gpio_oe_set().write(|w| w.bits(OW_MASK));
            ow_wait(timer, 2);
            sio.gpio_oe_clr().write(|w| w.bits(OW_MASK));
            ow_wait(timer, 8);
            let bit = (sio.gpio_in().read().bits() >> 15) & 1;
            byte |= (bit as u8) << i;
            ow_wait(timer, 50);
        }
    }
    byte
}

// ════════════════════════════════════════════════════════════════════════════
// Utilitaires USB
// ════════════════════════════════════════════════════════════════════════════

fn wait_ms_usb(
    timer:   &hal::Timer,
    ms:      u64,
    usb_dev: &mut UsbDevice<hal::usb::UsbBus>,
    serial:  &mut SerialPort<hal::usb::UsbBus>,
) {
    let end_us = timer.get_counter().ticks() + ms * 1_000;
    while timer.get_counter().ticks() < end_us {
        if usb_dev.poll(&mut [serial]) {
            let mut buf = [0u8; 64];
            serial.read(&mut buf).ok();
        }
    }
}

fn usb_write(serial: &mut SerialPort<hal::usb::UsbBus>, data: &[u8]) {
    let mut pos = 0;
    while pos < data.len() {
        match serial.write(&data[pos..]) {
            Ok(n)  => pos += n,
            Err(_) => break,
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Point d'entrée
// ════════════════════════════════════════════════════════════════════════════

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

    // ── USB CDC ACM ──────────────────────────────────────────────────────────
    // SAFETY : initialisé une seule fois ici, avant tout accès concurrent.
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

    // ── I²C GP4/GP5 à 100 kHz ────────────────────────────────────────────────
    use rp2040_hal::fugit::RateExtU32;
    let i2c = hal::I2C::new_controller(
        pac.I2C0,
        pins.gpio4.into_function::<hal::gpio::FunctionI2C>(),
        pins.gpio5.into_function::<hal::gpio::FunctionI2C>(),
        100u32.kHz(),
        &mut pac.RESETS,
        clocks.system_clock.freq(),
    );

    // ── Drivers capteurs ──────────────────────────────────────────────────────
    let mut ds_bus = Ds18b20Bus::new(OpenDrainPin::new(pins.gpio15.into_floating_input()));
    let mut bme    = Bme280Driver::new(i2c);

    wait_ms_usb(&timer, 5_000, &mut usb_dev, &mut serial);

    let count = ds_bus.discover(&mut TimerDelay::new(&timer));

    if count > 0 {
        usb_write(&mut serial, b"DS18B20 OK  (GP15)\r\n");
    } else {
        usb_write(&mut serial, b"DS18B20 non detecte -- verifier GP15 et pull-up 4.7k\r\n");
    }

    loop {
        match bme.init() {
            Ok(()) => { usb_write(&mut serial, b"BME280 OK   (I2C GP4/GP5)\r\n\r\n"); break; }
            Err(_) => {
                usb_write(&mut serial, b"BME280 non detecte (I2C GP4/GP5) -- nouvel essai...\r\n");
                wait_ms_usb(&timer, 2_000, &mut usb_dev, &mut serial);
            }
        }
    }

    // DS18B20 → résolution 9-bit (93.75 ms max)
    if count > 0 && ow_reset_raw(&timer) {
        ow_write_byte_raw(&timer, 0xCC); // SKIP ROM
        ow_write_byte_raw(&timer, 0x4E); // WRITE SCRATCHPAD
        ow_write_byte_raw(&timer, 0x55);
        ow_write_byte_raw(&timer, 0x05);
        ow_write_byte_raw(&timer, 0x1F); // Config → 9-bit
    }

    loop {
        usb_dev.poll(&mut [&mut serial]);

        let conv_started = if count > 0 {
            ow_reset_raw(&timer) && {
                ow_write_byte_raw(&timer, 0xCC);
                ow_write_byte_raw(&timer, 0x44);
                true
            }
        } else { false };

        wait_ms_usb(&timer, 100, &mut usb_dev, &mut serial);

        let ds_result: Option<f32> = if conv_started {
            let mut result = None;
            'retry: for _attempt in 0..2u8 {
                if !ow_reset_raw(&timer) { break 'retry; }
                ow_write_byte_raw(&timer, 0xCC);
                ow_write_byte_raw(&timer, 0xBE);
                let mut sp = [0u8; 9];
                for b in sp.iter_mut() { *b = ow_read_byte_raw(&timer); }
                let crc = {
                    let mut c: u8 = 0;
                    for &b in sp.iter() {
                        let mut byte = b;
                        for _ in 0..8 {
                            let mix = (c ^ byte) & 1;
                            c >>= 1;
                            if mix != 0 { c ^= 0x8C; }
                            byte >>= 1;
                        }
                    }
                    c
                };
                if crc == 0 {
                    let raw_t = (sp[0] as u16) | ((sp[1] as u16) << 8);
                    result = Some(raw_t as i16 as f32 / 16.0);
                    break 'retry;
                }
                let t_retry = timer.get_counter().ticks() + 5_000;
                while timer.get_counter().ticks() < t_retry {}
            }
            result
        } else {
            None
        };

        let bme_result = bme.measure(&mut CortexDelay).ok();

        let mut msg: String<128> = String::new();

        match ds_result {
            Some(t) => {
                let neg  = t < 0.0;
                let abs  = if neg { -t } else { t };
                let sign = if neg { "-" } else { "" };
                let _ = write!(msg, "DS18B20: {}{}.{:02} C",
                    sign, abs as i32, ((abs % 1.0) * 100.0) as u32);
            }
            None => { let _ = write!(msg, "DS18B20: --"); }
        }

        let _ = write!(msg, "  |  ");

        match bme_result {
            Some((t, p, h)) => {
                let tneg  = t < 0.0;
                let tabs  = if tneg { -t } else { t };
                let tsign = if tneg { "-" } else { "" };
                let _ = write!(msg, "BME280: {}{}.{:02} C  Pres: {}.{:01} hPa  Humi: {}.{} %",
                    tsign, tabs as i32, ((tabs % 1.0) * 100.0) as u32,
                    p as u32, ((p % 1.0) * 10.0) as u32,
                    h as i32, ((h % 1.0) * 10.0) as u32);
            }
            None => { let _ = write!(msg, "BME280: --"); }
        }

        let _ = write!(msg, "\r\n");
        usb_write(&mut serial, msg.as_bytes());

        wait_ms_usb(&timer, 200, &mut usb_dev, &mut serial);
    }
}
