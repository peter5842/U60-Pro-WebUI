//! An upright 320x480 RGB565 frame with the few primitives the pages need.
//! Rotation for the upside-down panel happens only when the frame is copied
//! into the scanout buffer (`blit_rotated`).

use crate::font::{Font, Row, Size, SPLEEN_12X24, SPLEEN_16X32, SPLEEN_32X64, SPLEEN_8X16};

pub const W: i32 = 320;
pub const H: i32 = 480;

pub const fn rgb(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 & 0xf8) << 8) | ((g as u16 & 0xfc) << 3) | (b as u16 >> 3)
}

/// Scale a colour's channels by `num / 256`.
pub fn dim(c: u16, num: u32) -> u16 {
    let r = ((c >> 11) as u32 & 0x1f) * num / 256;
    let g = ((c >> 5) as u32 & 0x3f) * num / 256;
    let b = (c as u32 & 0x1f) * num / 256;
    ((r << 11) | (g << 5) | b) as u16
}

/// `b` over `a` with opacity `alpha / 256`.
pub fn mix(a: u16, b: u16, alpha: u32) -> u16 {
    let alpha = alpha.min(256);
    let ch = |v: u16, shift: u16, mask: u32| (v >> shift) as u32 & mask;
    let m = |shift: u16, mask: u32| {
        let (x, y) = (ch(a, shift, mask), ch(b, shift, mask));
        ((x * (256 - alpha) + y * alpha) / 256) << shift
    };
    (m(11, 0x1f) | m(5, 0x3f) | m(0, 0x1f)) as u16
}

pub struct Canvas {
    pub px: Vec<u16>,
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

impl Canvas {
    pub fn new() -> Self {
        Self {
            px: vec![0; (W * H) as usize],
        }
    }

    pub fn clear(&mut self, c: u16) {
        self.px.fill(c);
    }

