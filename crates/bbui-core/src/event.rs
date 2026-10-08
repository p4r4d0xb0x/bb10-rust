//! Input event model: Screen MTOUCH/keyboard mapped to a tiny enum so the
//! pure layer stays testable without FFI. The device shell converts raw
//! screen events into these.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ev {
    TouchDown { x: i32, y: i32 },
    TouchMove { x: i32, y: i32 },
    TouchUp { x: i32, y: i32 },
    /// BB10 keyboard KEY_SYM (unicode); 0 = ignore.
    Key { sym: i32 },
    /// App lifecycle (navigator window state).
    Active,
    Inactive,
    /// Display orientation/idle changes the shell may ignore.
    Tick { now: u64 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    None,
    Press { id: u8 },
    Release { id: u8 },
    Tap { id: u8 },
    DragStart { id: u8, x: i32, y: i32 },
    DragMove { id: u8, x: i32, y: i32 },
    DragEnd { id: u8, x: i32, y: i32 },
}

#[inline]
pub fn inside(x: i32, y: i32, rx: i32, ry: i32, rw: i32, rh: i32) -> bool {
    x >= rx && x < rx + rw && y >= ry && y < ry + rh
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inside_rect() {
        assert!(inside(5, 5, 0, 0, 10, 10));
        assert!(!inside(10, 5, 0, 0, 10, 10));
    }
}
