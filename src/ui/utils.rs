use embedded_graphics::{
    geometry::{Point, Size},
    image::{ImageDrawable, ImageDrawableExt, SubImage},
    primitives::Rectangle,
};

/// Simplifies access to icons within a horizontally contiguous spritesheet image.
pub struct Icons<T: ImageDrawable + Copy> {
    texture: T,
    icon_size: Size,
}

#[derive(Debug)]
pub enum IconError {
    WrongShape,
}

impl<T: ImageDrawable + Copy> Icons<T> {
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

    pub fn get(&self, id: usize) -> Option<SubImage<'_, T>> {
        let top_left_x = id as i32 * self.icon_size.width as i32;

        if top_left_x + self.icon_size.width as i32 > self.texture.size().width as i32 {
            return None;
        }

        let top_left = Point::new(top_left_x, 0);
        let contour = Rectangle::new(top_left, self.icon_size);
        Some(self.texture.sub_image(&contour))
    }
}
