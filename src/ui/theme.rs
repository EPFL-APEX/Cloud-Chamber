//! Palette de couleurs et styles graphiques de l'interface.
//!
//! # Où viennent les couleurs
//!
//! De la planche d'icônes elle-même (`src/ui/images/menu_icons.bmp`,
//! `stats_icons.bmp`) : une famille de bleus — azur vif, bleu moyen,
//! glacier — posés sur le même bleu-nuit que le fond de la planche. La
//! palette ci-dessous reprend ces teintes-là, pour que texte, filets et
//! carrés d'état vivent dans le même monde que les icônes. Les valeurs
//! exactes ont été relevées dans le BMP (sonde jetable), pas inventées.
//!
//! # Encodage Rgb565 — le piège qui cassait tout
//!
//! `Rgb565::new(r, g, b)` attend **5 bits pour R, 6 pour G, 5 pour B** :
//! R et B vont de 0 à 31, G de 0 à 63. La palette d'origine avait été
//! écrite comme si les trois canaux allaient de 0 à 31 — le vert, divisé
//! par deux, éteignait toute la couleur : le « rouge » de `Rgb565::RED`
//! natif s'affichait bleu-noir à l'écran, le fond `(0,5,4)` était un noir
//! quasi pur, et l'accent `(1,9,8)` se fondait dedans. L'écran entier
//! était quasiment invisible.
//!
//! Les valeurs ci-dessous sont posées en connaissance de cause, et le
//! test `theme_colors_are_distinct_and_visible` en verrouille le
//! contraste : chaque couple fond/texte doit dépasser un écart de
//! luminosité clairement lisible.

use embedded_graphics::pixelcolor::{Rgb565, RgbColor};

// ─── Couleurs principales ─────────────────────────────────────────────────────

/// Fond d'écran : le bleu-nuit de la planche d'icônes (pas du noir pur —
/// l'écran d'une chambre à brouillard est regardé dans le noir, un noir
/// pur rend les bords indiscernables du cadre). Et les icônes, dont le
/// fond propre est exactement celui-ci, s'y fondent sans raccord.
pub const BACKGROUND_COLOR: Rgb565 = Rgb565::new(0, 5, 4);
/// Bandes (titre, pied de menu) : un cran sous le fond, pour découper
/// sans créer de front dur.
pub const BACKGROUND_COLOR_DARKER: Rgb565 = Rgb565::new(0, 3, 2);
/// Bleu moyen : bordures, séparateurs, sélection.
pub const ACCENT_COLOR: Rgb565 = Rgb565::new(1, 26, 21);
/// Azur vif — l'icône dominante de la planche : sélection, mise en
/// avant, carrés d'état actif.
pub const HIGHLIGHT_COLOR: Rgb565 = Rgb565::new(1, 32, 26);

// ─── Couleurs d'état (écrans de mesures) ───────────────────────────────────────

pub const TEXT_COLOR: Rgb565 = Rgb565::WHITE;
/// Texte secondaire : gris-bleu nettement plus sombre que le texte
/// principal, mais au-dessus du fond (lisible, juste discret).
pub const DIM_COLOR: Rgb565 = Rgb565::new(13, 33, 19);
pub const SUCCESS_COLOR: Rgb565 = Rgb565::new(0, 63, 0);
pub const WARNING_COLOR: Rgb565 = Rgb565::new(31, 63, 0);
/// Rouge vif : réservé au déclenchement sécurité — un état qui doit se
/// voir depuis l'autre bout de la pièce, pas un ornement.
pub const DANGER_COLOR: Rgb565 = Rgb565::new(31, 10, 0);

/// Luminosité perçue approximative (0-255) d'une couleur Rgb565,
/// pondérée comme l'œil l'est (vert dominant) — l'outil du test de
/// contraste ci-dessous.
pub(crate) fn luminance(c: Rgb565) -> u16 {
    // R et B sur 5 bits (×8 → 0-248), G sur 6 bits (×4 → 0-252) :
    // normaliser les trois sur 0-255 avant la pondération.
    let r = c.r() as u16 * 8;
    let g = c.g() as u16 * 4;
    let b = c.b() as u16 * 8;
    (r * 30 + g * 59 + b * 11) / 100
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fond vs texte : l'écart de luminosité qui rend un écran lisible.
    /// 80/255 est le seuil au-delà duquel un texte se lit d'un coup
    /// d'œil, même fatigué, même à distance — mesuré sur les paires
    /// réellement dessinées ensemble.
    #[test]
    fn theme_colors_are_distinct_and_visible() {
        // Texte principal et secondaire sur les deux fonds.
        for (fg, bg, what) in [
            (TEXT_COLOR, BACKGROUND_COLOR, "texte sur fond"),
            (TEXT_COLOR, BACKGROUND_COLOR_DARKER, "texte sur bande"),
            (DIM_COLOR, BACKGROUND_COLOR, "texte dim sur fond"),
            (DIM_COLOR, BACKGROUND_COLOR_DARKER, "texte dim sur bande"),
            (HIGHLIGHT_COLOR, BACKGROUND_COLOR, "highlight sur fond"),
        ] {
            let contrast = luminance(fg).abs_diff(luminance(bg));
            assert!(
                contrast >= 80,
                "{what} : contraste {contrast} (<80) — illisible"
            );
        }

        // Couleurs d'état : assez vives pour se voir sur le fond.
        for (c, what) in [
            (SUCCESS_COLOR, "succès"),
            (WARNING_COLOR, "avertissement"),
            (DANGER_COLOR, "danger"),
            (ACCENT_COLOR, "accent"),
        ] {
            let contrast = luminance(c).abs_diff(luminance(BACKGROUND_COLOR));
            assert!(
                contrast >= 60,
                "{what} : contraste {contrast} (<60) — invisible"
            );
        }

        // Les couleurs d'état entre elles : se distinguer, pour qu'un
        // bouton STOP rouge ne se confonde jamais avec un START vert.
        // La luminance seule ne fait pas tout : rouge et vert saturés
        // peuvent être proches en luminance tout en étant opposés en
        // teinte — 40 d'écart suffit pour cette paire, l'œil fait le
        // reste sur la teinte.
        assert!(
            luminance(DANGER_COLOR).abs_diff(luminance(SUCCESS_COLOR)) >= 40,
            "danger et succès trop proches"
        );
        assert!(
            luminance(WARNING_COLOR).abs_diff(luminance(SUCCESS_COLOR)) >= 40,
            "avertissement et succès trop proches"
        );
    }
}

