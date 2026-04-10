use crate::shared::error::{Error, Result};

pub struct RingBuffer<T: Copy, const N: usize> {
    data: [T; N],
    write_index: usize,
    is_full: bool,
}

impl<T: Copy + Default, const N: usize> RingBuffer<T, N> {
    pub fn new() -> Self {
        Self {
            data: [T::default(); N],
            write_index: 0,
            is_full: false,
        }
    }

    pub fn push(&mut self, value: T) {
        self.data[self.write_index] = value;
        self.write_index = (self.write_index + 1) % N;

        if self.write_index == 0 {
            self.is_full = true;
        }
    }

    pub fn get(&self, index: usize) -> Result<T> {
        if index >= N {
            return Err(Error::IndexOutOfBounds { index, len: N });
        }

        if !self.is_full && index >= self.write_index {
            return Err(Error::IndexOutOfBounds {
                index,
                len: self.write_index,
            });
        }

        let actual_index =
            (self.write_index + N - 1 - index) % N;

        Ok(self.data[actual_index])
    }
}