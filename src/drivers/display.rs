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
use embedded_graphics::primitives::Rectangle;
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
    pub fn render<F, E>(&mut self, draw_fn: F) -> Result<(), E>
    where
        F: FnMut(&mut FbTarget<'_>) -> Result<(), E>,
    {
        // Horloge constante : les durées mesurées tombent à zéro.
        self.render_timed(draw_fn, || 0).map(|_| ())
    }

    /// Comme [`render`](Self::render), en mesurant séparément le temps
    /// passé à dessiner dans le framebuffer (`draw_fn`) et le temps passé
    /// à transférer sur SPI — `micros` rend un compteur monotone en
    /// microsecondes.
    ///
    /// C'est l'instrument du diagnostic « un redraw prend combien, et
    /// où » : les deux moitiés se soignent différemment (le dessin dépend
    /// de l'écran et de ses icônes, le transfert du débit SPI et du
    /// câblage).
    pub fn render_timed<F, E>(
        &mut self,
        mut draw_fn: F,
        mut micros: impl FnMut() -> u64,
    ) -> Result<RenderStats, E>
    where
        F: FnMut(&mut FbTarget<'_>) -> Result<(), E>,
    {
        let mut stats = RenderStats::default();
        let mut band_start_y = 0;
        while band_start_y < SCREEN_HEIGHT {
            let mut target = FbTarget { pixels: self.framebuffer, band_start_y };
            let t0 = micros();
            draw_fn(&mut target)?;
            stats.draw_us += micros() - t0;

            let y0 = band_start_y as u16;
            let y1 = (band_start_y + BAND_HEIGHT - 1) as u16;
            let t1 = micros();
            let _ = self.display.draw_raw_slice(0, y0, (SCREEN_WIDTH - 1) as u16, y1, self.framebuffer);
            stats.transfer_us += micros() - t1;

            band_start_y += BAND_HEIGHT;
        }
        Ok(stats)
    }
}

/// Durées des deux phases d'un rendu, en microsecondes — cf.
/// [`FramebufferedDisplay::render_timed`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderStats {
    /// Temps passé dans `draw_fn` — dessin dans le framebuffer.
    pub draw_us: u64,
    /// Temps passé à transférer le framebuffer sur SPI.
    pub transfer_us: u64,
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

    /// Remplissage plein par la voie rapide : une rangée de l'aire est
    /// une tranche contiguë du framebuffer, écrite d'un seul geste
    /// plutôt que pixel par pixel à travers `draw_iter`.
    ///
    /// C'est le chemin de `clear()` et de chaque rectangle plein — de
    /// très loin le premier poste de pixels d'un rendu (76 800 pour le
    /// seul `clear`, que chaque écran paie) : sur M0+, la voie itérative
    /// coûte des dizaines de cycles par pixel, celle-ci en coûte ~2.
    /// Même clipping que `draw_iter` : hors écran, hors bande.
    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        let band_end_y = self.band_start_y + BAND_HEIGHT;
        let x0 = area.top_left.x.max(0).min(SCREEN_WIDTH as i32) as usize;
        let x1 = (area.top_left.x + area.size.width as i32)
            .clamp(0, SCREEN_WIDTH as i32) as usize;
        let y0 = area.top_left.y.max(self.band_start_y as i32);
        let y1 = (area.top_left.y + area.size.height as i32)
            .min(band_end_y as i32)
            .max(self.band_start_y as i32);

        if x0 >= x1 || y0 >= y1 {
            return Ok(());
        }

        let value = RawU16::from(color).into_inner();
        for y in y0..y1 {
            let row = (y - self.band_start_y as i32) as usize * SCREEN_WIDTH;
            self.pixels[row + x0..row + x1].fill(value);
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

    /// `fill_solid` doit produire exactement ce que la voie générique
    /// (pixel par pixel via `draw_iter`) produisait — seule la vitesse
    /// change. Couvre les aires à cheval sur les bords d'écran et sur
    /// les bords de bande.
    #[test]
    fn fill_solid_matches_the_pixel_by_pixel_reference() {
        let cases = [
            Rectangle::new(Point::new(0, 0), Size::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32)),
            Rectangle::new(Point::new(-10, -5), Size::new(30, 20)),
            Rectangle::new(Point::new(300, 220), Size::new(50, 50)),
            Rectangle::new(Point::new(100, 100), Size::new(0, 10)),
        ];
        for case in cases {
            for band_start_y in [0, BAND_HEIGHT] {
                let color = point_color(0xABCD);

                let mut fast = [0u16; SCREEN_WIDTH * BAND_HEIGHT];
                let mut target = FbTarget { pixels: &mut fast, band_start_y };
                target.fill_solid(&case, color).unwrap();

                let mut reference = [0u16; SCREEN_WIDTH * BAND_HEIGHT];
                let mut ref_target = FbTarget { pixels: &mut reference, band_start_y };
                ref_target
                    .draw_iter(
                        case.points().map(|p| Pixel(p, color)),
                    )
                    .unwrap();

                assert_eq!(
                    fast, reference,
                    "fill_solid != reference pour {case:?} bande {band_start_y}"
                );
            }
        }
    }
}
