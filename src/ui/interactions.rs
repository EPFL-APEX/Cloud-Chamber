pub trait Rotary {
    fn right_turn(&mut self);
    fn left_turn(&mut self);
}

pub trait Click {
    fn click(&mut self);
}