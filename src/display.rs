// Affichage TFT ILI9341 240×320 (portrait) — module KMRTM28028-SPI.
// Layout final : indicateur PRÊTE en haut, boutons tactiles en bas.
// Bouton gauche = bascule compresseur (MARCHE quand bloqué / ARRÊT quand autorisé),
// bouton droit = reset système.

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

// ── Palette ───────────────────────────────────────────────────────────────────
const BG:       Rgb565 = Rgb565::BLACK;
const WH:       Rgb565 = Rgb565::WHITE;
const CY:       Rgb565 = Rgb565::CYAN;
const GR:       Rgb565 = Rgb565::GREEN;
const RD:       Rgb565 = Rgb565::RED;
const YL:       Rgb565 = Rgb565::YELLOW;
const DIM:      Rgb565 = Rgb565::new(8, 16, 8);
const BTN_STOP: Rgb565 = Rgb565::new(18, 3, 3); // fond bouton ARRÊT (rouge sombre)
const BTN_GO:   Rgb565 = Rgb565::new(2, 18, 2); // fond bouton MARCHE (vert sombre)
const BTN_RST:  Rgb565 = Rgb565::new(3,  8, 18);

// ── Layout vertical ───────────────────────────────────────────────────────────
const Y_HDR:      i32 = 2;    // "CHAMBRE" + SAFE/ERR + uptime
const Y_READY:    i32 = 26;   // indicateur PRÊTE / EN PREPARATION
const Y_SURSAT:   i32 = 50;   // valeur sursaturation
const Y_LBL:      i32 = 72;   // "BASE CHAMBRE" label
const Y_DS4:      i32 = 84;   // grande valeur ds4 (FONT_10X20)
const Y_CTRL1:    i32 = 112;  // Cible + Ambiance
const Y_CTRL2:    i32 = 128;  // COMP + HV
const Y_DS01:     i32 = 150;  // ds0 + ds1
const Y_DS23:     i32 = 164;  // ds2 + ds3
const Y_BME:      i32 = 178;  // BME280

// ── Boutons tactiles (coordonnées pixels écran) ───────────────────────────────
pub const BTN_Y_TOP:   i32 = 196;
pub const BTN_Y_BOT:   i32 = 316;
pub const BTN_STOP_X1: i32 = 2;
pub const BTN_STOP_X2: i32 = 117;
pub const BTN_RST_X1:  i32 = 121;
pub const BTN_RST_X2:  i32 = 238;

// ── Calibration XPT2046 — ajuster selon le module ────────────────────────────
// Si les boutons ne répondent pas correctement : vérifier que X/Y ne sont pas
// inversés (swap raw_x/raw_y dans touch_to_screen) et ajuster MIN/MAX.
pub const TOUCH_X_MIN: u16 = 300;
pub const TOUCH_X_MAX: u16 = 3800;
pub const TOUCH_Y_MIN: u16 = 300;
pub const TOUCH_Y_MAX: u16 = 3700;

/// Coordonnées brutes XPT2046 → pixels écran.
pub fn touch_to_screen(raw_x: u16, raw_y: u16) -> (i32, i32) {
    let sx = ((raw_x.saturating_sub(TOUCH_X_MIN) as i32).max(0) * 240)
        / (TOUCH_X_MAX - TOUCH_X_MIN) as i32;
    let sy = ((raw_y.saturating_sub(TOUCH_Y_MIN) as i32).max(0) * 320)
        / (TOUCH_Y_MAX - TOUCH_Y_MIN) as i32;
    (sx.min(239), sy.min(319))
}

pub fn is_btn_comp(sx: i32, sy: i32) -> bool {
    sx >= BTN_STOP_X1 && sx <= BTN_STOP_X2 && sy >= BTN_Y_TOP
}

pub fn is_btn_reset(sx: i32, sy: i32) -> bool {
    sx >= BTN_RST_X1 && sx <= BTN_RST_X2 && sy >= BTN_Y_TOP
}

/// P_sat IPA (mmHg) — équation d'Antoine.
fn p_sat_ipa(t_c: f32) -> f32 {
    libm::powf(10.0_f32, 8.118 - 1580.92 / (219.617 + t_c))
}

