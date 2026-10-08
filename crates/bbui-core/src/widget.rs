//! Widget primitives: immediate-mode button, list row, status pill, gauge.
//! Geometry + hit-test are pure; rendering uses Canvas.

use crate::canvas::Canvas;
use crate::color;
use crate::event::inside;
use crate::fmt;

pub const ROW_H: i32 = 28;

/// Hit-test helper reused by the app's tap dispatch.
pub fn hit(id: u8, x: i32, y: i32, rx: i32, ry: i32, rw: i32, rh: i32) -> Option<u8> {
    if inside(x, y, rx, ry, rw, rh) {
        Some(id)
    } else {
        None
    }
}

pub fn button(cv: &mut Canvas, x: i32, y: i32, w: i32, h: i32, label: &[u8], accent: bool) {
    let bg = if accent { color::ACCENT } else { color::PANEL };
    cv.round_rect(x, y, w, h, 6, bg);
    let cx = x + w / 2 - (label.len() as i32 * 12) / 2;
    cv.text(cx, y + h / 2 + 8, label, color::TEXT);
}

/// Left-labeled value row. `colored` dot at x=16, label from 36.
pub fn row(cv: &mut Canvas, y: i32, label: &[u8], value: &[u8], dot: u32) {
    cv.fill_rect(16, y + 9, 10, 10, dot);
    cv.text(36, y + 19, label, color::DIM);
    // value right-aligned at width 720-16
    let vw = value.len() as i32 * 12;
    cv.text(720 - 16 - vw, y + 19, value, color::TEXT);
}

/// Title bar with bottom hairline.
pub fn titlebar(cv: &mut Canvas, title: &[u8], status_dot: u32) {
    cv.fill_rect(0, 0, 720, 56, color::PANEL);
    cv.hline(0, 56, 720, color::LINE);
    cv.text(20, 38, title, color::TEXT);
    cv.fill_rect(720 - 30, 24, 12, 12, status_dot);
}

/// Two-column stat grid: label above value, rounded panel bg.
pub fn stat(cv: &mut Canvas, x: i32, y: i32, w: i32, h: i32, label: &[u8], value: &[u8]) {
    cv.round_rect(x, y, w, h, 8, color::PANEL);
    cv.text(x + 14, y + 24, label, color::DIM);
    cv.text(x + 14, y + 54, value, color::ACCENT);
}

/// Progress + numeric like "40.9 Kbps".
pub fn gauge(
    cv: &mut Canvas,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    pct1000: u32,
    text: &[u8],
) {
    cv.round_rect(x, y, w, h, 6, color::PANEL_HI);
    cv.bar(x + 8, y + 8, w - 16, h - 30, pct1000, color::LINE, color::ACCENT);
    cv.text(x + 8, y + h - 10, text, color::TEXT);
}

/// 7-digit-free uptime: "3d 04:11:07" centered big.
pub fn uptime(cv: &mut Canvas, y: i32, secs: u64) {
    let mut b = [0u8; 24];
    let d = fmt::duration(secs, &mut b);
    cv.text_center(y, d, color::TEXT);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hit_works() {
        assert_eq!(hit(7, 100, 100, 90, 90, 50, 50), Some(7));
        assert_eq!(hit(7, 140, 100, 90, 90, 50, 50), None);
    }
    #[test]
    fn row_renders() {
        let mut buf = [0u32; 720 * 30];
        let mut cv = Canvas::new(720, 30, &mut buf);
        row(&mut cv, 0, b"handshake", b"12s", color::OK);
        assert!(cv.px.iter().any(|&p| p == color::OK));
        assert!(cv.px.iter().any(|&p| p == color::DIM));
        assert!(cv.px.iter().any(|&p| p == color::TEXT));
    }
    #[test]
    fn gauge_pct() {
        let mut buf = [0u32; 200 * 40];
        let mut cv = Canvas::new(200, 40, &mut buf);
        gauge(&mut cv, 0, 0, 200, 40, 333, b"33%");
        // left of bar filled with accent, right side panel_hi (not accent)
        assert!(cv.px.iter().any(|&p| p == color::ACCENT));
    }
}
