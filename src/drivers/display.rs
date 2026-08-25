//! Driver écran ILI9341 (320×240, SPI) avec framebuffer RAM.
//!
//! # Pourquoi un framebuffer plutôt que dessiner directement sur l'écran
//!
//! `Ili9341::draw_iter` (l'implémentation par défaut de `DrawTarget`, celle
//! qu'`embedded_graphics` utilise pour chaque pixel de texte/icône) fait un
//! `draw_raw_iter` par pixel *individuel* : chaque pixel rouvre sa propre
//! fenêtre d'écriture et sa propre transaction SPI. Pour un écran
//! texte/icônes riche, ça fait des milliers de mini-transactions au lieu
//! d'une seule — constaté sur matériel réel : plusieurs secondes pour
//! dessiner un menu, même en `--release`.
//!
//! [`FramebufferedDisplay`] dessine plutôt dans une zone de RAM (écritures
//! quasi gratuites, aucune transaction SPI), puis transfère l'écran entier
//! en un seul bloc via `Ili9341::draw_raw_slice`.
//!
//! # Plein écran plutôt que bandé
//!
//! Le framebuffer fait 320×240×2 = 150 Ko — plein écran, pas bandé.
//! `BAND_HEIGHT` reste paramétrable (mécanisme conservé pour re-réduire la
//! RAM utilisée si besoin un jour), mais vaut aujourd'hui [`SCREEN_HEIGHT`]
//! (une seule "bande" = l'écran entier, une seule transaction SPI).
//! L'ancienne version à deux bandes rejouait `draw_fn` deux fois par
//! redessin ; en plein écran, une seule fois.
//!
//! # Pourquoi le framebuffer est un `static` et non un champ par valeur
//!
//! Il vit en `.bss` et se réclame une fois via [`take_framebuffer`], plutôt
//! que d'être construit dans [`FramebufferedDisplay::new`].
//!
//! La version précédente le construisait par valeur — `Self { framebuffer:
//! [0; N], .. }` — et **plantait la carte en `--release` comme en debug
//! selon le profil** : un tableau de 150 Ko construit dans la frame de
//! `new()` puis renvoyé par valeur demande une copie, donc 300 Ko de pile
//! au moment de l'appel, pour 246 Ko disponibles. En `--release` le
//! compilateur élide la copie et ça passe ; en debug il ne l'élide pas, et
//! la pile déborde — HardFault au démarrage, sans message. Constaté sur
//! matériel réel.
//!
//! Passer par un `static` supprime la question : plus aucune copie, plus
//! aucune dépendance au profil de compilation ni aux optimisations du
//! compilateur pour que le firmware démarre. C'est aussi ce qui rend le
//! coût visible — 150 Ko en `.bss` apparaissent dans `size`, alors qu'une
//! grosse allocation de pile ne se voit nulle part avant de déborder.
//!
//! # Utilisation
//!
//! ```ignore
//! let ili9341_display = Ili9341::new(iface, reset_pin, &mut delay, Orientation::Landscape, DisplaySize240x320)?;
//! let framebuffer = display::take_framebuffer().expect("un seul ecran");
//! let mut display = FramebufferedDisplay::new(ili9341_display, framebuffer);
//!
//! // `draw_fn` doit être pure/idempotente (dessiner un état donné, pas des
//! // effets de bord cumulatifs) : appelée une fois par bande, avec le même
//! // résultat visuel voulu à chaque fois — c'est déjà le cas de tout écran
//! // écrit contre `DrawTarget` normalement (ex. Screens::draw).
//! display.render(|target| screens.draw(target, &state))?;
//! ```

use core::cell::Cell;
use critical_section::Mutex;

use display_interface::WriteOnlyDataCommand;
use embedded_hal::digital::OutputPin;
use embedded_graphics::pixelcolor::{Rgb565, raw::RawU16};
use embedded_graphics::prelude::*;
use ili9341::Ili9341;

