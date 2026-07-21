//! Firmware principal — Chambre à brouillard (Pico W)
//!
//! Capteurs  : DS18B20 (GP15, 1-Wire) + BME280 (I²C GP4/GP5)
//! Actuateur : relais compresseur (GP16)
//! Interface : USB série bidirectionnel
//!
//! Commandes (PC → Pico) :
//!   TARGET <°C>    temperature cible chambre
//!   HV <0|1>       haut voltage on/off
//!   COMP <0|1>     autoriser/bloquer compresseur
//!
//! Publications (Pico → PC, 5 Hz) :
//!   STATE ds0=<T> .. ds4=<T> bt=<T> bp=<P> bh=<H>
//!         tg=<T> co=<0|1> ca=<0|1> hv=<0|1> iso=<0.00> sf=<0|1> up=<s>

#![no_std]
#![no_main]

use core::fmt::Write as FmtWrite;
use rp2040_hal as hal;
use hal::pac;
use hal::Clock;
use embedded_hal::digital::OutputPin;
use embedded_hal::delay::DelayNs;

use usb_device::{class_prelude::*, prelude::*};
use usbd_serial::SerialPort;

/// Port série USB avec buffers dédiés — RX 128 o, TX 512 o (cf. new_with_store).
type Serial<'a> = SerialPort<'a, hal::usb::UsbBus, [u8; 128], [u8; 512]>;
use heapless::String;

use cloud_chamber_firmware::{
    control::{controller::Controller, output::ControlOutput, target::TargetState},
    data::SystemState,
    logic::history::MeasurementHistory,
    sensors::bme280::Bme280Driver,
    display,
};

use mipidsi::{Builder, interface::SpiInterface, models::ILI9341Rgb565,
              options::{ColorOrder, Orientation, Rotation}};
use embedded_hal_bus::spi::ExclusiveDevice;

use defmt_rtt as _;
use panic_halt as _;

// ── Boot2 ─────────────────────────────────────────────────────────────────────
#[link_section = ".boot2"]
#[used]
pub static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

// ── 1-Wire — bus unique sur GP15 ─────────────────────────────────────────────
// Tous les DS18B20 sont sur le même fil avec pull-up 4.7 kΩ.
// Les ROM codes sont découverts après l'init USB (SEARCH ROM).
// Slot 0..N = ordre de découverte — publié via USB pour identification.
const OW_MASK: u32 = 1 << 15; // GP15 — cf. config::PIN_ONEWIRE

// BME280 delay via cortex cycles
struct CortexDelay;
impl DelayNs for CortexDelay {
    fn delay_ns(&mut self, ns: u32) { cortex_m::asm::delay(ns / 32 + 1); }
}

// ── Fonctions 1-Wire ──────────────────────────────────────────────────────────
#[inline(always)]
fn ow_wait(timer: &hal::Timer, us: u64) {
    let end = timer.get_counter().ticks() + us;
    while timer.get_counter().ticks() < end { cortex_m::asm::nop(); }
}

fn ow_reset(timer: &hal::Timer, mask: u32) -> bool {
    unsafe {
        let sio = &*pac::SIO::ptr();
        sio.gpio_oe_clr().write(|w| w.bits(mask)); ow_wait(timer, 5);
        sio.gpio_oe_set().write(|w| w.bits(mask)); ow_wait(timer, 480);
        sio.gpio_oe_clr().write(|w| w.bits(mask)); ow_wait(timer, 70);
        let presence = sio.gpio_in().read().bits() & mask == 0;
        ow_wait(timer, 410);
        presence
    }
}

// Briques bit-level (base pour SEARCH ROM)
fn ow_write_bit(timer: &hal::Timer, mask: u32, bit: bool) {
    unsafe {
        let sio = &*pac::SIO::ptr();
        sio.gpio_oe_set().write(|w| w.bits(mask));
        if bit {
            ow_wait(timer, 6);
            sio.gpio_oe_clr().write(|w| w.bits(mask));
            ow_wait(timer, 64);
        } else {
            ow_wait(timer, 60);
            sio.gpio_oe_clr().write(|w| w.bits(mask));
            ow_wait(timer, 10);
        }
    }
}

fn ow_read_bit(timer: &hal::Timer, mask: u32) -> bool {
    unsafe {
        let sio = &*pac::SIO::ptr();
        sio.gpio_oe_set().write(|w| w.bits(mask)); ow_wait(timer, 2);
        sio.gpio_oe_clr().write(|w| w.bits(mask)); ow_wait(timer, 8);
        let bit = sio.gpio_in().read().bits() & mask != 0;
        ow_wait(timer, 50);
        bit
    }
}

fn ow_write_byte(timer: &hal::Timer, mask: u32, byte: u8) {
    for i in 0..8u32 { ow_write_bit(timer, mask, (byte >> i) & 1 == 1); }
}

fn ow_read_byte(timer: &hal::Timer, mask: u32) -> u8 {
    let mut b = 0u8;
    for i in 0..8u32 { if ow_read_bit(timer, mask) { b |= 1 << i; } }
    b
}

// CRC-8 Dallas / Maxim (polynôme réfléchi 0x8C) — utilisé pour ROM et scratchpad
fn crc8(data: &[u8]) -> u8 {
    let mut c = 0u8;
    for &b in data {
        let mut x = b;
        for _ in 0..8 { let m=(c^x)&1; c>>=1; if m!=0{c^=0x8C;} x>>=1; }
    }
    c
}

// ── SEARCH ROM — algorithme Dallas AN187 ─────────────────────────────────────
struct OwSearch { last_discrepancy: u8, last_device_flag: bool, rom: [u8; 8] }

