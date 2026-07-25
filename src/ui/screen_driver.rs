// Affichage TFT ILI9341 240×320 (portrait) — module KMRTM28028-SPI.
// Layout final : indicateur PRÊTE en haut, boutons tactiles en bas.
// Bouton gauche = bascule compresseur (MARCHE quand bloqué / ARRÊT quand autorisé),
// bouton droit = reset système.

//
// Review PR #20 : ce fichier ne garde que l'ecran lui-meme (layout, rendu,
// zones de boutons). Les primitives reutilisables sont dans `super::widgets`
// et la calibration du panneau tactile dans `super::touch`.

use core::fmt::Write as _;
use heapless::String;

use embedded_graphics::{
    mono_font::ascii::{FONT_6X10, FONT_6X13, FONT_9X18_BOLD, FONT_10X20},
    pixelcolor::Rgb565,
    prelude::*,
};

use crate::{
    control::{output::ControlOutput, target::TargetState},
    data::SystemState,
};

use super::widgets::{
    fill, fmt_temp, txt, txt_on,
    BG, BTN_CYC, BTN_GO, BTN_RST, BTN_STOP, CY, DIM, GR, RD, WH, YL,
};

const W: i32 = 240;

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
// Trois colonnes : CYCLE | COMP | RESET
pub const BTN_Y_TOP:   i32 = 196;
pub const BTN_Y_BOT:   i32 = 316;
pub const BTN_CYC_X1:  i32 = 2;
pub const BTN_CYC_X2:  i32 = 78;
pub const BTN_STOP_X1: i32 = 82;
pub const BTN_STOP_X2: i32 = 158;
pub const BTN_RST_X1:  i32 = 162;
pub const BTN_RST_X2:  i32 = 238;

pub fn is_btn_cycle(sx: i32, sy: i32) -> bool {
    sx >= BTN_CYC_X1 && sx <= BTN_CYC_X2 && sy >= BTN_Y_TOP
}

pub fn is_btn_comp(sx: i32, sy: i32) -> bool {
    sx >= BTN_STOP_X1 && sx <= BTN_STOP_X2 && sy >= BTN_Y_TOP
}

pub fn is_btn_reset(sx: i32, sy: i32) -> bool {
    sx >= BTN_RST_X1 && sx <= BTN_RST_X2 && sy >= BTN_Y_TOP
}

// Zone de la bannière d'alerte (entre les séparateurs y=22 et y=68).
const ALERT_Y: u32 = 24;
const ALERT_H: u32 = 43;

/// Efface la zone de la bannière d'alerte (à appeler quand l'alerte disparaît,
/// avant le draw() suivant — les textes normaux ne couvrent pas toute la zone).
pub fn clear_alert_zone<D: DrawTarget<Color = Rgb565>>(disp: &mut D) {
    fill(disp, 0, ALERT_Y, 240, ALERT_H, BG);
}

/// P_sat IPA (mmHg) — équation d'Antoine.
fn p_sat_ipa(t_c: f32) -> f32 {
    libm::powf(10.0_f32, 8.118 - 1580.92 / (219.617 + t_c))
}

