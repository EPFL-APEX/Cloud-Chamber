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
//! Publications (Pico → PC, toutes les secondes) :
//!   STATE ds=<T> bme_t=<T> bme_p=<P> bme_h=<H>
//!         target=<T> comp=<0|1> hv=<0|1> iso=<0.00> safe=<0|1> up=<s>

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
use heapless::String;

use cloud_chamber_firmware::{
    control::{controller::Controller, output::ControlOutput, target::TargetState},
    data::SystemState,
    sensors::bme280::Bme280Driver,
    display,
};

use mipidsi::{Builder, interface::SpiInterface, models::ST7789};
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
fn usb_write(serial: &mut SerialPort<hal::usb::UsbBus>, data: &[u8]) {
    let mut pos = 0;
    while pos < data.len() {
        match serial.write(&data[pos..]) { Ok(n) => pos += n, Err(_) => break }
    }
}

fn keepalive(timer: &hal::Timer, ms: u64,
             usb_dev: &mut UsbDevice<hal::usb::UsbBus>,
             serial:  &mut SerialPort<hal::usb::UsbBus>) {
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
    cmd:    &str,
    target: &mut TargetState,
    state:  &mut SystemState,
    serial: &mut SerialPort<hal::usb::UsbBus>,
) {
    let cmd  = cmd.trim();
    let (name, rest) = cmd.split_once(' ').unwrap_or((cmd, ""));
    let rest = rest.trim();

    match name {
        "TARGET" => match parse_f32(rest) {
            Some(v) => {
                target.chamber_temp_c = v;
                let mut r: String<32> = String::new();
                let neg = v < 0.0; let abs = if neg { -v } else { v };
                let _ = write!(r, "OK TARGET={}{}.{}\r\n",
                    if neg {"-"} else {""}, abs as i32, ((abs % 1.0)*10.0) as u32);
                usb_write(serial, r.as_bytes());
            }
            None => usb_write(serial, b"ERR TARGET needs a float\r\n"),
        },
        "HV" => match rest {
            "1" => { target.high_voltage_enabled = true;  usb_write(serial, b"OK HV=1\r\n"); }
            "0" => { target.high_voltage_enabled = false; usb_write(serial, b"OK HV=0\r\n"); }
            _   => usb_write(serial, b"ERR HV needs 0 or 1\r\n"),
        },
        "COMP" => match rest {
            "1" => { state.compressor_allowed = true;  usb_write(serial, b"OK COMP=1\r\n"); }
            "0" => { state.compressor_allowed = false; usb_write(serial, b"OK COMP=0\r\n"); }
            _   => usb_write(serial, b"ERR COMP needs 0 or 1\r\n"),
        },
        _ => usb_write(serial, b"ERR unknown command\r\n"),
    }
}

