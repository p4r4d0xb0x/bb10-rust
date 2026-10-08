//! Software canvas: fixed-size RGBX buffer, blit primitives, text, boxes.
//! Pure — no unsafe. The device shell copies its mapped Screen buffer into
//! the canvas or renders directly over a &mut slice of it.

use crate::color;
use crate::font::{BASELINE, GLYPH_H, GLYPH_W, ROWS};

pub struct Canvas<'a> {
    pub w: usize,
    pub h: usize,
    pub stride: usize, // in pixels
    pub px: &'a mut [u32],
}

impl<'a> Canvas<'a> {
    pub fn new(w: usize, h: usize, px: &'a mut [u32]) -> Self {
        Self { w, h, stride: w, px }
    }

    #[inline]
    fn inb(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h
    }

    #[inline]
    pub fn put(&mut self, x: i32, y: i32, c: u32) {
        if self.inb(x, y) {
            self.px[(y as usize) * self.stride + x as usize] = c;
        }
    }

    pub fn fill(&mut self, c: u32) {
        let n = self.h * self.stride;
        for p in &mut self.px[..n] {
            *p = c;
        }
    }

    /// Axis-aligned rect, clipped.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        for yy in y..(y + h) {
            for xx in x..(x + w) {
                self.put(xx, yy, c);
            }
        }
    }

    /// Rect with alpha: a in 0..=255 (0 = no-op, 255 = opaque).
    pub fn fill_rect_a(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32, a: u8) {
        for yy in y..(y + h) {
            for xx in x..(x + w) {
                if self.inb(xx, yy) {
                    let i = (yy as usize) * self.stride + xx as usize;
                    self.px[i] = color::blend(self.px[i], color::with_alpha(c, a));
                }
            }
        }
    }

    pub fn hline(&mut self, x: i32, y: i32, w: i32, c: u32) {
        self.fill_rect(x, y, w, 1, c);
    }

    pub fn round_rect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, c: u32) {
        for dy in -r..0 {
            // corners: shrink width by (r - sqrt(r^2-dy^2)) via integer approx
            let cut = r - isqrt(r * r - dy * dy);
            self.fill_rect(x + cut, y + r + dy, w - 2 * cut, 1, c);
            self.fill_rect(x + cut, y + h - 1 - r - dy, w - 2 * cut, 1, c);
        }
        self.fill_rect(x, y + r, w, h - 2 * r, c);
    }

    /// Draw 1 glyph with baseline at (x, y). Returns advance (GLYPH_W).
    pub fn draw_glyph(&mut self, x: i32, y_base: i32, g: &[u8], c: u32) {
        for gy in 0..GLYPH_H {
            let sy = y_base - BASELINE as i32 + gy as i32;
            for gx in 0..GLYPH_W {
                if g[gy * ROWS + gx / 8] & (0x80 >> (gx % 8)) != 0 {
                    self.put(x + gx as i32, sy, c);
                }
            }
        }
    }

    /// Draw mixed ASCII+Hangul string (UTF-8, 1 scalar = 1 cell). Unsupported
    /// scalars render as '?'. Returns end x.
    pub fn text(&mut self, x: i32, y_base: i32, s: &[u8], c: u32) -> i32 {
        let mut cx = x;
        let mut i = 0usize;
        while i < s.len() {
            let (cp, n) = crate::hangul::decode(&s[i..]);
            if cp < 0x80 {
                match crate::font::glyph(s[i]) {
                    Some(g) => self.draw_glyph(cx, y_base, g, c),
                    None => {}
                }
            } else if let Some(g) = crate::hangul::glyph_syllable(cp) {
                self.draw_glyph(cx, y_base, g, c);
            } else if let Some(g) = crate::font::glyph(b'?') {
                self.draw_glyph(cx, y_base, g, c);
            }
            cx += GLYPH_W as i32;
            i += n;
        }
        cx
    }

    /// Display width in cells of a mixed ASCII+Hangul UTF-8 string.
    pub fn cells(s: &[u8]) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < s.len() {
            i += crate::hangul::decode(&s[i..]).1;
            n += 1;
        }
        n
    }

    pub fn text_center(&mut self, cy: i32, s: &[u8], c: u32) {
        let w = (Self::cells(s) * GLYPH_W) as i32;
        self.text((self.w as i32 - w) / 2, cy, s, c);
    }

    /// Horizontal bar: 0..=1000 (0.0%..100.0% in tenths).
    pub fn bar(&mut self, x: i32, y: i32, w: i32, h: i32, pct1000: u32, bg: u32, fg: u32) {
        self.fill_rect(x, y, w, h, bg);
        let fw = (w as u32 * pct1000.min(1000) / 1000) as i32;
        self.fill_rect(x, y, fw, h, fg);
    }
}

/// Integer sqrt (floor), pure-integer Newton (no_std: core has no f64::sqrt).
pub fn isqrt(v: i32) -> i32 {
    if v <= 1 {
        return if v < 0 { 0 } else { v };
    }
    let mut x = v;
    loop {
        let y = (x + v / x) / 2;
        if y >= x {
            return x;
        }
        x = y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clip() {
        let mut buf = [0u32; 8 * 4];
        let mut cv = Canvas::new(8, 4, &mut buf);
        cv.fill_rect(-2, -2, 4, 4, 0xff000000);
        assert_eq!(cv.px[0], 0xff000000); // (0,0) inside
        cv.fill_rect(100, 100, 10, 10, 0x11); // fully outside, no panic
    }
    #[test]
    fn glyph_a_ink() {
        let mut buf = [0u32; 24 * 22];
        let mut cv = Canvas::new(24, 22, &mut buf);
        cv.text(0, 19, b"A", 0xff000000); // baseline at 19
        let row = (0..24).filter(|&x| buf[18 * 24 + x] != 0).count(); // cap-bottom row
        assert!(row >= 4, "row18={}", row);
        let top = (0..24).filter(|&x| buf[4 * 24 + x] != 0).count(); // cap-top row
        assert!(top >= 2, "row4={}", top);
    }
    #[test]
    fn isqrt_correct() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(9), 3);
        assert_eq!(isqrt(14), 3);
        assert_eq!(isqrt(100), 10);
    }
    #[test]
    fn bar_clamps() {
        let mut buf = [0u32; 32];
        let mut cv = Canvas::new(10, 2, &mut buf);
        cv.bar(0, 0, 10, 1, 500, 0, 0xff000000);
        assert_eq!(cv.px[4], 0xff000000);
        assert_eq!(cv.px[5], 0);
        cv.bar(0, 1, 10, 1, 99999, 0, 0xff000000);
        assert_eq!(cv.px[2 * 10 - 1], 0xff000000);
    }
    #[test]
    fn round_rect_fits() {
        let mut buf = [0u32; 20 * 10];
        let mut cv = Canvas::new(20, 10, &mut buf);
        cv.round_rect(2, 2, 16, 6, 3, 0xab);
        // top-left corner carved: (2,2) empty, (5,2) filled
        assert_eq!(cv.px[2 * 20 + 2], 0);
        assert_eq!(cv.px[2 * 20 + 5], 0xab);
        // body center filled
        assert_eq!(cv.px[5 * 20 + 10], 0xab);
    }
}
