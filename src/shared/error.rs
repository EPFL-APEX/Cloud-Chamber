#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Error {
    /// Generic index out of bounds (used by low-level structures)
    IndexOutOfBounds { index: usize, len: usize },
}

pub type Result<T> = core::result::Result<T, Error>;