    pub fn pixel(&mut self, x: i32, y: i32, c: u16) {
        if (0..W).contains(&x) && (0..H).contains(&y) {
            self.px[(y * W + x) as usize] = c;
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        let (x0, y0) = (x.max(0), y.max(0));
        let (x1, y1) = ((x + w).min(W), (y + h).min(H));
        for yy in y0..y1 {
            let row = (yy * W) as usize;
            self.px[row + x0 as usize..row + x1.max(x0) as usize].fill(c);
        }
    }

    pub fn hline(&mut self, x: i32, y: i32, w: i32, c: u16) {
        self.fill_rect(x, y, w, 1, c);
    }

    pub fn vline(&mut self, x: i32, y: i32, h: i32, c: u16) {
        self.fill_rect(x, y, 1, h, c);
    }

    /// Dotted horizontal rule: one lit pixel every `step`.
    pub fn dotted(&mut self, x: i32, y: i32, w: i32, step: i32, c: u16) {
        let mut xx = x;
        while xx < x + w {
            self.pixel(xx, y, c);
            xx += step.max(1);
        }
    }

    pub fn frame(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        self.hline(x, y, w, c);
        self.hline(x, y + h - 1, w, c);
        self.vline(x, y, h, c);
        self.vline(x + w - 1, y, h, c);
    }

    fn glyph<R: Row>(&mut self, font: &Font<R>, x: i32, y: i32, ch: char, c: u16) {
        for (dy, row) in font.glyph(ch).iter().enumerate() {
            for dx in 0..font.width {
                if row.bit(font.width - 1 - dx) {
                    self.pixel(x + dx as i32, y + dy as i32, c);
                }
            }
        }
    }

    /// Draw `s` with its top-left at (x, y); returns the x after the last cell.
    pub fn text(&mut self, x: i32, y: i32, size: Size, c: u16, s: &str) -> i32 {
        let (cw, _) = size.cell();
        let mut xx = x;
        for ch in s.chars() {
            match size {
                Size::Small => self.glyph(&SPLEEN_8X16, xx, y, ch, c),
                Size::Medium => self.glyph(&SPLEEN_12X24, xx, y, ch, c),
                Size::Large => self.glyph(&SPLEEN_16X32, xx, y, ch, c),
                Size::Huge => self.glyph(&SPLEEN_32X64, xx, y, ch, c),
            }
            xx += cw;
        }
        xx
    }

    /// Right-aligned text ending at `right`.
    pub fn text_right(&mut self, right: i32, y: i32, size: Size, c: u16, s: &str) -> i32 {
        let x = right - size.cell().0 * s.chars().count() as i32;
        self.text(x, y, size, c, s);
        x
    }

    pub fn text_center(&mut self, cx: i32, y: i32, size: Size, c: u16, s: &str) {
        let x = cx - size.cell().0 * s.chars().count() as i32 / 2;
        self.text(x, y, size, c, s);
    }

    /// Segmented meter: `cells` blocks, the first `frac` of them lit.
    #[allow(clippy::too_many_arguments)]
    pub fn meter(&mut self, x: i32, y: i32, w: i32, h: i32, frac: f32, on: u16, off: u16) {
        let cells = (w / 5).max(1);
        let lit = ((frac.clamp(0.0, 1.0) * cells as f32) + 0.5) as i32;
        for i in 0..cells {
            let c = if i < lit { on } else { off };
            self.fill_rect(x + i * 5, y, 4, h, c);
        }
    }

    /// Line plot of `samples` (oldest first) scaled to `max`, right-aligned in the box,
    /// with a dim fill under the line.
    #[allow(clippy::too_many_arguments)]
    pub fn plot(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        samples: &[f32],
        max: f32,
        line: u16,
        fill: u16,
    ) {
        if samples.is_empty() || max <= 0.0 {
            return;
        }
        let step = w as f32 / (samples.len().max(2) - 1) as f32;
        let to_y = |v: f32| y + h - 1 - ((v / max).clamp(0.0, 1.0) * (h - 1) as f32) as i32;
        let mut prev: Option<(i32, i32)> = None;
        for (i, v) in samples.iter().enumerate() {
            let px = x + (i as f32 * step) as i32;
            let py = to_y(*v);
            if fill != line {
                self.vline(px, py + 1, y + h - py - 1, fill);
            }
            if let Some((qx, qy)) = prev {
                self.line(qx, qy, px, py, line);
            }
            prev = Some((px, py));
        }
    }

    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: u16) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        loop {
            self.pixel(x, y, c);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Mixed text on the 16 px grid: ASCII in Spleen 8x16, everything else
    /// from the CJK subset. Returns the x after the last glyph.
    pub fn text_cjk(&mut self, x: i32, y: i32, c: u16, s: &str) -> i32 {
        let mut xx = x;
        for ch in s.chars() {
            if ch.is_ascii() {
                self.glyph(&SPLEEN_8X16, xx, y, ch, c);
                xx += 8;
                continue;
            }
            let (w, rows) = crate::cjk::glyph(ch);
            for (dy, row) in rows.iter().enumerate() {
                for dx in 0..w {
                    if row >> (15 - dx) & 1 == 1 {
                        self.pixel(xx + dx, y + dy as i32, c);
                    }
                }
            }
            xx += w;
        }
        xx
    }

    /// Mix `c` over the pixel at (x, y) with opacity `alpha / 256`.
    pub fn blend(&mut self, x: i32, y: i32, c: u16, alpha: u32) {
        if (0..W).contains(&x) && (0..H).contains(&y) {
            let i = (y * W + x) as usize;
            self.px[i] = mix(self.px[i], c, alpha);
        }
    }

    pub fn blend_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16, alpha: u32) {
        let (x0, y0) = (x.max(0), y.max(0));
        let (x1, y1) = ((x + w).min(W), (y + h).min(H));
        for yy in y0..y1 {
            let row = (yy * W) as usize;
            for p in &mut self.px[row + x0 as usize..row + x1.max(x0) as usize] {
                *p = mix(*p, c, alpha);
            }
        }
    }