// ── Éléments fixes — appeler une seule fois à l'init ─────────────────────────
pub fn draw_static<D: DrawTarget<Color = Rgb565>>(disp: &mut D, comp_allowed: bool) {
    fill(disp, 0, 0, 240, 320, BG);

    txt(disp, "CHAMBRE", 4, Y_HDR, &FONT_9X18_BOLD, WH);
    fill(disp, 0, 22, 240, 1, CY);

    fill(disp, 0, 68,  240, 1, DIM);
    fill(disp, 0, 108, 240, 1, DIM);
    fill(disp, 0, 145, 240, 1, DIM);
    fill(disp, 0, 192, 240, 1, DIM);

    txt(disp, "BASE CHAMBRE", 4, Y_LBL, &FONT_6X10, CY);

    draw_btn_comp(disp, comp_allowed);
    btn_reset_normal(disp);
}

// ── Valeurs dynamiques — appeler toutes les 500 ms ───────────────────────────
pub fn draw<D: DrawTarget<Color = Rgb565>>(
    disp: &mut D,
    state: &SystemState,
    target: &TargetState,
    out: &ControlOutput,
    rom_count: usize,
) {
    // ─── Header : SAFE/ERR + uptime ──────────────────────────────────────────
    if out.safety_override {
        txt(disp, "!ERR!", 152, Y_HDR, &FONT_9X18_BOLD, RD);
    } else {
        txt(disp, " SAFE", 152, Y_HDR, &FONT_9X18_BOLD, GR);
    }
    {
        let mut s: String<10> = String::new();
        let up = state.uptime_s;
        if up < 3600 { write!(s, "{:5}s   ", up).ok(); }
        else          { write!(s, "{:2}h{:02}m  ", up / 3600, (up % 3600) / 60).ok(); }
        txt(disp, s.as_str(), 82, Y_HDR + 4, &FONT_6X10, DIM);
    }

    // ─── Indicateur PRÊTE + sursaturation ─────────────────────────────────────
    let t_cold = if 4 < rom_count && state.temperatures[4].valid {
        Some(state.temperatures[4].value)
    } else {
        None
    };
    let t_amb = if state.bme280.valid { Some(state.bme280.temp_c) } else { None };

    match (t_cold, t_amb) {
        (Some(tc), Some(ta)) => {
            let s = p_sat_ipa(ta) / p_sat_ipa(tc);
            let ready = s >= 50.0;
            let col = if ready { GR } else { YL };
            // Les deux chaînes font 14 chars → même largeur, pas de résidu
            txt(disp,
                if ready { "CHAMBRE PRETE " } else { "EN PREPARATION" },
                4, Y_READY, &FONT_9X18_BOLD, col);
            let mut ss: String<18> = String::new();
            write!(ss, "Sursat: x{:5.0}     ", s).ok();
            txt(disp, ss.as_str(), 4, Y_SURSAT, &FONT_6X13, col);
        }
        _ => {
            txt(disp, "EN PREPARATION", 4, Y_READY,  &FONT_9X18_BOLD, DIM);
            txt(disp, "Sursat: ---       ", 4, Y_SURSAT, &FONT_6X13, DIM);
        }
    }

    // ─── Base chambre (ds4) — grande valeur ──────────────────────────────────
    {
        let mut val: String<12> = String::new();
        let col;
        if let Some(t) = t_cold {
            write!(val, "{:+7.1}C  ", t).ok();
            col = if t <= -35.0 { CY } else if t < -20.0 { WH } else { YL };
        } else {
            write!(val, "   ---     ").ok();
            col = DIM;
        }
        txt(disp, val.as_str(), 26, Y_DS4, &FONT_10X20, col);
    }

    // ─── Cible + Ambiance ────────────────────────────────────────────────────
    {
        let mut s: String<36> = String::new();
        let ta_s: String<8> = match t_amb {
            Some(ta) => { let mut b: String<8> = String::new(); write!(b, "{:+5.1}C", ta).ok(); b }
            None     => { let mut b: String<8> = String::new(); write!(b, "  ---  ").ok(); b }
        };
        write!(s, "Cible:{:+5.1}C  Amb:{} ", target.chamber_temp_c, ta_s.as_str()).ok();
        while s.len() < 34 { s.push(' ').ok(); }
        txt(disp, s.as_str(), 4, Y_CTRL1, &FONT_6X10, WH);
    }

    // ─── Compresseur + HV ────────────────────────────────────────────────────
    // Trois états : ON (sortie active), BLOQ (interdit par IHM/écran), OFF (sécurité).
    // Chaînes de même largeur (10 chars) pour éviter les résidus.
    let (ct, cc) = if out.compressor { ("COMP: ON  ", GR) }
        else if !state.compressor_allowed { ("COMP: BLOQ", RD) }
        else { ("COMP: OFF ", DIM) };
    let (ht, hc) = if out.high_voltage { ("HV: ON " , YL) } else { ("HV: OFF", DIM)   };
    txt(disp, ct, 4,   Y_CTRL2, &FONT_6X13, cc);
    txt(disp, ht, 140, Y_CTRL2, &FONT_6X13, hc);

    // ─── Circuit ds0..ds3 ────────────────────────────────────────────────────
    let (v0, c0) = fmt_temp::<9>(state, 0, rom_count);
    let (v1, c1) = fmt_temp::<9>(state, 1, rom_count);
    let (v2, c2) = fmt_temp::<9>(state, 2, rom_count);
    let (v3, c3) = fmt_temp::<9>(state, 3, rom_count);
    txt(disp, "ds0:", 4,   Y_DS01, &FONT_6X10, DIM); txt(disp, v0.as_str(), 28,  Y_DS01, &FONT_6X10, c0);
    txt(disp, "ds1:", 124, Y_DS01, &FONT_6X10, DIM); txt(disp, v1.as_str(), 148, Y_DS01, &FONT_6X10, c1);
    txt(disp, "ds2:", 4,   Y_DS23, &FONT_6X10, DIM); txt(disp, v2.as_str(), 28,  Y_DS23, &FONT_6X10, c2);
    txt(disp, "ds3:", 124, Y_DS23, &FONT_6X10, DIM); txt(disp, v3.as_str(), 148, Y_DS23, &FONT_6X10, c3);

    // ─── BME280 ──────────────────────────────────────────────────────────────
    {
        let mut s: String<36> = String::new();
        if state.bme280.valid {
            write!(s, "T:{:+5.1}C  P:{:6.1}hPa  H:{:3.0}%",
                state.bme280.temp_c, state.bme280.pressure_hpa, state.bme280.humidity_pct).ok();
        } else {
            write!(s, "BME280 absent                     ").ok();
        }
        while s.len() < 34 { s.push(' ').ok(); }
        txt(disp, s.as_str(), 4, Y_BME, &FONT_6X10, if state.bme280.valid { WH } else { DIM });
    }
}

