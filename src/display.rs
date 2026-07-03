// Affichage TFT ILI9341 240×320 (portrait) — KMRTM28028-SPI
// Rafraîchi toutes les 500 ms depuis la boucle principale.

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

/// Dessin complet de l'écran.
/// Appelé toutes les 500 ms ; redessine tout avec fond intégré (sans scintillement notable).
pub fn draw<D: DrawTarget<Color = Rgb565>>(
    disp: &mut D,
    state: &SystemState,
    target: &TargetState,
    out: &ControlOutput,
    rom_count: usize,
) {
    // Fond
    fill(disp, 0, 0, W as u32, 320, BG);

    // ─── HEADER ─────────────────────────────────────────────── y=2
    txt(disp, "CHAMBRE", 4, 2, &FONT_9X18_BOLD, WH);
    if out.safety_override {
        txt(disp, "! ERR !", 152, 2, &FONT_9X18_BOLD, RD);
    } else {
        txt(disp, "  SAFE", 152, 2, &FONT_9X18_BOLD, GR);
    }
    {
        let mut s: String<12> = String::new();
        let up = state.uptime_s;
        if up < 3600 { write!(s, "t={:4}s", up).ok(); }
        else { write!(s, "t={:2}h{:02}m", up / 3600, (up % 3600) / 60).ok(); }
        txt(disp, s.as_str(), 82, 6, &FONT_6X10, DIM);
    }
    fill(disp, 0, 22, 240, 1, CY);

    // ─── CIRCUIT DE REFROIDISSEMENT ─────────────────────────── y=26
    txt(disp, "CIRCUIT DE REFROIDISSEMENT", 4, 26, &FONT_6X10, CY);

    let mut y = 38i32;
    for i in 0..4usize {
        txt(disp, DS_LABELS[i], 4, y + 3, &FONT_6X10, DIM);
        let col;
        let mut val: String<12> = String::new();
        if i < rom_count && state.temperatures[i].valid {
            let t = state.temperatures[i].value;
            write!(val, "{:+.1}C ", t).ok();
            col = if t < -20.0 { CY } else if t > 80.0 { RD } else { WH };
        } else {
            write!(val, "  ---  ").ok();
            col = DIM;
        }
        let vs = MonoTextStyleBuilder::new()
            .font(&FONT_6X13).text_color(col).background_color(BG).build();
        Text::with_baseline(val.as_str(), Point::new(142, y), vs, Baseline::Top)
            .draw(disp).ok();
        y += 20;
    }

    fill(disp, 0, y as u32 + 2, 240, 1, DIM);

    // ─── BASE CHAMBRE (ds4) ──────────────────────────────────── y≈122
    y += 8;
    txt(disp, "BASE CHAMBRE", 4, y, &FONT_6X10, CY);
    y += 14;
    {
        let col;
        let mut val: String<12> = String::new();
        if 4 < rom_count && state.temperatures[4].valid {
            let t = state.temperatures[4].value;
            write!(val, "{:+.1}C", t).ok();
            col = if t <= -35.0 { CY } else if t < -20.0 { WH } else { YL };
        } else {
            write!(val, "  ---  ").ok();
            col = DIM;
        }
        let big = MonoTextStyleBuilder::new()
            .font(&FONT_10X20).text_color(col).background_color(BG).build();
        Text::with_baseline(val.as_str(), Point::new(40, y), big, Baseline::Top)
            .draw(disp).ok();
    }
    y += 28;

    fill(disp, 0, y as u32, 240, 1, DIM);
    y += 6;

    // ─── AMBIANCE BME280 ─────────────────────────────────────── y≈168
    txt(disp, "AMBIANCE", 4, y, &FONT_6X10, CY);
    y += 14;
    if state.bme280.valid {
        let mut s: String<32> = String::new();
        write!(s, "T {:.1}C   P {:.0}hPa   H {:.0}%",
            state.bme280.temp_c,
            state.bme280.pressure_hpa,
            state.bme280.humidity_pct).ok();
        txt(disp, s.as_str(), 4, y, &FONT_6X10, WH);
    } else {
        txt(disp, "BME280 absent", 4, y, &FONT_6X10, DIM);
    }
    y += 18;

    fill(disp, 0, y as u32, 240, 1, DIM);
    y += 6;

    // ─── CONTROLE ────────────────────────────────────────────── y≈212
    txt(disp, "CONTROLE", 4, y, &FONT_6X10, CY);
    y += 14;

    let (comp_txt, comp_col) = if out.compressor { ("COMP: ON ", GR) } else { ("COMP: OFF", DIM) };
    txt(disp, comp_txt, 4, y, &FONT_6X13, comp_col);

    let (hv_txt, hv_col) = if out.high_voltage { ("HV: ON ", YL) } else { ("HV: OFF", DIM) };
    txt(disp, hv_txt, 136, y, &FONT_6X13, hv_col);
    y += 20;

    {
        let mut s: String<20> = String::new();
        write!(s, "Cible: {:+.1} C", target.chamber_temp_c).ok();
        txt(disp, s.as_str(), 4, y, &FONT_6X13, WH);
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

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
