//! Protocole série : commandes reçues de l'hôte.
//!
//! Commande supportée : `CYCLE 0|1` (démarrage/arrêt du cycle automatique ;
//! `CYCLE 0` réarme aussi le disjoncteur s'il est déclenché). `TARGET`/`HV`/
//! `COMP` dépendent d'une consigne opérateur et d'un mode manuel pas encore
//! construits sur cette branche — retournent `ERR ... pas encore
//! supporte` plutôt que d'être simulés silencieusement.

use rp2040_hal as hal;
use usb_device::prelude::UsbDevice;

use crate::logic::control_loop::{PhaseClock, request_start, request_stop};
use crate::logic::security::SafetyMonitor;

use super::usb::{Serial, usb_write};

/// Parseur de flottant minimal (évite de tirer une crate de parsing float
/// complète pour un protocole texte simple).
///
/// Pas encore appelé : réservé pour `TARGET`, pas encore implémenté (cf.
/// `handle_command` — dépend d'une consigne opérateur qui n'existe pas
/// encore sur cette branche).
#[allow(dead_code)]
pub fn parse_f32(s: &str) -> Option<f32> {
    let s = s.trim();
    let (neg, s) = if s.starts_with('-') {
        (true, &s[1..])
    } else {
        (false, s)
    };
    let mut int_p: f32 = 0.0;
    let mut frac_p: f32 = 0.0;
    let mut frac_div: f32 = 1.0;
    let mut in_frac = false;
    if s.is_empty() {
        return None;
    }
    for c in s.chars() {
        match c {
            '0'..='9' => {
                let d = (c as u8 - b'0') as f32;
                if in_frac {
                    frac_div *= 10.0;
                    frac_p += d / frac_div;
                } else {
                    int_p = int_p * 10.0 + d;
                }
            }
            '.' => {
                if in_frac {
                    return None;
                }
                in_frac = true;
            }
            _ => return None,
        }
    }
    let v = int_p + frac_p;
    Some(if neg { -v } else { v })
}

pub fn handle_command(
    cmd: &str,
    clock: &mut PhaseClock,
    safety: &mut SafetyMonitor,
    timer: &hal::Timer,
    usb_dev: &mut UsbDevice<hal::usb::UsbBus>,
    serial: &mut Serial<'_>,
) {
    let cmd = cmd.trim();
    let (name, rest) = cmd.split_once(' ').unwrap_or((cmd, ""));
    let rest = rest.trim();
    let now_ms = timer.get_counter().ticks() / 1_000;

    match name {
        "CYCLE" => match rest {
            "1" => {
                if request_start(clock, safety, now_ms) {
                    usb_write(timer, usb_dev, serial, b"OK CYCLE=1\r\n");
                } else if safety.is_tripped() {
                    usb_write(
                        timer,
                        usb_dev,
                        serial,
                        b"ERR CYCLE refuse - disjoncteur verrouille (CYCLE 0 pour rearmer)\r\n",
                    );
                } else {
                    usb_write(
                        timer,
                        usb_dev,
                        serial,
                        b"ERR CYCLE refuse - cycle deja en cours\r\n",
                    );
                }
            }
            "0" => {
                if request_stop(clock, safety, now_ms) {
                    usb_write(timer, usb_dev, serial, b"OK CYCLE=0\r\n");
                } else {
                    usb_write(timer, usb_dev, serial, b"OK CYCLE=0 (deja inactif)\r\n");
                }
            }
            _ => usb_write(timer, usb_dev, serial, b"ERR CYCLE needs 0 or 1\r\n"),
        },
        "TARGET" => usb_write(
            timer,
            usb_dev,
            serial,
            b"ERR TARGET pas encore supporte\r\n",
        ),
        "HV" => usb_write(timer, usb_dev, serial, b"ERR HV pas encore supporte\r\n"),
        "COMP" => usb_write(timer, usb_dev, serial, b"ERR COMP pas encore supporte\r\n"),
        _ => usb_write(timer, usb_dev, serial, b"ERR unknown command\r\n"),
    }
}