/// Bouton compresseur — bascule. `allowed` = état ACTUEL de l'autorisation :
/// - bloqué  → le bouton propose "MARCHE" (vert)
/// - autorisé → le bouton propose "ARRET" (rouge)
pub fn draw_btn_comp<D: DrawTarget<Color = Rgb565>>(disp: &mut D, allowed: bool) {
    let bw = (BTN_STOP_X2 - BTN_STOP_X1 + 1) as u32;
    let bh = (BTN_Y_BOT   - BTN_Y_TOP   + 1) as u32;
    let (bg, edge, l1, l2) = if allowed {
        (BTN_STOP, RD, " ARRET",  "COMPRESSEUR")
    } else {
        (BTN_GO,   GR, " MARCHE", "COMPRESSEUR")
    };
    fill(disp, BTN_STOP_X1 as u32, BTN_Y_TOP as u32, bw, bh, bg);
    fill(disp, BTN_STOP_X1 as u32, BTN_Y_TOP as u32, bw, 2, edge);
    fill(disp, BTN_STOP_X1 as u32, (BTN_Y_BOT - 1) as u32, bw, 2, edge);
    fill(disp, BTN_STOP_X1 as u32, BTN_Y_TOP as u32, 2, bh, edge);
    fill(disp, (BTN_STOP_X2 - 1) as u32, BTN_Y_TOP as u32, 2, bh, edge);
    txt_on(disp, l1, BTN_STOP_X1 + 4,  BTN_Y_TOP + 36, &FONT_9X18_BOLD, edge, bg);
    txt_on(disp, l2, BTN_STOP_X1 + 22, BTN_Y_TOP + 60, &FONT_6X13,      edge, bg);
}

