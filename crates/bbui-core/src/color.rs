//! 32bpp pixel layout for BB10 Screen RGBA_8888 buffers.
//!
//! AUTHORITATIVE CONTRACT: memory bytes are R,G,B,A (what SCREEN_FORMAT_RGBA_8888
//! expects). On little-endian armv7 the u32 word is therefore
//! `(A<<24)|(B<<16)|(G<<8)|R`. All canvas/widget code and unit tests use rgba()
//! so the layout stays consistent end-to-end; a wrong guess is ONE line to flip.

pub const TRANSPARENT: u32 = 0;

// ---- dashboard theme (Classic dark chassis) ----
pub const BG: u32 = rgb(0x10, 0x14, 0x18);
pub const PANEL: u32 = rgb(0x1b, 0x22, 0x29);
pub const PANEL_HI: u32 = rgb(0x24, 0x2d, 0x37);
pub const LINE: u32 = rgb(0x2a, 0x34, 0x3e);
pub const TEXT: u32 = rgb(0xe8, 0xed, 0xf2);
pub const DIM: u32 = rgb(0x8a, 0x98, 0xa6);
pub const ACCENT: u32 = rgb(0x2f, 0x8a, 0xd8);
pub const OK: u32 = rgb(0x3d, 0xb3, 0x5c);
pub const WARN: u32 = rgb(0xe0, 0xa0, 0x30);
pub const BAD: u32 = rgb(0xd8, 0x4a, 0x4a);

#[inline]
pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((a as u32) << 24) | ((b as u32) << 16) | ((g as u32) << 8) | r as u32
}

#[inline]
pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    rgba(r, g, b, 255)
}

#[inline]
pub const fn with_alpha(px: u32, a: u8) -> u32 {
    (px & 0x00ff_ffff) | ((a as u32) << 24)
}

#[inline]
pub const fn alpha(px: u32) -> u8 {
    (px >> 24) as u8
}

/// Source-over blend, a in 0..=255 (per-channel, straight alpha).
#[inline]
pub fn blend(bg: u32, fg: u32) -> u32 {
    let a = (fg >> 24) as u32;
    if a == 0 {
        return bg;
    }
    if a == 255 {
        return fg | 0xff00_0000;
    }
    let ia = 255 - a;
    let r = ((((bg) & 0xff) * ia + ((fg >> 0) & 0xff) * a) / 255) & 0xff;
    let g = ((((bg >> 8) & 0xff) * ia + ((fg >> 8) & 0xff) * a) / 255) & 0xff;
    let b = ((((bg >> 16) & 0xff) * ia + ((fg >> 16) & 0xff) * a) / 255) & 0xff;
    r | (g << 8) | (b << 16) | (0xff << 24)
}

/// Mix two opaque colors 50/50.
pub fn mix(c1: u32, c2: u32) -> u32 {
    let r = ((c1 & 0xff) + (c2 & 0xff)) / 2;
    let g = (((c1 >> 8) & 0xff) + ((c2 >> 8) & 0xff)) / 2;
    let b = (((c1 >> 16) & 0xff) + ((c2 >> 16) & 0xff)) / 2;
    rgb(r as u8, g as u8, b as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_byte_order_is_rgba() {
        let px = rgba(0x11, 0x22, 0x33, 0x44);
        let bytes = px.to_le_bytes(); // device reads memory
        assert_eq!(bytes, [0x11, 0x22, 0x33, 0x44]);
    }
    #[test]
    fn blend_endpoints() {
        assert_eq!(blend(0xdeadbeef, rgba(1, 2, 3, 0)), 0xdeadbeef);
        let m = blend(0xdeadbeef, rgba(1, 2, 3, 255));
        assert_eq!(m & 0x00ff_ffff, rgba(1, 2, 3, 0) & 0x00ff_ffff);
        assert_eq!(m >> 24, 0xff);
    }
    #[test]
    fn blend_mid_is_close_to_half() {
        let m = blend(rgb(0, 0, 0), rgba(255, 255, 255, 128));
        assert!((((m >> 0) & 0xff) as i32 - 128).abs() <= 2);
        assert!((((m >> 8) & 0xff) as i32 - 128).abs() <= 2);
        assert!((((m >> 16) & 0xff) as i32 - 128).abs() <= 2);
    }
    #[test]
    fn mix_endpoints_and_mid() {
        assert_eq!(mix(rgb(0, 0, 0), rgb(255, 255, 255)), rgb(127, 127, 127));
        assert_eq!(mix(rgb(127, 128, 129), rgb(255, 254, 253)), rgb(191, 191, 191));
    }
}