// ── Publication d'état ────────────────────────────────────────────────────────
fn publish_state(
    serial:   &mut SerialPort<hal::usb::UsbBus>,
    state:    &SystemState,
    output:   &ControlOutput,
    target:   &TargetState,
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
        let _ = write!(msg, "bme_t={}{}.{:02} bme_p={}.{} bme_h={}.{} ",
            if tn {"-"} else {""}, ta as i32, ((ta%1.0)*100.0) as u32,
            bme.pressure_hpa as u32, ((bme.pressure_hpa%1.0)*10.0) as u32,
            bme.humidity_pct as u32, ((bme.humidity_pct%1.0)*10.0) as u32);
    } else {
        let _ = write!(msg, "bme_t=-- bme_p=-- bme_h=-- ");
    }

    let t = target.chamber_temp_c;
    let tn = t < 0.0; let ta = if tn { -t } else { t };
    let _ = write!(msg, "target={}{}.{} comp={} hv={} iso={}.{:02} safe={} up={}\r\n",
        if tn {"-"} else {""}, ta as i32, ((ta%1.0)*10.0) as u32,
        output.compressor as u8,
        output.high_voltage as u8,
        output.isopropanol_heater_duty as i32,
        ((output.isopropanol_heater_duty%1.0)*100.0) as u32,
        output.safety_override as u8,
        uptime_s);

    usb_write(serial, msg.as_bytes());
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
    let mut serial  = SerialPort::new(usb_alloc);
    let mut usb_dev = UsbDeviceBuilder::new(usb_alloc, UsbVidPid(0x2e8a, 0x0005))
        .device_class(0x02).build();

    // Écran TFT — SPI1 (GP10=SCK, GP11=MOSI, GP12=MISO interne, GP8=DC, GP9=CS, GP7=RST)
    let _disp_mosi = pins.gpio11.into_function::<hal::gpio::FunctionSpi>();
    let _disp_miso = pins.gpio12.into_function::<hal::gpio::FunctionSpi>(); // non câblé
    let _disp_sck  = pins.gpio10.into_function::<hal::gpio::FunctionSpi>();
    let spi1 = hal::Spi::<_, _, _, 8>::new(pac.SPI1, (_disp_mosi, _disp_miso, _disp_sck))
        .init(&mut pac.RESETS, clocks.peripheral_clock.freq(), 10_000_000u32.Hz(), embedded_hal::spi::MODE_3);
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

    // DS18B20 — bus unique GP15, pull-up 4.7 kΩ externe
    let _ow = pins.gpio15.into_push_pull_output();
    unsafe {
        let sio = &*pac::SIO::ptr();
        sio.gpio_out_clr().write(|w| w.bits(OW_MASK));
        sio.gpio_oe_clr().write(|w| w.bits(OW_MASK)); // high-Z = idle open-drain
    }
    // Configurer résolution 9-bit sur tous (SKIP ROM = broadcast)
    if ow_reset(&timer, OW_MASK) {
        ow_write_byte(&timer, OW_MASK, 0xCC); // SKIP ROM
        ow_write_byte(&timer, OW_MASK, 0x4E); // WRITE SCRATCHPAD
        ow_write_byte(&timer, OW_MASK, 0x55); // TH
        ow_write_byte(&timer, OW_MASK, 0x05); // TL
        ow_write_byte(&timer, OW_MASK, 0x1F); // 9-bit
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
    let i2c = hal::I2C::new_controller(
        pac.I2C0,
        pins.gpio4.into_function::<hal::gpio::FunctionI2C>(),
        pins.gpio5.into_function::<hal::gpio::FunctionI2C>(),
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
    let mut last_output = ControlOutput::emergency_stop();
    let mut cmd_buf: String<64> = String::new();

    // Attente connexion USB (3 s)
    keepalive(&timer, 3_000, &mut usb_dev, &mut serial);
    usb_write(&mut serial, b"READY chambre-a-brouillard\r\n");
    usb_write(&mut serial, if bme_ok { b"INFO BME280 OK\r\n" } else { b"WARN BME280 absent\r\n" });

    // Diagnostic : état du bus au repos (doit être HIGH grâce au pull-up)
    unsafe { (&*pac::SIO::ptr()).gpio_oe_clr().write(|w| w.bits(OW_MASK)); }
    ow_wait(&timer, 500);
    let bus_idle_high = unsafe { (&*pac::SIO::ptr()).gpio_in().read().bits() & OW_MASK != 0 };
    usb_write(&mut serial, if bus_idle_high {
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
        usb_write(&mut serial, s.as_bytes());
    }

    // SEARCH ROM ici — USB connecté, diagnostics visibles dans le terminal
    if bus_idle_high && ow_reset(&timer, OW_MASK) {
        usb_write(&mut serial, b"INFO 1W presence OK\r\n");
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
        usb_write(&mut serial, b"WARN 1W no presence - capteur absent ou VCC/GND inverses\r\n");
    }
    // Rapport ROM codes — identifie chaque slot
    {
        let mut info: String<48> = String::new();
        let _ = write!(info, "INFO DS18B20 count={}\r\n", rom_count);
        usb_write(&mut serial, info.as_bytes());
    }
    for i in 0..rom_count {
        let r = rom_codes[i];
        let mut info: String<64> = String::new();
        let _ = write!(info, "INFO ds{} {:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}\r\n",
                       i, r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7]);
        usb_write(&mut serial, info.as_bytes());
    }

    // Initialisation écran TFT
    let spi_dev = ExclusiveDevice::new_no_delay(spi1, disp_cs).unwrap();
    let mut spi_tx_buf = [0u8; 512];
    let di = SpiInterface::new(spi_dev, disp_dc, &mut spi_tx_buf);
    let mut disp_opt = Builder::new(ST7789, di)
        .reset_pin(disp_rst)
        .display_size(240, 320)
        .init(&mut CortexDelay)
        .ok();
    if disp_opt.is_some() {
        usb_write(&mut serial, b"INFO display ILI9341 OK\r\n");
    } else {
        usb_write(&mut serial, b"WARN display init failed\r\n");
    }

    let t0 = timer.get_counter().ticks();
    let mut last_ds_ms   = 0u64;
    let mut last_bme_ms  = 0u64;
    let mut last_pub_ms  = 0u64;
    let mut last_ctrl_ms = timer.get_counter().ticks() / 1_000;
    let mut last_disp_ms = 0u64;

    loop {
        let now_ms = timer.get_counter().ticks() / 1_000;

        // Lecture et accumulation des commandes USB
        if usb_dev.poll(&mut [&mut serial]) {
            let mut buf = [0u8; 64];
            if let Ok(n) = serial.read(&mut buf) {
                for &b in &buf[..n] {
                    if b == b'\n' || b == b'\r' {
                        if !cmd_buf.is_empty() {
                            handle_command(cmd_buf.as_str(), &mut target, &mut state, &mut serial);
                            cmd_buf.clear();
                        }
                    } else if b >= 0x20 {
                        cmd_buf.push(b as char).ok();
                    }
                }
            }
        }

        // DS18B20 — bus unique GP15, ROM addressing
        // CONVERT T via SKIP ROM (broadcast = tous convertissent en parallèle)
        // puis READ SCRATCHPAD via MATCH ROM + ROM code individuel
        if now_ms.saturating_sub(last_ds_ms) >= 200 {
            // 1. CONVERT T — SKIP ROM : broadcast simultané sur tout le bus
            let convert_ok = ow_reset(&timer, OW_MASK) && {
                ow_write_byte(&timer, OW_MASK, 0xCC); // SKIP ROM
                ow_write_byte(&timer, OW_MASK, 0x44); // CONVERT T
                true
            };

            // 2. Attente unique 9-bit (93.75 ms)
            keepalive(&timer, 100, &mut usb_dev, &mut serial);

            // 3. Lecture individuelle via MATCH ROM
            for idx in 0..5usize {
                if !convert_ok || idx >= rom_count {
                    state.temperatures[idx].valid = false;
                    continue;
                }
                let rom = rom_codes[idx];
                let mut val: Option<f32> = None;
                'retry: for _ in 0..4u8 {
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
                    ow_wait(&timer, 1_000);
                }
                state.temperatures[idx].valid = val.is_some();
                if let Some(t) = val { state.temperatures[idx].value = t; }
            }
            last_ds_ms = now_ms;
        }

        // BME280 — toutes les 500 ms
        if bme_ok && now_ms.saturating_sub(last_bme_ms) >= 500 {
            match bme.measure(&mut CortexDelay) {
                Ok((t, p, h)) => {
                    state.bme280.temp_c = t; state.bme280.pressure_hpa = p;
                    state.bme280.humidity_pct = h; state.bme280.valid = true;
                }
                Err(_) => { state.bme280.valid = false; }
            }
            last_bme_ms = now_ms;
        }

        // Boucle de contrôle — toutes les 200 ms
        if now_ms.saturating_sub(last_ctrl_ms) >= 200 {
            let dt_s = (now_ms - last_ctrl_ms).min(2_000) as f32 / 1_000.0;
            last_output  = controller.step(&state, &target, dt_s);
            if last_output.compressor     { relay.set_high().ok();   } else { relay.set_low().ok();   }
            if last_output.high_voltage   { hv_out.set_high().ok();  } else { hv_out.set_low().ok();  }
            if last_output.isopropanol_heater_duty > 0.0 { iso_out.set_high().ok(); } else { iso_out.set_low().ok(); }
            state.cycle_count += 1;
            state.uptime_s     = (timer.get_counter().ticks() - t0) / 1_000_000;
            last_ctrl_ms       = now_ms;
        }

        // Écran TFT — toutes les 500 ms
        if now_ms.saturating_sub(last_disp_ms) >= 500 {
            if let Some(d) = disp_opt.as_mut() {
                display::draw(d, &state, &target, &last_output, rom_count);
            }
            last_disp_ms = now_ms;
        }

        // Publication état — toutes les 1 s
        if now_ms.saturating_sub(last_pub_ms) >= 1_000 {
            publish_state(&mut serial, &state, &last_output, &target, state.uptime_s);
            last_pub_ms = now_ms;
        }
    }
}
