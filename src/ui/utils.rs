use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    image::{GetPixel, ImageDrawable},
    Pixel,
};

/// Simplifie l'accès aux icônes d'une bande d'images contiguës
/// horizontalement (spritesheet).
///
/// # Pourquoi pas `sub_image`
///
/// `ImageDrawableExt::sub_image` délègue à `draw_sub_image`, qui — au
/// moins pour les BMP de `tinybmp` — **décode l'image entière** et
/// laisse un adaptateur clippé en éliminer les pixels hors cadre.
/// Dessiner une icône 64×64 d'une bande de 768×64 décodait donc
/// 49 152 pixels pour en afficher 4 096 — et le menu, qui en affiche
/// sept et se redessine à chaque nouvelle mesure, passait l'essentiel
/// de son temps là (vu sur la machine : ~920 ms par redraw du menu
/// contre ~170 ms pour les écrans sans icônes).
///
/// [`Icons::draw_icon`] lit à la place exactement les pixels de la
/// fenêtre de l'icône, via l'accès aléatoire de [`GetPixel`].
pub struct Icons<T: ImageDrawable + Copy> {
    texture: T,
    icon_size: Size,
}

#[derive(Debug)]
pub enum IconError {
    WrongShape,
}

impl<T> Icons<T>
where
    T: ImageDrawable + Copy,
    T: GetPixel<Color = <T as ImageDrawable>::Color>,
{
    pub fn new(texture: T, icon_size: Size) -> Result<Self, IconError> {
        let texture_size = texture.size();
        if icon_size.width == 0
            || texture_size.height != icon_size.height
            || texture_size.width % icon_size.width != 0
        {
            return Err(IconError::WrongShape);
        }

        Ok(Icons { texture, icon_size })
    }

    /// Dessine l'icône `id` avec son coin supérieur gauche en `position`.
    ///
    /// Une icône au-delà de la bande, ou un pixel illisible, ne dessine
    /// rien — `GetPixel::pixel` rend `None` et le pixel est simplement
    /// absent du flux. Les pixels hors écran sont éliminés par la cible
    /// de dessin, comme pour tout `draw_iter`.
    pub fn draw_icon<D>(&self, target: &mut D, id: usize, position: Point) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = <T as ImageDrawable>::Color>,
    {
        let width = self.icon_size.width as i32;
        let height = self.icon_size.height as i32;
        let sheet_x0 = id as i32 * width;

        let pixels = (0..height)
            .flat_map(move |y| (0..width).map(move |x| (x, y)))
            .filter_map(|(x, y)| {
                self.texture.pixel(Point::new(sheet_x0 + x, y)).map(|c| {
                    Pixel(Point::new(position.x + x, position.y + y), c)
                })
            });

        target.draw_iter(pixels)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use embedded_graphics::{
        Drawable,
        geometry::Size,
        image::{Image, ImageDrawableExt},
        pixelcolor::Rgb565,
        primitives::Rectangle,
    };
    use embedded_graphics_simulator::SimulatorDisplay;
    use tinybmp::Bmp;

    /// `draw_icon` doit produire exactement ce que l'ancien chemin
    /// (`sub_image` + clip, cf. doc de module) produisait — seule la
    /// quantité de travail change, pas le résultat.
    #[test]
    fn draw_icon_matches_the_sub_image_reference() {
        let data = include_bytes!("images/stats_icons.bmp");
        let bmp = Bmp::<Rgb565>::from_slice(data).unwrap();
        let icons = Icons::new(bmp, Size::new(18, 18)).unwrap();

        let size = Size::new(320, 240);
        // Position non alignée : le clip de la cible ne doit masquer ni
        // moins ni plus de pixels que la référence.
        let position = Point::new(37, 91);

        let mut fast = SimulatorDisplay::<Rgb565>::new(size);
        icons.draw_icon(&mut fast, 2, position).unwrap();

        let mut reference = SimulatorDisplay::<Rgb565>::new(size);
        let area = Rectangle::new(Point::new(2 * 18, 0), Size::new(18, 18));
        Image::new(&bmp.sub_image(&area), position).draw(&mut reference).unwrap();

        assert!(
            fast.diff(&reference).is_none(),
            "draw_icon et sub_image ne rendent pas la meme image"
        );
    }
}