impl OwSearch {
    const fn new() -> Self {
        Self { last_discrepancy: 0, last_device_flag: false, rom: [0u8; 8] }
    }

    fn next(&mut self, timer: &hal::Timer, mask: u32) -> Option<[u8; 8]> {
        if self.last_device_flag { return None; }
        if !ow_reset(timer, mask) {
            self.last_discrepancy = 0;
            self.last_device_flag = false;
            return None;
        }
        ow_write_byte(timer, mask, 0xF0); // SEARCH ROM

        let mut last_zero = 0u8;
        let mut bit_number = 1u8;
        for byte_idx in 0..8usize {
            for bit_shift in 0..8u8 {
                let id_bit  = ow_read_bit(timer, mask);
                let cmp_bit = ow_read_bit(timer, mask);
                if id_bit && cmp_bit { return None; } // erreur bus

                let dir = if id_bit != cmp_bit {
                    id_bit
                } else {
                    // discordance : suivre le chemin précédent ou choisir 0
                    if bit_number < self.last_discrepancy {
                        (self.rom[byte_idx] >> bit_shift) & 1 == 1
                    } else {
                        bit_number == self.last_discrepancy
                    }
                };

                if !id_bit && !cmp_bit && !dir { last_zero = bit_number; }
                if dir { self.rom[byte_idx] |= 1 << bit_shift; }
                else   { self.rom[byte_idx] &= !(1 << bit_shift); }
                ow_write_bit(timer, mask, dir);
                bit_number += 1;
            }
        }
        self.last_discrepancy = last_zero;
        self.last_device_flag = last_zero == 0;
        Some(self.rom)
    }
}