/// Largeur de l'écran ILI9341 en pixels (orientation paysage).
pub const SCREEN_WIDTH: usize = 320;
/// Hauteur de l'écran ILI9341 en pixels (orientation paysage).
pub const SCREEN_HEIGHT: usize = 240;
/// Hauteur d'une bande du framebuffer — cf. doc de module ("Plein écran
/// plutôt que bandé"). Vaut aujourd'hui l'écran entier ; réduire cette
/// constante (ex. `SCREEN_HEIGHT / 2`) redonne le comportement bandé si la
/// RAM redevient contrainte.
pub const BAND_HEIGHT: usize = SCREEN_HEIGHT;

/// Le tampon lui-même — cf. doc de module pour la raison de ce type.
pub type Framebuffer = [u16; SCREEN_WIDTH * BAND_HEIGHT];

/// Le framebuffer, en `.bss`. Réclamé une seule fois via
/// [`take_framebuffer`], jamais accédé autrement.
static mut FRAMEBUFFER: Framebuffer = [0; SCREEN_WIDTH * BAND_HEIGHT];

/// Vrai dès que [`take_framebuffer`] a rendu la main sur `FRAMEBUFFER`.
static FRAMEBUFFER_TAKEN: Mutex<Cell<bool>> = Mutex::new(Cell::new(false));

/// Réclame le framebuffer statique. Renvoie `None` au deuxième appel.
///
/// C'est cette unicité qui rend sûre la référence `&'static mut` distribuée
/// ici : un seul appelant peut l'obtenir, donc il n'existe jamais deux
/// chemins d'écriture vers le même tampon. Le drapeau est lu et posé sous
/// section critique, donc l'unicité tient aussi entre les deux cœurs.
pub fn take_framebuffer() -> Option<&'static mut Framebuffer> {
    let already_taken = critical_section::with(|cs| FRAMEBUFFER_TAKEN.borrow(cs).replace(true));
    if already_taken {
        return None;
    }
    // Sûr : le drapeau ci-dessus garantit qu'on ne passe ici qu'une fois,
    // donc cette référence exclusive est la seule qui existera. `&raw mut`
    // évite de créer une référence intermédiaire vers le `static mut`
    // (interdit en édition 2024).
    Some(unsafe { &mut *(&raw mut FRAMEBUFFER) })
}

/// Écran ILI9341 avec framebuffer RAM — cf. doc de module.
pub struct FramebufferedDisplay<IFACE, RESET> {
    display: Ili9341<IFACE, RESET>,
    framebuffer: &'static mut Framebuffer,
}

impl<IFACE, RESET> FramebufferedDisplay<IFACE, RESET>
where
    IFACE: WriteOnlyDataCommand,
    RESET: OutputPin,
{
    /// `framebuffer` vient de [`take_framebuffer`] — cf. doc de module
    /// pour pourquoi il n'est pas construit ici.
    pub fn new(display: Ili9341<IFACE, RESET>, framebuffer: &'static mut Framebuffer) -> Self {
        Self { display, framebuffer }
    }

    /// Dessine l'écran en appelant `draw_fn` une fois par bande verticale
    /// du framebuffer, puis transfère chaque bande à l'écran en une seule
    /// transaction SPI — cf. doc de module.
    ///
    /// `draw_fn` doit être pure/idempotente : appelée plusieurs fois de
    /// suite, elle doit produire le même résultat visuel à chaque fois. Le
    /// clipping par bande (cf. [`FbTarget`]) fait le tri des pixels
    /// réellement écrits selon la bande courante — `draw_fn` n'a pas à en
    /// tenir compte, elle dessine "l'écran entier" à chaque appel comme
    /// n'importe quel `DrawTarget` normal.
    pub fn render<F, E>(&mut self, mut draw_fn: F) -> Result<(), E>
    where
        F: FnMut(&mut FbTarget<'_>) -> Result<(), E>,
    {
        let mut band_start_y = 0;
        while band_start_y < SCREEN_HEIGHT {
            let mut target = FbTarget { pixels: self.framebuffer, band_start_y };
            draw_fn(&mut target)?;

            let y0 = band_start_y as u16;
            let y1 = (band_start_y + BAND_HEIGHT - 1) as u16;
            let _ = self.display.draw_raw_slice(0, y0, (SCREEN_WIDTH - 1) as u16, y1, self.framebuffer);

            band_start_y += BAND_HEIGHT;
        }
        Ok(())
    }
}

/// `DrawTarget` dessinant dans le framebuffer RAM d'une bande plutôt que
/// directement sur l'écran — cf. doc de module. Rapporte toujours la
/// taille plein écran via `OriginDimensions` (les écrans dessinent le même
/// contenu à chaque bande, cf. [`FramebufferedDisplay::render`]) ; seuls
/// les pixels dont `y` tombe dans `[band_start_y, band_start_y +
/// BAND_HEIGHT)` sont réellement écrits, les autres sont silencieusement
/// ignorés (ils seront écrits lors d'une bande suivante).
pub struct FbTarget<'a> {
    pixels: &'a mut [u16],
    band_start_y: usize,
}