/// Feedback visuel : remplit le bouton en couleur pleine au moment du toucher.
/// Pour le bouton compresseur, `new_allowed` = état APRÈS la bascule.
pub fn draw_btn_comp_flash<D: DrawTarget<Color = Rgb565>>(disp: &mut D, new_allowed: bool) {
    let bw = (BTN_STOP_X2 - BTN_STOP_X1 + 1) as u32;
    let bh = (BTN_Y_BOT  - BTN_Y_TOP  + 1) as u32;
    let (bg, l1) = if new_allowed { (GR, " MARCHE") } else { (RD, " ARRET") };
    fill(disp, BTN_STOP_X1 as u32, BTN_Y_TOP as u32, bw, bh, bg);
    txt_on(disp, l1,            BTN_STOP_X1 + 4,  BTN_Y_TOP + 36, &FONT_9X18_BOLD, BG, bg);
    txt_on(disp, "COMPRESSEUR", BTN_STOP_X1 + 22, BTN_Y_TOP + 60, &FONT_6X13,      BG, bg);
}

pub fn draw_btn_reset_flash<D: DrawTarget<Color = Rgb565>>(disp: &mut D) {
    let bw = (BTN_RST_X2 - BTN_RST_X1 + 1) as u32;
    let bh = (BTN_Y_BOT  - BTN_Y_TOP  + 1) as u32;
    fill(disp, BTN_RST_X1 as u32, BTN_Y_TOP as u32, bw, bh, CY);
    txt_on(disp, " RESET",   BTN_RST_X1 + 8, BTN_Y_TOP + 36, &FONT_9X18_BOLD, BG, CY);
    txt_on(disp, " SYSTEME", BTN_RST_X1 + 6, BTN_Y_TOP + 60, &FONT_6X13,      BG, CY);
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn btn_reset_normal<D: DrawTarget<Color = Rgb565>>(disp: &mut D) {
    let bw = (BTN_RST_X2 - BTN_RST_X1 + 1) as u32;
    let bh = (BTN_Y_BOT  - BTN_Y_TOP  + 1) as u32;
    fill(disp, BTN_RST_X1 as u32, BTN_Y_TOP as u32, bw, bh, BTN_RST);
    fill(disp, BTN_RST_X1 as u32, BTN_Y_TOP as u32, bw, 2, CY);
    fill(disp, BTN_RST_X1 as u32, (BTN_Y_BOT - 1) as u32, bw, 2, CY);
    fill(disp, BTN_RST_X1 as u32, BTN_Y_TOP as u32, 2, bh, CY);
    fill(disp, (BTN_RST_X2 - 1) as u32, BTN_Y_TOP as u32, 2, bh, CY);
    txt_on(disp, " RESET",   BTN_RST_X1 + 8, BTN_Y_TOP + 36, &FONT_9X18_BOLD, CY, BTN_RST);
    txt_on(disp, " SYSTEME", BTN_RST_X1 + 6, BTN_Y_TOP + 60, &FONT_6X13,      CY, BTN_RST);
}

fn fmt_temp<const N: usize>(
    state: &SystemState, idx: usize, rom_count: usize,
) -> (String<N>, Rgb565) {
    let mut val: String<N> = String::new();
    if idx < rom_count && state.temperatures[idx].valid {
        let t = state.temperatures[idx].value;
        write!(val, "{:+6.1}C ", t).ok();
        (val, if t < -20.0 { CY } else if t > 80.0 { RD } else { WH })
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
    d: &mut D, s: &str, x: i32, y: i32, font: &MonoFont<'_>, fg: Rgb565,
) {
    txt_on(d, s, x, y, font, fg, BG);
}

/// Texte avec fond explicite — pour dessiner sur les boutons colorés
/// sans laisser de rectangle noir derrière les caractères.
fn txt_on<D: DrawTarget<Color = Rgb565>>(
    d: &mut D, s: &str, x: i32, y: i32, font: &MonoFont<'_>, fg: Rgb565, bg: Rgb565,
) {
    let style = MonoTextStyleBuilder::new()
        .font(font).text_color(fg).background_color(bg).build();
    Text::with_baseline(s, Point::new(x, y), style, Baseline::Top).draw(d).ok();
}
