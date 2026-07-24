use embedded_graphics::{
    Drawable, draw_target::DrawTarget, geometry::{Point, Size}, image::{Image, ImageDrawable, ImageDrawableExt, SubImage}, pixelcolor::{Rgb565, Rgb888}, primitives::{Line, Primitive, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle},
};

use tinybmp::Bmp;

//pub fn draw_icons_on_grid<T: ImageDrawable, D: DrawTarget>(texture:T, icon_size:Size, top_left:Point, col_row:(u32, u32), step_size:(i32, i32), display:D) {
//    for i in 0..col_row.0 {
//        for j in 0..col_row.1 {
//            let icon_texture_top_left = todo!();
//        }
//    }
//}

/// Simplifies access to icons within a horizontally contiguous spritesheet image.
pub struct Icons<T: ImageDrawable + Copy> {
    texture:T,
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

    pub fn get(&self, id: usize) -> Option<SubImage<T>> {
        let top_left_x = id as i32 * self.icon_size.width as i32;

        if top_left_x + self.icon_size.width as i32 > self.texture.size().width as i32 {
            return None;
        }

        let top_left = Point::new(top_left_x, 0);
        let contour = Rectangle::new(top_left, self.icon_size);
        Some(self.texture.sub_image(&contour))
    }
}