impl OriginDimensions for FbTarget<'_> {
    fn size(&self) -> Size {
        Size::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32)
    }
}

impl DrawTarget for FbTarget<'_> {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let band_end_y = self.band_start_y + BAND_HEIGHT;
        for Pixel(point, color) in pixels {
            if point.x >= 0 && point.y >= 0 {
                let (x, y) = (point.x as usize, point.y as usize);
                if x < SCREEN_WIDTH && y >= self.band_start_y && y < band_end_y {
                    let local_y = y - self.band_start_y;
                    self.pixels[local_y * SCREEN_WIDTH + x] = RawU16::from(color).into_inner();
                }
            }
        }
        Ok(())
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn point_color(value: u16) -> Rgb565 {
        Rgb565::from(RawU16::new(value))
    }

    #[test]
    fn pixel_within_band_is_written_at_local_offset() {
        let mut buf = [0u16; SCREEN_WIDTH * BAND_HEIGHT];
        let mut target = FbTarget { pixels: &mut buf, band_start_y: BAND_HEIGHT };
        // y = BAND_HEIGHT (première ligne de la 2e bande) → local_y = 0.
        target
            .draw_iter([Pixel(Point::new(5, BAND_HEIGHT as i32), point_color(0x1234))])
            .unwrap();
        assert_eq!(buf[5], 0x1234);
    }

    #[test]
    fn pixel_outside_band_is_ignored() {
        let mut buf = [0u16; SCREEN_WIDTH * BAND_HEIGHT];
        let mut target = FbTarget { pixels: &mut buf, band_start_y: 0 };
        // y = BAND_HEIGHT tombe dans la bande *suivante*, pas celle-ci.
        target
            .draw_iter([Pixel(Point::new(5, BAND_HEIGHT as i32), point_color(0x1234))])
            .unwrap();
        assert!(buf.iter().all(|&p| p == 0));
    }

    #[test]
    fn negative_coordinates_are_ignored() {
        let mut buf = [0u16; SCREEN_WIDTH * BAND_HEIGHT];
        let mut target = FbTarget { pixels: &mut buf, band_start_y: 0 };
        target
            .draw_iter([
                Pixel(Point::new(-1, 0), point_color(0x1234)),
                Pixel(Point::new(0, -1), point_color(0x1234)),
            ])
            .unwrap();
        assert!(buf.iter().all(|&p| p == 0));
    }

    #[test]
    fn x_beyond_screen_width_is_ignored() {
        let mut buf = [0u16; SCREEN_WIDTH * BAND_HEIGHT];
        let mut target = FbTarget { pixels: &mut buf, band_start_y: 0 };
        target
            .draw_iter([Pixel(Point::new(SCREEN_WIDTH as i32, 0), point_color(0x1234))])
            .unwrap();
        assert!(buf.iter().all(|&p| p == 0));
    }

    #[test]
    fn origin_dimensions_reports_full_screen_regardless_of_band() {
        let mut buf = [0u16; SCREEN_WIDTH * BAND_HEIGHT];
        let target = FbTarget { pixels: &mut buf, band_start_y: BAND_HEIGHT };
        assert_eq!(target.size(), Size::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32));
    }
}
