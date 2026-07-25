//! Protocole série : commandes reçues de l'hôte et publication de l'état.
//!
//! Commandes acceptées : `CYCLE 0|1`, `TARGET <float>`, `HV 0|1`, `COMP 0|1`.
//! L'état est publié sous forme d'une ligne `STATE ...` (cf. [`publish_state`]),
//! consommée par les scripts Python d'acquisition et l'UI.

use core::fmt::Write as _;
use heapless::String;

use rp2040_hal as hal;
use usb_device::prelude::UsbDevice;

use crate::control::{controller::Controller, output::ControlOutput, target::TargetState};
use crate::data::SystemState;

use super::usb::{usb_write, Serial};

pub fn parse_f32(s: &str) -> Option<f32> {
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
pub fn handle_command(
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
pub fn publish_state(
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
