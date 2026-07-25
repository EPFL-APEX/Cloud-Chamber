//! Panneau tactile résistif XPT2046 — bit-bang SPI.
//!
//! Déplacé depuis `src/bin/main.rs` suite à la review PR #20 : le main ne
//! doit contenir que de la logique globale. Ce pilote a sa place dans `ui`,
//! qui porte toute l'interaction utilisateur — l'écran comme le toucher.
//!
//! Les fonctions sont génériques sur les traits `embedded_hal::digital` :
//! aucune dépendance au HAL RP2040, donc testables et réutilisables tels
//! quels sur une autre puce.

use embedded_hal::digital::{InputPin, OutputPin};

/// DCLK max du XPT2046 = 2 MHz : l'ADC SAR convertit PENDANT les clocks de
/// lecture. Bit-bangé sans délai, le RP2040 dépasse cette limite → conversions
/// fausses (X/Y ≈ 0 constants alors que Z1 semble plausible). On force ~500 kHz.
#[inline(always)]
fn xpt_tick() { cortex_m::asm::delay(125); } // ~1 µs @125 MHz → DCLK ≈ 500 kHz

/// Seuil de pression Z1 en dessous duquel on considère l'écran non touché.
const TOUCH_PRESSURE_MIN: u16 = 50;

/// Bornes de validité des coordonnées brutes (rejet des lectures parasites).
const RAW_MIN: u16 = 100;
const RAW_MAX: u16 = 3950;

/// Envoie une commande 8 bits et lit 16 bits de réponse.
/// Retourne les 12 bits de données : `(val >> 3) & 0x0FFF` (protocole XPT2046).
pub fn xpt2046_read(
    clk:  &mut impl OutputPin,
    din:  &mut impl OutputPin,
    dout: &mut impl InputPin,
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

/// Lecture d'un canal avec temps d'établissement : la 1re conversion après le
/// changement de drivers du panneau est jetée, puis moyenne de 2 lectures.
pub fn xpt2046_read_ch(
    clk:  &mut impl OutputPin,
    din:  &mut impl OutputPin,
    dout: &mut impl InputPin,
    cmd:  u8,
) -> u16 {
    let _ = xpt2046_read(clk, din, dout, cmd); // dummy : polarise le panneau
    let a  = xpt2046_read(clk, din, dout, cmd);
    let b  = xpt2046_read(clk, din, dout, cmd);
    (a + b) / 2
}

/// Lit Z1, X, Y bruts — pour diagnostic uniquement.
pub fn touch_raw(
    clk:  &mut impl OutputPin,
    din:  &mut impl OutputPin,
    dout: &mut impl InputPin,
    cs:   &mut impl OutputPin,
) -> (u16, u16, u16) {
    cs.set_low().ok();
    let z1 = xpt2046_read(clk, din, dout, 0xB1);
    let x  = xpt2046_read_ch(clk, din, dout, 0xD1);
    let y  = xpt2046_read_ch(clk, din, dout, 0x91);
    xpt2046_read(clk, din, dout, 0x00);
    cs.set_high().ok();
    (z1, x, y)
}

/// Lit X et Y si l'écran est touché, sinon `None`.
pub fn touch_read(
    clk:  &mut impl OutputPin,
    din:  &mut impl OutputPin,
    dout: &mut impl InputPin,
    cs:   &mut impl OutputPin,
) -> Option<(u16, u16)> {
    cs.set_low().ok();
    let z1 = xpt2046_read(clk, din, dout, 0xB1); // canal pression Z1
    if z1 < TOUCH_PRESSURE_MIN {
        cs.set_high().ok();
        return None;
    }
    let x = xpt2046_read_ch(clk, din, dout, 0xD1); // canal X
    let y = xpt2046_read_ch(clk, din, dout, 0x91); // canal Y
    xpt2046_read(clk, din, dout, 0x00);            // mise en veille
    cs.set_high().ok();
    if x < RAW_MIN || x > RAW_MAX || y < RAW_MIN || y > RAW_MAX { return None; }
    Some((x, y))
}

// ── Calibration du panneau — ajuster selon le module ────────────────────────
// Si les boutons ne repondent pas correctement : verifier que X/Y ne sont pas
// inverses (swap raw_x/raw_y dans touch_to_screen) et ajuster MIN/MAX.
//
// Deplace depuis screen_driver.rs (review PR #20) : c'est de la calibration
// de panneau, independante de ce qui est dessine a l'ecran.

pub const TOUCH_X_MIN: u16 = 300;
pub const TOUCH_X_MAX: u16 = 3800;
pub const TOUCH_Y_MIN: u16 = 300;
pub const TOUCH_Y_MAX: u16 = 3700;

/// Coordonnées brutes XPT2046 → pixels écran (240×320, portrait).
pub fn touch_to_screen(raw_x: u16, raw_y: u16) -> (i32, i32) {
    let sx = ((raw_x.saturating_sub(TOUCH_X_MIN) as i32).max(0) * 240)
        / (TOUCH_X_MAX - TOUCH_X_MIN) as i32;
    let sy = ((raw_y.saturating_sub(TOUCH_Y_MIN) as i32).max(0) * 320)
        / (TOUCH_Y_MAX - TOUCH_Y_MIN) as i32;
    (sx.min(239), sy.min(319))
}
