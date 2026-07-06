// Affichage TFT ILI9341 240×320 (portrait) — module KMRTM28028-SPI.


use core::fmt::Write as _;
use heapless::String;

use embedded_graphics::{
    mono_font::{
        ascii::{FONT_6X10, FONT_6X13, FONT_9X18_BOLD, FONT_10X20},
        MonoFont, MonoTextStyleBuilder,
    },
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyleBuilder, Rectangle},
    text::{Baseline, Text},
};

use crate::{
    control::{output::ControlOutput, target::TargetState},
    data::SystemState,
};

const W: i32 = 240;
const H: i32 = 320;

const DS_LABELS: [&str; 4] = [
    "Compresseur",
    "Condenseur ",
    "Entree evap",
    "Sortie evap",
];

// Palette
const BG:  Rgb565 = Rgb565::BLACK;
const WH:  Rgb565 = Rgb565::WHITE;
const CY:  Rgb565 = Rgb565::CYAN;
const GR:  Rgb565 = Rgb565::GREEN;
const RD:  Rgb565 = Rgb565::RED;
const YL:  Rgb565 = Rgb565::YELLOW;
const DIM: Rgb565 = Rgb565::new(8, 16, 8);

// Disposition verticale (portrait 240×320)
const Y_HDR:    i32 = 2;    // "CHAMBRE" + statut + uptime
const Y_DS:     i32 = 38;   // 4 lignes circuit, pas de 20 px
const DS_STEP:  i32 = 20;
const Y_BASE:   i32 = 140;  // grande valeur base chambre
const Y_BME:    i32 = 188;  // ligne T / P / H
const Y_CTRL:   i32 = 226;  // COMP / HV
const Y_CIBLE:  i32 = 246;  // consigne

/// Fond et éléments fixes — à appeler une seule fois après l'init de l'écran.
pub fn draw_static<D: DrawTarget<Color = Rgb565>>(disp: &mut D) {
    fill(disp, 0, 0, W as u32, H as u32, BG);

    txt(disp, "CHAMBRE", 4, Y_HDR, &FONT_9X18_BOLD, WH);
    fill(disp, 0, 22, 240, 1, CY);

    txt(disp, "CIRCUIT DE REFROIDISSEMENT", 4, 26, &FONT_6X10, CY);
    for (i, label) in DS_LABELS.iter().enumerate() {
        txt(disp, label, 4, Y_DS + DS_STEP * i as i32 + 3, &FONT_6X10, DIM);
    }

    fill(disp, 0, 120, 240, 1, DIM);
    txt(disp, "BASE CHAMBRE", 4, 126, &FONT_6X10, CY);

    fill(disp, 0, 168, 240, 1, DIM);
    txt(disp, "AMBIANCE", 4, 174, &FONT_6X10, CY);

    fill(disp, 0, 206, 240, 1, DIM);
    txt(disp, "CONTROLE", 4, 212, &FONT_6X10, CY);
}

/// Mise à jour des valeurs uniquement. Appelé toutes les 500 ms.
pub fn draw<D: DrawTarget<Color = Rgb565>>(
    disp: &mut D,
    state: &SystemState,
    target: &TargetState,
    out: &ControlOutput,
    rom_count: usize,
) {
    // ─── Statut + uptime (header) ────────────────────────────────────────────
    if out.safety_override {
        txt(disp, "! ERR !", 152, Y_HDR, &FONT_9X18_BOLD, RD);
    } else {
        txt(disp, "  SAFE ", 152, Y_HDR, &FONT_9X18_BOLD, GR);
    }
    {
        let mut s: String<12> = String::new();
        let up = state.uptime_s;
        if up < 3600 { write!(s, "t={:4}s", up).ok(); }
        else { write!(s, "t={:2}h{:02}m", up / 3600, (up % 3600) / 60).ok(); }
        while s.len() < 8 { s.push(' ').ok(); }
        txt(disp, s.as_str(), 82, Y_HDR + 4, &FONT_6X10, DIM);
    }

    // ─── Circuit de refroidissement (4 × DS) ─────────────────────────────────
    for i in 0..4usize {
        let (val, col) = fmt_temp::<9>(state, i, rom_count);
        txt(disp, val.as_str(), 142, Y_DS + DS_STEP * i as i32, &FONT_6X13, col);
    }

    // ─── Base chambre (ds4) — grande valeur ──────────────────────────────────
    {
        let col;
        let mut val: String<12> = String::new();
        if 4 < rom_count && state.temperatures[4].valid {
            let t = state.temperatures[4].value;
            write!(val, "{:+7.1}C", t).ok();
            col = if t <= -35.0 { CY } else if t < -20.0 { WH } else { YL };
        } else {
            write!(val, "  ---   ").ok();
            col = DIM;
        }
        txt(disp, val.as_str(), 40, Y_BASE, &FONT_10X20, col);
    }

    // ─── Ambiance BME280 ─────────────────────────────────────────────────────
    {
        let mut s: String<40> = String::new();
        if state.bme280.valid {
            write!(s, "T {:5.1}C  P {:6.1}hPa  H {:3.0}%",
                state.bme280.temp_c,
                state.bme280.pressure_hpa,
                state.bme280.humidity_pct).ok();
        } else {
            write!(s, "BME280 absent").ok();
        }
        while s.len() < 34 { s.push(' ').ok(); }
        txt(disp, s.as_str(), 4, Y_BME, &FONT_6X10, if state.bme280.valid { WH } else { DIM });
    }

    // ─── Contrôle ────────────────────────────────────────────────────────────
    let (comp_txt, comp_col) = if out.compressor { ("COMP: ON ", GR) } else { ("COMP: OFF", DIM) };
    txt(disp, comp_txt, 4, Y_CTRL, &FONT_6X13, comp_col);

    let (hv_txt, hv_col) = if out.high_voltage { ("HV: ON ", YL) } else { ("HV: OFF", DIM) };
    txt(disp, hv_txt, 136, Y_CTRL, &FONT_6X13, hv_col);

    {
        let mut s: String<20> = String::new();
        write!(s, "Cible: {:+6.1} C", target.chamber_temp_c).ok();
        txt(disp, s.as_str(), 4, Y_CIBLE, &FONT_6X13, WH);
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Valeur DS formatée à largeur fixe (8 caractères) + couleur selon la plage.
fn fmt_temp<const N: usize>(
    state: &SystemState, idx: usize, rom_count: usize,
) -> (String<N>, Rgb565) {
    let mut val: String<N> = String::new();
    if idx < rom_count && state.temperatures[idx].valid {
        let t = state.temperatures[idx].value;
        write!(val, "{:+6.1}C ", t).ok();
        let col = if t < -20.0 { CY } else if t > 80.0 { RD } else { WH };
        (val, col)
    } else {
        write!(val, "  ---   ").ok();
        (val, DIM)
    }
}

fn fill<D: DrawTarget<Color = Rgb565>>(d: &mut D, x: u32, y: u32, w: u32, h: u32, col: Rgb565) {
    Rectangle::new(Point::new(x as i32, y as i32), Size::new(w, h))
        .into_styled(PrimitiveStyleBuilder::new().fill_color(col).build())
        .draw(d).ok();
}

fn txt<D: DrawTarget<Color = Rgb565>>(
    d: &mut D, s: &str, x: i32, y: i32,
    font: &MonoFont<'_>, fg: Rgb565,
) {
    let style = MonoTextStyleBuilder::new()
        .font(font).text_color(fg).background_color(BG).build();
    Text::with_baseline(s, Point::new(x, y), style, Baseline::Top).draw(d).ok();
}