// ── USB ───────────────────────────────────────────────────────────────────────
// Écriture bloquante bornée : le buffer TX de usbd-serial (128 o) est plus petit
// qu'une ligne STATE (~140 o), il faut donc poller pour le drainer entre deux
// écritures partielles. Le deadline de 10 ms évite de bloquer la boucle de
// contrôle si l'hôte ne lit pas (terminal fermé).
fn usb_write(
    timer:   &hal::Timer,
    usb_dev: &mut UsbDevice<hal::usb::UsbBus>,
    serial:  &mut Serial<'_>,
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

fn keepalive(timer: &hal::Timer, ms: u64,
             usb_dev: &mut UsbDevice<hal::usb::UsbBus>,
             serial:  &mut Serial<'_>) {
    let end = timer.get_counter().ticks() + ms * 1_000;
    while timer.get_counter().ticks() < end { usb_dev.poll(&mut [serial]); }
}

// ── Parsing float no_std ──────────────────────────────────────────────────────
fn parse_f32(s: &str) -> Option<f32> {
    let s = s.trim();
    let (neg, s) = if s.starts_with('-') { (true, &s[1..]) } else { (false, s) };
    let mut int_p: f32 = 0.0;
    let mut frac_p: f32 = 0.0;
    let mut frac_div: f32 = 1.0;
    let mut in_frac = false;
    if s.is_empty() { return None; }
    for c in s.chars() {
        match c {
            '0'..='9' => {
                let d = (c as u8 - b'0') as f32;
                if in_frac { frac_div *= 10.0; frac_p += d / frac_div; }
                else { int_p = int_p * 10.0 + d; }
            }
            '.' => { if in_frac { return None; } in_frac = true; }
            _ => return None,
        }
    }
    let v = int_p + frac_p;
    Some(if neg { -v } else { v })
}

// ── Parser de commandes ───────────────────────────────────────────────────────
fn handle_command(
    cmd:     &str,
    target:  &mut TargetState,
    state:   &mut SystemState,
    ctrl:    &mut Controller,
    timer:   &hal::Timer,
    usb_dev: &mut UsbDevice<hal::usb::UsbBus>,
    serial:  &mut Serial<'_>,
) {
    let cmd  = cmd.trim();
    let (name, rest) = cmd.split_once(' ').unwrap_or((cmd, ""));
    let rest = rest.trim();
    let now_ms = timer.get_counter().ticks() / 1_000;

    match name {
        "CYCLE" => match rest {
            "1" => {
                // Le cycle automatique implique l'autorisation compresseur.
                state.compressor_allowed = true;
                if ctrl.request_start(now_ms) {
                    usb_write(timer, usb_dev, serial, b"OK CYCLE=1\r\n");
                } else if ctrl.is_tripped() {
                    usb_write(timer, usb_dev, serial,
                        b"ERR CYCLE refuse - disjoncteur verrouille (CYCLE 0 pour rearmer)\r\n");
                } else if ctrl.phase_code() >= 8 {
                    usb_write(timer, usb_dev, serial,
                        b"ERR CYCLE refuse - arret en cours (equilibrage anti court-cycle, ~60 s)\r\n");
                } else {
                    usb_write(timer, usb_dev, serial,
                        b"ERR CYCLE refuse - cycle deja en cours\r\n");
                }
            }
            "0" => {
                if ctrl.request_stop(now_ms) {
                    usb_write(timer, usb_dev, serial, b"OK CYCLE=0\r\n");
                } else {
                    usb_write(timer, usb_dev, serial, b"OK CYCLE=0 (deja inactif)\r\n");
                }
            }
            _ => usb_write(timer, usb_dev, serial, b"ERR CYCLE needs 0 or 1\r\n"),
        },
        "TARGET" => match parse_f32(rest) {
            Some(v) => {
                target.chamber_temp_c = v;
                let mut r: String<32> = String::new();
                let neg = v < 0.0; let abs = if neg { -v } else { v };
                let _ = write!(r, "OK TARGET={}{}.{}\r\n",
                    if neg {"-"} else {""}, abs as i32, ((abs % 1.0)*10.0) as u32);
                usb_write(timer, usb_dev, serial,r.as_bytes());
            }
            None => usb_write(timer, usb_dev, serial,b"ERR TARGET needs a float\r\n"),
        },
        "HV" => match rest {
            "1" => { target.high_voltage_enabled = true;  usb_write(timer, usb_dev, serial,b"OK HV=1\r\n"); }
            "0" => { target.high_voltage_enabled = false; usb_write(timer, usb_dev, serial,b"OK HV=0\r\n"); }
            _   => usb_write(timer, usb_dev, serial,b"ERR HV needs 0 or 1\r\n"),
        },
        "COMP" => match rest {
            "1" => { state.compressor_allowed = true;  usb_write(timer, usb_dev, serial,b"OK COMP=1\r\n"); }
            "0" => { state.compressor_allowed = false; usb_write(timer, usb_dev, serial,b"OK COMP=0\r\n"); }
            _   => usb_write(timer, usb_dev, serial,b"ERR COMP needs 0 or 1\r\n"),
        },
        _ => usb_write(timer, usb_dev, serial,b"ERR unknown command\r\n"),
    }
}

// ── Publication d'état ────────────────────────────────────────────────────────
fn publish_state(
    timer:    &hal::Timer,
    usb_dev:  &mut UsbDevice<hal::usb::UsbBus>,
    serial:   &mut Serial<'_>,
    state:    &SystemState,
    output:   &ControlOutput,
    target:   &TargetState,
    phase:    u8,
    uptime_s: u64,
) {
    let mut msg: String<320> = String::new();
    let _ = write!(msg, "STATE ");

    for i in 0..5usize {
        let ds = &state.temperatures[i];
        if ds.valid {
            let neg = ds.value < 0.0; let abs = if neg { -ds.value } else { ds.value };
            let _ = write!(msg, "ds{}={}{}.{:02} ", i, if neg {"-"} else {""}, abs as i32, ((abs%1.0)*100.0) as u32);
        } else {
            let _ = write!(msg, "ds{}=-- ", i);
        }
    }

    let bme = &state.bme280;
    if bme.valid {
        let tn = bme.temp_c < 0.0; let ta = if tn { -bme.temp_c } else { bme.temp_c };
        let _ = write!(msg, "bt={}{}.{:02} bp={}.{} bh={}.{} ",
            if tn {"-"} else {""}, ta as i32, ((ta%1.0)*100.0) as u32,
            bme.pressure_hpa as u32, ((bme.pressure_hpa%1.0)*10.0) as u32,
            bme.humidity_pct as u32, ((bme.humidity_pct%1.0)*10.0) as u32);
    } else {
        let _ = write!(msg, "bt=-- bp=-- bh=-- ");
    }

    let t = target.chamber_temp_c;
    let tn = t < 0.0; let ta = if tn { -t } else { t };
    let _ = write!(msg, "tg={}{}.{} co={} ca={} hv={} ph={} iso={}.{:02} sf={} up={}\r\n",
        if tn {"-"} else {""}, ta as i32, ((ta%1.0)*10.0) as u32,
        output.compressor as u8,
        state.compressor_allowed as u8,
        output.high_voltage as u8,
        phase,
        output.isopropanol_heater_duty as i32,
        ((output.isopropanol_heater_duty%1.0)*100.0) as u32,
        output.safety_override as u8,
        uptime_s);

    usb_write(timer, usb_dev, serial,msg.as_bytes());
}

// ── XPT2046 bit-bang ──────────────────────────────────────────────────────────
// DCLK max du XPT2046 = 2 MHz : l'ADC SAR convertit PENDANT les clocks de
// lecture. Bit-bangé sans délai, le RP2040 dépasse cette limite → conversions
// fausses (X/Y ≈ 0 constants alors que Z1 semble plausible). On force ~500 kHz.
#[inline(always)]
fn xpt_tick() { cortex_m::asm::delay(125); } // ~1 µs @125 MHz → DCLK ≈ 500 kHz

// Envoie une commande 8 bits et lit 16 bits de réponse.
// Retourne les 12 bits de données : (val >> 3) & 0x0FFF (protocole XPT2046).
fn xpt2046_read(
    clk:  &mut impl embedded_hal::digital::OutputPin,
    din:  &mut impl embedded_hal::digital::OutputPin,
    dout: &mut impl embedded_hal::digital::InputPin,
    cmd:  u8,
) -> u16 {
    for i in (0..8u8).rev() {
        if (cmd >> i) & 1 == 1 { din.set_high().ok(); } else { din.set_low().ok(); }
        xpt_tick();
        clk.set_high().ok();
        xpt_tick();
        clk.set_low().ok();
    }
    din.set_low().ok();
    let mut val = 0u16;
    for _ in 0..16u8 {
        clk.set_high().ok();
        xpt_tick();
        val = (val << 1) | dout.is_high().unwrap_or(false) as u16;
        clk.set_low().ok();
        xpt_tick();
    }
    (val >> 3) & 0x0FFF
}

// Lecture d'un canal avec temps d'établissement : la 1re conversion après le
// changement de drivers du panneau est jetée, puis moyenne de 2 lectures.
fn xpt2046_read_ch(
    clk:  &mut impl embedded_hal::digital::OutputPin,
    din:  &mut impl embedded_hal::digital::OutputPin,
    dout: &mut impl embedded_hal::digital::InputPin,
    cmd:  u8,
) -> u16 {
    let _ = xpt2046_read(clk, din, dout, cmd); // dummy : polarise le panneau
    let a  = xpt2046_read(clk, din, dout, cmd);
    let b  = xpt2046_read(clk, din, dout, cmd);
    (a + b) / 2
}

// Lit Z1, X, Y bruts — pour diagnostic uniquement.
fn touch_raw(
    clk:  &mut impl embedded_hal::digital::OutputPin,
    din:  &mut impl embedded_hal::digital::OutputPin,
    dout: &mut impl embedded_hal::digital::InputPin,
    cs:   &mut impl embedded_hal::digital::OutputPin,
) -> (u16, u16, u16) {
    cs.set_low().ok();
    let z1 = xpt2046_read(clk, din, dout, 0xB1);
    let x  = xpt2046_read_ch(clk, din, dout, 0xD1);
    let y  = xpt2046_read_ch(clk, din, dout, 0x91);
    xpt2046_read(clk, din, dout, 0x00);
    cs.set_high().ok();
    (z1, x, y)
}

// Lit X et Y si l'écran est touché (Z1 > 50), sinon None.
fn touch_read(
    clk:  &mut impl embedded_hal::digital::OutputPin,
    din:  &mut impl embedded_hal::digital::OutputPin,
    dout: &mut impl embedded_hal::digital::InputPin,
    cs:   &mut impl embedded_hal::digital::OutputPin,
) -> Option<(u16, u16)> {
    cs.set_low().ok();
    let z1 = xpt2046_read(clk, din, dout, 0xB1); // canal pression Z1
    if z1 < 50 {
        cs.set_high().ok();
        return None;
    }
    let x = xpt2046_read_ch(clk, din, dout, 0xD1); // canal X
    let y = xpt2046_read_ch(clk, din, dout, 0x91); // canal Y
    xpt2046_read(clk, din, dout, 0x00);            // mise en veille
    cs.set_high().ok();
    if x < 100 || x > 3950 || y < 100 || y > 3950 { return None; }
    Some((x, y))
}

// ── Point d'entrée ────────────────────────────────────────────────────────────
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
    // Buffer TX de 512 o : une ligne STATE (~150 o) tient d'un coup, usb_write
    // n'a plus besoin de drainer en boucle (le défaut de 128 o était le goulot).
    let mut serial  = SerialPort::new_with_store(usb_alloc, [0u8; 128], [0u8; 512]);
    let mut usb_dev = UsbDeviceBuilder::new(usb_alloc, UsbVidPid(0x2e8a, 0x0005))
        .device_class(0x02).build();

    // Enumération USB immédiate — Windows doit voir le device dans les 2 s
    keepalive(&timer, 2_000, &mut usb_dev, &mut serial);

    // Écran TFT — SPI1 (GP10=SCK, GP11=MOSI, GP12=MISO interne, GP8=DC, GP9=CS, GP7=RST)
    let _disp_mosi = pins.gpio11.into_function::<hal::gpio::FunctionSpi>();
    let _disp_miso = pins.gpio12.into_function::<hal::gpio::FunctionSpi>(); // non câblé
    let _disp_sck  = pins.gpio10.into_function::<hal::gpio::FunctionSpi>();
    let spi1 = hal::Spi::<_, _, _, 8>::new(pac.SPI1, (_disp_mosi, _disp_miso, _disp_sck))
        // 20 MHz : l'ILI9341 accepte 20-40 MHz en écriture. Un redraw plein
        // écran (240×320×2 o) prend ~62 ms. Repasser à 10 MHz si artefacts.
        .init(&mut pac.RESETS, clocks.peripheral_clock.freq(), 20_000_000u32.Hz(), embedded_hal::spi::MODE_0);
    let disp_dc  = pins.gpio8.into_push_pull_output();
    let disp_rst = pins.gpio7.into_push_pull_output();
    let disp_cs  = pins.gpio9.into_push_pull_output(); // géré par ExclusiveDevice

    // Sorties de contrôle — toutes à LOW par défaut (fail-safe)
    let mut relay   = pins.gpio16.into_push_pull_output(); // compresseur
    let mut hv_out  = pins.gpio17.into_push_pull_output(); // haut voltage
    let mut iso_out = pins.gpio18.into_push_pull_output(); // chauffage ISO
    relay.set_low().ok();
    hv_out.set_low().ok();
    iso_out.set_low().ok();

    // XPT2046 touch — bit-bang SPI (GP0=DO, GP1=CS, GP2=CLK, GP3=DIN)
    let mut t_do  = pins.gpio0.into_pull_up_input();
    let mut t_cs  = pins.gpio1.into_push_pull_output();
    let mut t_clk = pins.gpio2.into_push_pull_output();
    let mut t_din = pins.gpio3.into_push_pull_output();
    t_cs.set_high().ok();
    t_clk.set_low().ok();
    t_din.set_low().ok();

    // DS18B20 — bus unique GP15, pull-up 4.7 kΩ externe
    let _ow = pins.gpio15.into_push_pull_output();
    unsafe {
        let sio = &*pac::SIO::ptr();
        sio.gpio_out_clr().write(|w| w.bits(OW_MASK));
        sio.gpio_oe_clr().write(|w| w.bits(OW_MASK)); // high-Z = idle open-drain
    }
    // Configurer résolution 12-bit sur tous (SKIP ROM = broadcast)
    if ow_reset(&timer, OW_MASK) {
        ow_write_byte(&timer, OW_MASK, 0xCC); // SKIP ROM
        ow_write_byte(&timer, OW_MASK, 0x4E); // WRITE SCRATCHPAD
        ow_write_byte(&timer, OW_MASK, 0x55); // TH
        ow_write_byte(&timer, OW_MASK, 0x05); // TL
        ow_write_byte(&timer, OW_MASK, 0x7F); // 12-bit (0.0625 °C, conversion 750 ms)
    }
    // ROM codes découverts après USB ready (section plus bas)
    let mut rom_codes = [[0u8; 8]; 5];
    let mut rom_count = 0usize;

    // Récupération bus I²C avant init BME280
    unsafe {
        let sio = &*pac::SIO::ptr();
        const SDA: u32 = 1 << 4; const SCL: u32 = 1 << 5;
        sio.gpio_out_set().write(|w| w.bits(SDA | SCL));
        sio.gpio_oe_set().write(|w|  w.bits(SDA | SCL));
        ow_wait(&timer, 10);
        for _ in 0..9u8 {
            sio.gpio_out_clr().write(|w| w.bits(SCL)); ow_wait(&timer, 5);
            sio.gpio_out_set().write(|w| w.bits(SCL)); ow_wait(&timer, 5);
        }
        sio.gpio_out_clr().write(|w| w.bits(SDA)); ow_wait(&timer, 5);
        sio.gpio_out_set().write(|w| w.bits(SDA)); ow_wait(&timer, 10);
        sio.gpio_oe_clr().write(|w| w.bits(SDA | SCL)); ow_wait(&timer, 100);
    }

    use rp2040_hal::fugit::RateExtU32;
    // Pull-ups INTERNES sur SDA/SCL : les pull-ups du bus sont sur le module
    // BME280 — s'il est débranché à chaud, le bus flotte et le driver I²C
    // (bloquant, sans timeout) gèle la boucle → USB mort + reboot watchdog.
    // Avec les pull-ups internes, un bus vide répond NACK immédiatement.
    let sda_pin: hal::gpio::Pin<_, hal::gpio::FunctionI2C, hal::gpio::PullUp> =
        pins.gpio4.reconfigure();
    let scl_pin: hal::gpio::Pin<_, hal::gpio::FunctionI2C, hal::gpio::PullUp> =
        pins.gpio5.reconfigure();
    let i2c = hal::I2C::new_controller(
        pac.I2C0,
        sda_pin,
        scl_pin,
        100u32.kHz(), &mut pac.RESETS, clocks.system_clock.freq(),
    );
    let mut bme    = Bme280Driver::new(i2c);
    let mut bme_ok = false;
    for _ in 0..3u8 {
        if bme.init().is_ok() { bme_ok = true; break; }
        keepalive(&timer, 500, &mut usb_dev, &mut serial);
    }

    // Structures de contrôle
    let mut state       = SystemState::new();
    let mut target      = TargetState::default();
    let mut controller  = Controller::new();
    let mut history     = MeasurementHistory::new();
    let mut last_output = ControlOutput::emergency_stop();
    let mut cmd_buf: String<64> = String::new();

    usb_write(&timer, &mut usb_dev, &mut serial,b"READY chambre-a-brouillard\r\n");
    usb_write(&timer, &mut usb_dev, &mut serial,if bme_ok { b"INFO BME280 OK\r\n" } else { b"WARN BME280 absent\r\n" });

    // Diagnostic : état du bus au repos (doit être HIGH grâce au pull-up)
    unsafe { (&*pac::SIO::ptr()).gpio_oe_clr().write(|w| w.bits(OW_MASK)); }
    ow_wait(&timer, 500);
    let bus_idle_high = unsafe { (&*pac::SIO::ptr()).gpio_in().read().bits() & OW_MASK != 0 };
    usb_write(&timer, &mut usb_dev, &mut serial,if bus_idle_high {
        b"INFO 1W idle HIGH (pull-up OK)\r\n"
    } else {
        b"WARN 1W idle LOW - pull-up absent ou court-circuit GND\r\n"
    });

    // Diagnostic : READ ROM (commande 0x33 — valide uniquement si 1 capteur)
    if bus_idle_high && ow_reset(&timer, OW_MASK) {
        ow_write_byte(&timer, OW_MASK, 0x33); // READ ROM
        let mut rr = [0u8; 8];
        for b in rr.iter_mut() { *b = ow_read_byte(&timer, OW_MASK); }
        let mut s: String<80> = String::new();
        let _ = write!(s, "INFO READ_ROM {:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X} crc={}\r\n",
            rr[0],rr[1],rr[2],rr[3],rr[4],rr[5],rr[6],rr[7], crc8(&rr));
        usb_write(&timer, &mut usb_dev, &mut serial,s.as_bytes());
    }

    // SEARCH ROM ici — USB connecté, diagnostics visibles dans le terminal
    if bus_idle_high && ow_reset(&timer, OW_MASK) {
        usb_write(&timer, &mut usb_dev, &mut serial,b"INFO 1W presence OK\r\n");
        for _pass in 0..3u8 {
            let mut searcher  = OwSearch::new();
            let mut pass_roms = [[0u8; 8]; 5];
            let mut pass_count = 0usize;
            loop {
                keepalive(&timer, 1, &mut usb_dev, &mut serial);
                match searcher.next(&timer, OW_MASK) {
                    Some(rom) if crc8(&rom) == 0 => {
                        pass_roms[pass_count] = rom;
                        pass_count += 1;
                        if pass_count >= 5 { break; }
                    }
                    Some(_) => break, // CRC corrompu — recommencer le pass
                    None    => break, // plus de capteurs
                }
            }
            if pass_count > rom_count {
                rom_count = pass_count;
                rom_codes = pass_roms;
            }
            if rom_count >= 5 { break; }
            keepalive(&timer, 200, &mut usb_dev, &mut serial);
        }
    } else if bus_idle_high {
        usb_write(&timer, &mut usb_dev, &mut serial,b"WARN 1W no presence - capteur absent ou VCC/GND inverses\r\n");
    }
    // Rapport ROM codes — identifie chaque slot
    {
        let mut info: String<48> = String::new();
        let _ = write!(info, "INFO DS18B20 count={}\r\n", rom_count);
        usb_write(&timer, &mut usb_dev, &mut serial,info.as_bytes());
    }
    for i in 0..rom_count {
        let r = rom_codes[i];
        let mut info: String<64> = String::new();
        let _ = write!(info, "INFO ds{} {:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}\r\n",
                       i, r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7]);
        usb_write(&timer, &mut usb_dev, &mut serial,info.as_bytes());
    }

    // Initialisation écran TFT
    let spi_dev = ExclusiveDevice::new_no_delay(spi1, disp_cs).unwrap();
    let mut spi_tx_buf = [0u8; 512];
    let di = SpiInterface::new(spi_dev, disp_dc, &mut spi_tx_buf);
    // KMRTM28028-SPI = contrôleur ILI9341 (pas ST7789). Les modules ILI9341
    // câblent la matrice en BGR, d'où color_order — inverser si les couleurs
    // apparaissent échangées (rouge ↔ bleu).
    let mut disp_opt = Builder::new(ILI9341Rgb565, di)
        .reset_pin(disp_rst)
        .display_size(240, 320)
        .color_order(ColorOrder::Bgr)
        // flip_horizontal() annule l'effet miroir gauche-droite du panneau.
        // Si l'image devient à l'envers (haut-bas), remplacer Deg180 par Deg0.
        .orientation(Orientation::new().rotate(Rotation::Deg180).flip_horizontal())
        .init(&mut CortexDelay)
        .ok();
    if let Some(d) = disp_opt.as_mut() {
        display::draw_static(d, state.compressor_allowed, false);
        usb_write(&timer, &mut usb_dev, &mut serial, b"INFO display ILI9341 OK\r\n");
    } else {
        usb_write(&timer, &mut usb_dev, &mut serial, b"WARN display init failed\r\n");
    }
    // États affichés sur les boutons — pour redessiner uniquement quand ils
    // changent (p. ex. commande COMP/CYCLE reçue par USB, transition auto).
    let mut btn_shown_allowed = state.compressor_allowed;
    let mut btn_shown_cycle   = false;
    let mut last_safety_logged = false;

    // Watchdog — si la boucle principale fige plus de 4 s, le RP2040 redémarre
    // et toutes les sorties (relais, HV, chauffage) repartent LOW (fail-safe).
    watchdog.start(hal::fugit::MicrosDurationU32::secs(4));

    let t0 = timer.get_counter().ticks();
    let mut last_touch_ms  = 0u64;
    let mut touch_down     = false;
    let mut last_ds_start_ms  = 0u64;
    let mut ds_reading_phase  = false;
    let mut last_bme_ms      = 0u64;
    let mut bme_fail: u8     = 0;
    let mut last_bme_init_ms = 0u64;
    let mut last_pub_ms  = 0u64;
    let mut last_ctrl_ms = timer.get_counter().ticks() / 1_000;
    let mut last_disp_ms = 0u64;
    let mut btn_flash_until_ms: u64 = 0; // 0 = pas de flash actif
    // Rescan 1-Wire périodique — détecte les capteurs branchés/débranchés à chaud.
    let mut last_scan_ms = timer.get_counter().ticks() / 1_000;

    loop {
        watchdog.feed();
        let now_ms = timer.get_counter().ticks() / 1_000;

        // USB — poll à chaque itération
        if usb_dev.poll(&mut [&mut serial]) {
            let mut buf = [0u8; 64];
            if let Ok(n) = serial.read(&mut buf) {
                for &b in &buf[..n] {
                    if b == b'\n' || b == b'\r' {
                        if !cmd_buf.is_empty() {
                            handle_command(cmd_buf.as_str(), &mut target, &mut state, &mut controller, &timer, &mut usb_dev, &mut serial);
                            cmd_buf.clear();
                        }
                    } else if b >= 0x20 {
                        cmd_buf.push(b as char).ok();
                    }
                }
            }
        }

        // Touch XPT2046 — toutes les 50 ms (anti-rebond via touch_down)
        if now_ms.saturating_sub(last_touch_ms) >= 50 {
            last_touch_ms = now_ms;

            // ── Diagnostic brut — retirer quand la calibration est validée ──────
            let (z1, rx_raw, ry_raw) = touch_raw(&mut t_clk, &mut t_din, &mut t_do, &mut t_cs);
            if z1 > 500 {
                let (sx, sy) = display::touch_to_screen(rx_raw, ry_raw);
                let mut dbg: String<64> = String::new();
                let _ = write!(dbg, "TOUCH z1={} raw={},{} px={},{}\r\n",
                               z1, rx_raw, ry_raw, sx, sy);
                usb_write(&timer, &mut usb_dev, &mut serial, dbg.as_bytes());
            }
            // ────────────────────────────────────────────────────────────────────

            match touch_read(&mut t_clk, &mut t_din, &mut t_do, &mut t_cs) {
                Some((rx, ry)) if !touch_down => {
                    touch_down = true;
                    let (sx, sy) = display::touch_to_screen(rx, ry);
                    if display::is_btn_cycle(sx, sy) {
                        if controller.phase_code() == 0 {
                            if controller.is_tripped() {
                                // Disjoncteur verrouillé : 1er appui = réarmement
                                // (request_stop réarme le moniteur, même en Idle).
                                controller.request_stop(now_ms);
                                usb_write(&timer, &mut usb_dev, &mut serial,
                                          b"CMD REARM touch\r\n");
                            } else {
                                // Lancer la séquence automatique
                                state.compressor_allowed = true;
                                if controller.request_start(now_ms) {
                                    if let Some(d) = disp_opt.as_mut() {
                                        display::draw_btn_cycle_flash(d, true);
                                    }
                                    btn_flash_until_ms = now_ms + 250;
                                    usb_write(&timer, &mut usb_dev, &mut serial, b"CMD CYCLE touch=1\r\n");
                                }
                            }
                        } else {
                            // Cycle en cours → arrêt propre
                            controller.request_stop(now_ms);
                            if let Some(d) = disp_opt.as_mut() {
                                display::draw_btn_cycle_flash(d, false);
                            }
                            btn_flash_until_ms = now_ms + 250;
                            usb_write(&timer, &mut usb_dev, &mut serial, b"CMD CYCLE touch=0\r\n");
                        }
                    } else if display::is_btn_comp(sx, sy) {
                        // Bascule autorisation compresseur (MARCHE ⇆ ARRÊT)
                        state.compressor_allowed = !state.compressor_allowed;
                        if !state.compressor_allowed {
                            target.high_voltage_enabled = false; // l'arrêt coupe aussi le HV
                        }
                        if let Some(d) = disp_opt.as_mut() {
                            display::draw_btn_comp_flash(d, state.compressor_allowed);
                        }
                        btn_flash_until_ms = now_ms + 250;
                        usb_write(&timer, &mut usb_dev, &mut serial,
                            if state.compressor_allowed { b"CMD COMP touch=1\r\n" }
                            else                        { b"CMD COMP touch=0\r\n" });
                    } else if display::is_btn_reset(sx, sy) {
                        if let Some(d) = disp_opt.as_mut() {
                            display::draw_btn_reset_flash(d);
                        }
                        CortexDelay.delay_ms(200);
                        cortex_m::peripheral::SCB::sys_reset();
                    }
                }
                Some(_) => { touch_down = true; }
                None    => { touch_down = false; }
            }
        }

        // DS18B20 — Phase 1 : lancer la conversion (non-bloquant), 1 Hz
        if !ds_reading_phase && now_ms.saturating_sub(last_ds_start_ms) >= 1_000 {
            if ow_reset(&timer, OW_MASK) {
                ow_write_byte(&timer, OW_MASK, 0xCC); // SKIP ROM
                ow_write_byte(&timer, OW_MASK, 0x44); // CONVERT T
            }
            ds_reading_phase  = true;
            last_ds_start_ms  = now_ms;
        }

        // DS18B20 — Phase 2 : lecture après 750 ms (conversion 12-bit terminée)
        if ds_reading_phase && now_ms.saturating_sub(last_ds_start_ms) >= 750 {
            for idx in 0..5usize {
                // La lecture des 5 capteurs bloque ~40 ms au total : on draine
                // l'USB entre chaque capteur pour ne pas perdre de commandes.
                usb_dev.poll(&mut [&mut serial]);
                if idx >= rom_count {
                    state.temperatures[idx].valid = false;
                    continue;
                }
                let rom = rom_codes[idx];
                let mut val: Option<f32> = None;
                'retry: for _ in 0..2u8 {
                    if !ow_reset(&timer, OW_MASK) { break 'retry; }
                    ow_write_byte(&timer, OW_MASK, 0x55); // MATCH ROM
                    for &b in &rom { ow_write_byte(&timer, OW_MASK, b); }
                    ow_write_byte(&timer, OW_MASK, 0xBE); // READ SCRATCHPAD
                    let mut sp = [0u8; 9];
                    for b in sp.iter_mut() { *b = ow_read_byte(&timer, OW_MASK); }
                    if crc8(&sp) == 0 {
                        let raw = (sp[0] as u16) | ((sp[1] as u16) << 8);
                        val = Some(raw as i16 as f32 / 16.0);
                        break 'retry;
                    }
                }
                state.temperatures[idx].valid = val.is_some();
                if let Some(t) = val { state.temperatures[idx].value = t; }
            }
            ds_reading_phase = false;
        }

        // BME280 — mesure toutes les 500 ms. Hot-plug géré : après 4 échecs
        // consécutifs (~2 s) le capteur est déclaré perdu, puis une ré-init
        // est tentée toutes les 5 s — le rebranchement à chaud refonctionne
        // sans reset (l'init reconfigure les registres du capteur).
        if now_ms.saturating_sub(last_bme_ms) >= 500 {
            if bme_ok {
                match bme.measure(&mut CortexDelay) {
                    Ok((t, p, h)) => {
                        state.bme280.temp_c = t; state.bme280.pressure_hpa = p;
                        state.bme280.humidity_pct = h; state.bme280.valid = true;
                        bme_fail = 0;
                    }
                    Err(_) => {
                        state.bme280.valid = false;
                        bme_fail = bme_fail.saturating_add(1);
                        if bme_fail >= 4 {
                            bme_ok = false;
                            usb_write(&timer, &mut usb_dev, &mut serial,
                                      b"WARN BME280 perdu - re-init auto active\r\n");
                        }
                    }
                }
            } else if now_ms.saturating_sub(last_bme_init_ms) >= 5_000 {
                last_bme_init_ms = now_ms;
                if bme.init().is_ok() {
                    bme_ok   = true;
                    bme_fail = 0;
                    usb_write(&timer, &mut usb_dev, &mut serial,
                              b"INFO BME280 reconnecte\r\n");
                }
            }
            last_bme_ms = now_ms;
        }

        // Rescan 1-Wire : détecte les nouveaux capteurs branchés à chaud.
        // On n'actualise QUE si on trouve plus de capteurs qu'avant : une erreur CRC
        // partielle retourne un count faible mais ne doit pas invalider les lectures
        // en cours. Le retrait d'un capteur est géré par Phase 2 (MATCH ROM échoue
        // naturellement → valid = false), pas par le rescan.
        let scan_interval = if rom_count < 5 { 3_000u64 } else { 60_000u64 };
        if !ds_reading_phase && now_ms.saturating_sub(last_scan_ms) >= scan_interval {
            let mut new_roms  = [[0u8; 8]; 5];
            let mut new_count = 0usize;
            let mut searcher  = OwSearch::new();
            loop {
                usb_dev.poll(&mut [&mut serial]);
                match searcher.next(&timer, OW_MASK) {
                    Some(rom) if crc8(&rom) == 0 => {
                        new_roms[new_count] = rom;
                        new_count += 1;
                        if new_count >= 5 { break; }
                    }
                    _ => break,
                }
            }
            if new_count > rom_count {
                rom_count = new_count;
                rom_codes = new_roms;
                for i in 0..5 { state.temperatures[i].valid = false; }
                let mut info: String<32> = String::new();
                let _ = write!(info, "INFO DS rescan count={}\r\n", rom_count);
                usb_write(&timer, &mut usb_dev, &mut serial, info.as_bytes());
            }
            last_scan_ms = now_ms;
        }

        // Boucle de contrôle — toutes les 100 ms
        if now_ms.saturating_sub(last_ctrl_ms) >= 100 {
            let dt_s = (now_ms - last_ctrl_ms).min(2_000) as f32 / 1_000.0;
            history.update(&state, now_ms); // 1 échantillon/s max (gate interne)
            let phase_before = controller.phase_code();
            last_output  = controller.tick(&mut state, &history, &target, now_ms, dt_s);
            // Trace le déclenchement/réarmement du disjoncteur.
            if last_output.safety_override != last_safety_logged {
                usb_write(&timer, &mut usb_dev, &mut serial,
                    if last_output.safety_override {
                        b"WARN DISJONCTEUR declenche - CYCLE 0 pour rearmer\r\n".as_slice()
                    } else {
                        b"INFO DISJONCTEUR rearme\r\n".as_slice()
                    });
                last_safety_logged = last_output.safety_override;
            }
            // Trace chaque transition de phase sur l'USB — visible dans le
            // Journal de l'UI (abandons de phase inclus).
            if controller.phase_code() != phase_before {
                let mut info: String<48> = String::new();
                let _ = write!(info, "INFO PHASE {} -> {} ({})\r\n",
                    phase_before, controller.phase_code(),
                    controller.phase_label().unwrap_or("Idle/manuel"));
                usb_write(&timer, &mut usb_dev, &mut serial, info.as_bytes());
            }
            if last_output.compressor     { relay.set_high().ok();   } else { relay.set_low().ok();   }
            if last_output.high_voltage   { hv_out.set_high().ok();  } else { hv_out.set_low().ok();  }
            if last_output.isopropanol_heater_duty > 0.0 { iso_out.set_high().ok(); } else { iso_out.set_low().ok(); }
            state.cycle_count += 1;
            state.uptime_s     = (timer.get_counter().ticks() - t0) / 1_000_000;
            last_ctrl_ms       = now_ms;
        }

        // Flash tactile — restaure les boutons (dans leur nouvel état) après 250 ms
        if btn_flash_until_ms > 0 && now_ms >= btn_flash_until_ms {
            if let Some(d) = disp_opt.as_mut() {
                display::draw_btn_comp(d, state.compressor_allowed);
                display::draw_btn_cycle(d, controller.phase_code() != 0);
            }
            btn_shown_allowed  = state.compressor_allowed;
            btn_shown_cycle    = controller.phase_code() != 0;
            btn_flash_until_ms = 0;
        }

        // Écran TFT — toutes les 500 ms
        if now_ms.saturating_sub(last_disp_ms) >= 500 {
            if let Some(d) = disp_opt.as_mut() {
                display::draw(d, &state, &target, &last_output, rom_count,
                              controller.phase_label());
                // Redessine les boutons si leur état a changé ailleurs que par
                // le tactile (commande USB, transition automatique de phase).
                if btn_flash_until_ms == 0 && btn_shown_allowed != state.compressor_allowed {
                    display::draw_btn_comp(d, state.compressor_allowed);
                    btn_shown_allowed = state.compressor_allowed;
                }
                let cyc_active = controller.phase_code() != 0;
                if btn_flash_until_ms == 0 && btn_shown_cycle != cyc_active {
                    display::draw_btn_cycle(d, cyc_active);
                    btn_shown_cycle = cyc_active;
                }
            }
            last_disp_ms = now_ms;
        }

        // Publication état — toutes les 200 ms (5 Hz)
        if now_ms.saturating_sub(last_pub_ms) >= 200 {
            publish_state(&timer, &mut usb_dev, &mut serial, &state, &last_output, &target,
                          controller.phase_code(), state.uptime_s);
            last_pub_ms = now_ms;
        }
    }
}