// ── Éléments fixes — appeler une seule fois à l'init ─────────────────────────
pub fn draw_static<D: DrawTarget<Color = Rgb565>>(
    disp: &mut D, comp_allowed: bool, cycle_active: bool,
) {
    fill(disp, 0, 0, 240, 320, BG);

    txt(disp, "CHAMBRE", 4, Y_HDR, &FONT_9X18_BOLD, WH);
    fill(disp, 0, 22, 240, 1, CY);

    fill(disp, 0, 68,  240, 1, DIM);
    fill(disp, 0, 108, 240, 1, DIM);
    fill(disp, 0, 145, 240, 1, DIM);
    fill(disp, 0, 192, 240, 1, DIM);

    txt(disp, "BASE CHAMBRE", 4, Y_LBL, &FONT_6X10, CY);

    draw_btn_cycle(disp, cycle_active);
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
    phase_label: Option<&'static str>,
    alert: Option<&'static str>,
    blink: bool,
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

    if let Some(msg) = alert {
        // ── Bannière d'erreur clignotante — couvre la zone PRETE/SURSAT ─────
        // Alternance rouge plein / fond noir à chaque rafraîchissement (1 Hz).
        let (bg_c, fg_c) = if blink { (RD, WH) } else { (BG, RD) };
        fill(disp, 0, ALERT_Y, 240, ALERT_H, bg_c);
        let x = (W - msg.len() as i32 * 9) / 2; // centré (FONT_9X18)
        txt_on(disp, msg, x.max(0), Y_READY + 8, &FONT_9X18_BOLD, fg_c, bg_c);
    } else {
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

        // Cycle automatique actif → la phase remplace la ligne PRETE/PREPARATION.
        // Libellés de 14 chars (SystemTask::label) → même largeur, pas de résidu.
        if let Some(pl) = phase_label {
            txt(disp, pl, 4, Y_READY, &FONT_9X18_BOLD, YL);
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
    // Trois états : ON (sortie active), BLOQ (interdit par IHM/écran),
    // PRET (autorisé mais sortie inactive — phase d'attente du cycle).
    // Chaînes de même largeur (10 chars) pour éviter les résidus.
    let (ct, cc) = if out.compressor { ("COMP: ON  ", GR) }
        else if !state.compressor_allowed { ("COMP: BLOQ", RD) }
        else { ("COMP: PRET", YL) };
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

/// Cadre + fond commun des boutons.
fn btn_frame<D: DrawTarget<Color = Rgb565>>(
    disp: &mut D, x1: i32, x2: i32, bg: Rgb565, edge: Rgb565,
) {
    let bw = (x2 - x1 + 1) as u32;
    let bh = (BTN_Y_BOT - BTN_Y_TOP + 1) as u32;
    fill(disp, x1 as u32, BTN_Y_TOP as u32, bw, bh, bg);
    fill(disp, x1 as u32, BTN_Y_TOP as u32, bw, 2, edge);
    fill(disp, x1 as u32, (BTN_Y_BOT - 1) as u32, bw, 2, edge);
    fill(disp, x1 as u32, BTN_Y_TOP as u32, 2, bh, edge);
    fill(disp, (x2 - 1) as u32, BTN_Y_TOP as u32, 2, bh, edge);
}

/// Bouton cycle automatique. `active` = un cycle est en cours :
/// - inactif → propose "START" (lance la séquence)
/// - actif   → propose "STOP" (arrêt propre)
pub fn draw_btn_cycle<D: DrawTarget<Color = Rgb565>>(disp: &mut D, active: bool) {
    let (edge, l2) = if active { (RD, " STOP") } else { (GR, "START") };
    btn_frame(disp, BTN_CYC_X1, BTN_CYC_X2, BTN_CYC, edge);
    txt_on(disp, "CYCLE", BTN_CYC_X1 + 16, BTN_Y_TOP + 36, &FONT_9X18_BOLD, edge, BTN_CYC);
    txt_on(disp, l2,      BTN_CYC_X1 + 23, BTN_Y_TOP + 60, &FONT_6X13,      edge, BTN_CYC);
}

/// Flash tactile du bouton cycle. `starting` = true si on vient de lancer.
pub fn draw_btn_cycle_flash<D: DrawTarget<Color = Rgb565>>(disp: &mut D, starting: bool) {
    let (bg, l2) = if starting { (GR, "START") } else { (RD, " STOP") };
    let bw = (BTN_CYC_X2 - BTN_CYC_X1 + 1) as u32;
    let bh = (BTN_Y_BOT  - BTN_Y_TOP  + 1) as u32;
    fill(disp, BTN_CYC_X1 as u32, BTN_Y_TOP as u32, bw, bh, bg);
    txt_on(disp, "CYCLE", BTN_CYC_X1 + 16, BTN_Y_TOP + 36, &FONT_9X18_BOLD, BG, bg);
    txt_on(disp, l2,      BTN_CYC_X1 + 23, BTN_Y_TOP + 60, &FONT_6X13,      BG, bg);
}

/// Bouton compresseur — bascule. `allowed` = état ACTUEL de l'autorisation :
/// - bloqué  → le bouton propose "MARCHE" (vert)
/// - autorisé → le bouton propose "ARRET" (rouge)
pub fn draw_btn_comp<D: DrawTarget<Color = Rgb565>>(disp: &mut D, allowed: bool) {
    let (bg, edge, l1) = if allowed {
        (BTN_STOP, RD, " ARRET")
    } else {
        (BTN_GO,   GR, "MARCHE")
    };
    btn_frame(disp, BTN_STOP_X1, BTN_STOP_X2, bg, edge);
    txt_on(disp, l1,     BTN_STOP_X1 + 11, BTN_Y_TOP + 36, &FONT_9X18_BOLD, edge, bg);
    txt_on(disp, "COMP.", BTN_STOP_X1 + 23, BTN_Y_TOP + 60, &FONT_6X13,     edge, bg);
}

/// Feedback visuel : remplit le bouton en couleur pleine au moment du toucher.
/// Pour le bouton compresseur, `new_allowed` = état APRÈS la bascule.
pub fn draw_btn_comp_flash<D: DrawTarget<Color = Rgb565>>(disp: &mut D, new_allowed: bool) {
    let bw = (BTN_STOP_X2 - BTN_STOP_X1 + 1) as u32;
    let bh = (BTN_Y_BOT  - BTN_Y_TOP  + 1) as u32;
    let (bg, l1) = if new_allowed { (GR, "MARCHE") } else { (RD, " ARRET") };
    fill(disp, BTN_STOP_X1 as u32, BTN_Y_TOP as u32, bw, bh, bg);
    txt_on(disp, l1,      BTN_STOP_X1 + 11, BTN_Y_TOP + 36, &FONT_9X18_BOLD, BG, bg);
    txt_on(disp, "COMP.", BTN_STOP_X1 + 23, BTN_Y_TOP + 60, &FONT_6X13,      BG, bg);
}

pub fn draw_btn_reset_flash<D: DrawTarget<Color = Rgb565>>(disp: &mut D) {
    let bw = (BTN_RST_X2 - BTN_RST_X1 + 1) as u32;
    let bh = (BTN_Y_BOT  - BTN_Y_TOP  + 1) as u32;
    fill(disp, BTN_RST_X1 as u32, BTN_Y_TOP as u32, bw, bh, CY);
    txt_on(disp, " RESET",  BTN_RST_X1 + 11, BTN_Y_TOP + 36, &FONT_9X18_BOLD, BG, CY);
    txt_on(disp, "SYSTEME", BTN_RST_X1 + 17, BTN_Y_TOP + 60, &FONT_6X13,      BG, CY);
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn btn_reset_normal<D: DrawTarget<Color = Rgb565>>(disp: &mut D) {
    btn_frame(disp, BTN_RST_X1, BTN_RST_X2, BTN_RST, CY);
    txt_on(disp, " RESET",  BTN_RST_X1 + 11, BTN_Y_TOP + 36, &FONT_9X18_BOLD, CY, BTN_RST);
    txt_on(disp, "SYSTEME", BTN_RST_X1 + 17, BTN_Y_TOP + 60, &FONT_6X13,      CY, BTN_RST);
}