    /// Draw a palette-indexed sprite, each pixel `scale` x `scale`;
    /// `None` is transparent. `flip` mirrors it horizontally.
    #[allow(clippy::too_many_arguments)]
    pub fn sprite(&mut self, x: i32, y: i32, w: usize, px: &[Option<u16>], scale: i32, flip: bool) {
        for (i, p) in px.iter().enumerate() {
            let Some(c) = p else { continue };
            let (sx, sy) = ((i % w) as i32, (i / w) as i32);
            let sx = if flip { w as i32 - 1 - sx } else { sx };
            self.fill_rect(x + sx * scale, y + sy * scale, scale, scale, *c);
        }
    }

    /// CRT flavour: every third row slightly darker.
    /// CRT flavour, leaving the rectangle `keep` (x, y, w, h) untouched.
    pub fn scanlines_except(&mut self, keep: Option<(i32, i32, i32, i32)>) {
        for y in (2..H).step_by(3) {
            let row = (y * W) as usize;
            for (x, p) in self.px[row..row + W as usize].iter_mut().enumerate() {
                let x = x as i32;
                if keep.is_some_and(|(kx, ky, kw, kh)| {
                    (kx..kx + kw).contains(&x) && (ky..ky + kh).contains(&y)
                }) {
                    continue;
                }
                *p = dim(*p, 200);
            }
        }
    }

    /// Copy into a scanout buffer of `pitch` bytes per row, turned 180 degrees:
    /// the LCD is mounted upside down relative to the case.
    pub fn blit_rotated(&self, dst: &mut [u8], pitch: usize) {
        for y in 0..H as usize {
            let src = &self.px[y * W as usize..(y + 1) * W as usize];
            let out_row = (H as usize - 1 - y) * pitch;
            let out = &mut dst[out_row..out_row + W as usize * 2];
            for (x, p) in src.iter().enumerate() {
                let o = (W as usize - 1 - x) * 2;
                out[o..o + 2].copy_from_slice(&p.to_ne_bytes());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb565_packs_channels() {
        assert_eq!(rgb(255, 255, 255), 0xffff);
        assert_eq!(rgb(255, 0, 0), 0xf800);
        assert_eq!(rgb(0, 255, 0), 0x07e0);
        assert_eq!(dim(0xffff, 128), rgb(120, 124, 120));
    }

    #[test]
    fn mix_blends_each_channel() {
        assert_eq!(mix(0, 0xffff, 256), 0xffff);
        assert_eq!(mix(0xffff, 0, 256), 0);
        assert_eq!(mix(0x1234, 0xffff, 0), 0x1234);
        assert_eq!(
            mix(rgb(0, 0, 0), rgb(255, 255, 255), 128),
            rgb(120, 124, 120)
        );
    }

    #[test]
    fn cjk_text_advances_by_glyph_width() {
        let mut c = Canvas::new();
        assert_eq!(c.text_cjk(10, 10, 0xffff, "a鲸"), 10 + 8 + 16);
        assert!(c.px.contains(&0xffff));
    }

    #[test]
    fn drawing_is_clipped_to_the_frame() {
        let mut c = Canvas::new();
        c.fill_rect(-10, -10, 20, 20, 1);
        c.fill_rect(W - 5, H - 5, 50, 50, 2);
        c.text(W - 4, H - 4, Size::Large, 3, "clip");
        assert_eq!(c.px[0], 1);
        assert_eq!(c.px[(H * W - 1) as usize], 2);
    }

    #[test]
    fn rotation_maps_corners_and_respects_pitch() {
        let mut c = Canvas::new();
        c.pixel(0, 0, 0x1234);
        c.pixel(W - 1, 0, 0x5678);
        let pitch = W as usize * 2 + 64;
        let mut buf = vec![0u8; pitch * H as usize];
        c.blit_rotated(&mut buf, pitch);
        let at = |x: usize, y: usize| {
            u16::from_ne_bytes([buf[y * pitch + x * 2], buf[y * pitch + x * 2 + 1]])
        };
        assert_eq!(at(W as usize - 1, H as usize - 1), 0x1234);
        assert_eq!(at(0, H as usize - 1), 0x5678);
    }
}
