//! Fixed-cell bitmap fonts (Spleen), embedded as one integer per glyph row.

pub use crate::font_data::{SPLEEN_12X24, SPLEEN_16X32, SPLEEN_32X64, SPLEEN_8X16};

/// One glyph row; bit `width - 1` is the leftmost pixel.
pub trait Row: Copy {
    fn bit(self, index: u32) -> bool;
}

impl Row for u8 {
    fn bit(self, index: u32) -> bool {
        self >> index & 1 == 1
    }
}

impl Row for u16 {
    fn bit(self, index: u32) -> bool {
        self >> index & 1 == 1
    }
}

impl Row for u32 {
    fn bit(self, index: u32) -> bool {
        self >> index & 1 == 1
    }
}

pub struct Font<R: 'static> {
    pub width: u32,
    pub height: u32,
    /// Sorted code points; glyph `i` occupies `rows[i * height..][..height]`.
    pub codes: &'static [u32],
    pub rows: &'static [R],
}

impl<R: Row> Font<R> {
    /// Rows of `c`, or of `?` when the font lacks it.
    pub fn glyph(&self, c: char) -> &'static [R] {
        let index = self
            .codes
            .binary_search(&(c as u32))
            .or_else(|_| self.codes.binary_search(&('?' as u32)))
            .unwrap_or(0);
        let start = index * self.height as usize;
        &self.rows[start..start + self.height as usize]
    }
}

/// The four embedded sizes, so drawing code can pick one by value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Size {
    Small,
    Medium,
    Large,
    Huge,
}

impl Size {
    pub fn cell(self) -> (i32, i32) {
        match self {
            Size::Small => (8, 16),
            Size::Medium => (12, 24),
            Size::Large => (16, 32),
            Size::Huge => (32, 64),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_is_complete_in_every_size() {
        for c in ' '..='~' {
            assert!(SPLEEN_8X16.codes.binary_search(&(c as u32)).is_ok());
            assert!(SPLEEN_12X24.codes.binary_search(&(c as u32)).is_ok());
            assert!(SPLEEN_16X32.codes.binary_search(&(c as u32)).is_ok());
            assert!(SPLEEN_32X64.codes.binary_search(&(c as u32)).is_ok());
        }
    }

    #[test]
    fn missing_glyphs_fall_back_to_question_mark() {
        assert_eq!(SPLEEN_8X16.glyph('中'), SPLEEN_8X16.glyph('?'));
        assert_ne!(SPLEEN_8X16.glyph('A'), SPLEEN_8X16.glyph('?'));
    }

    #[test]
    fn rows_have_pixels_where_expected() {
        // Spleen's 'A' has a crossbar row with every inner column set.
        let a = SPLEEN_8X16.glyph('A');
        assert!(a.iter().any(|row| row.count_ones() >= 6));
        assert!(SPLEEN_8X16.glyph(' ').iter().all(|row| *row == 0));
    }
}
