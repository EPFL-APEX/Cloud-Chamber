#[derive(Debug, Copy, Clone)]
pub enum Error {
    SensorIndexOutOfBounds { index: usize },
    HistoryIndexOutOfBounds { index: usize },
}

pub type Result<T> = core::result::Result<T, Error>;