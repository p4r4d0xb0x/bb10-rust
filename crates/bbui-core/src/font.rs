//! Bitmap font accessors over the generated 1bpp strip (tools/gen_font.py).

include!("font_data.rs");

/// Row bytes of glyph `c` (ASCII), or None out of range. Space (0x20) is a
/// zero cell — that's the whole point of space.
pub fn glyph(c: u8) -> Option<&'static [u8]> {
    if c >= FIRST && c <= LAST {
        let idx = (c - FIRST) as usize;
        Some(&FONT[idx][..])
    } else {
        None
    }
}

/// Is this ASCII printable in our strip (incl. space)?
pub fn supported(c: u8) -> bool {
    (FIRST..=LAST).contains(&c)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strip_bounds() {
        assert!(glyph(b'A').is_some());
        assert!(glyph(0x7f).is_none());
        assert!(glyph(b' ').unwrap().iter().all(|&b| b == 0));
        // 'A' must have ink
        assert!(glyph(b'A').unwrap().iter().any(|&b| b != 0));
    }
    #[test]
    fn baseline_consistency() {
        // Cell 12x22 (ROWS=2 bytes/row), BASELINE=19: caps span rows 4..=18,
        // descenders to row 21, rows 0..2 always empty.
        let ink = |c: u8, row: usize| -> bool {
            let g = glyph(c).unwrap();
            g[row * ROWS] != 0 || g[row * ROWS + 1] != 0
        };
        for c in 32..=126 {
            assert!(!ink(c, 0) && !ink(c, 1) && !ink(c, 2), "glyph {} top", c);
        }
        assert!(ink(b'M', 4) && ink(b'M', BASELINE - 1)); // caps: 4..=18
        assert!(!ink(b'M', 3) && !ink(b'M', BASELINE));
        assert!(ink(b'g', BASELINE + 1) && ink(b'g', GLYPH_H - 1)); // descender
        assert!(!(0..GLYPH_H).any(|r| ink(b' ', r)));
    }
}
