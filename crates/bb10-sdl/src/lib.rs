//! bb10-sdl — safe-ish Rust bindings to BlackBerry 10's bundled `libSDL12.so`.
//!
//! This is THE proven native rendering path on BB10 (QNX): Term49 and the
//! FreeGAG/StuntCarRacer ports all reach the screen through SDL, because the
//! PLAYBOOK video driver handles the navigator window-group join internally
//! (a hand-rolled libscreen window renders *behind* the splash and never
//! composites — see docs/04-sdl-rendering.md).
//!
//! Link model: `libSDL12.so` must arrive as a **DT_NEEDED** entry of the
//! executable (supplied to the linker on the command line by
//! `toolkit/build.sh`). Never dlopen it at runtime — its deps resolve
//! differently and it fails.
//!
//! CRITICAL: the binary crate MUST expand [`sdl_link_prims!`] once. The stubs
//! live in the final executable's own object file; putting them in this rlib
//! silently drops them at link time (`--allow-shlib-undefined` means the
//! linker never pulls archive members to satisfy shared-lib imports).
//!
//! ```text
//! use bb10_sdl::{self as sdl, sdl_link_prims};
//! sdl_link_prims!();
//! let s = sdl::init(720, 720)?;      // SDL_Init + SDL_SetVideoMode
//! s.fb[..].copy_from_slice(..);       // draw straight into the surface
//! s.flip();                           // SDL_Flip (page flip, double buffer)
//! while let Some(ev) = sdl::poll_event() { }
//! ```
#![no_std]
#![allow(clippy::missing_safety_doc)]

extern crate core;

/// SDL_Init subsystem flags (SDL 1.2).
pub const INIT_VIDEO: u32 = 0x0000_0020;
/// HW + double-buffered: required by the PLAYBOOK flip path.
pub const HWSURFACE: u32 = 0x0000_0001;
pub const DOUBLEBUF: u32 = 0x4000_0000;

// ---------------------------------------------------------------------------
// FFI. libSDL12.so is linked as DT_NEEDED; direct calls, no dlsym.
// ---------------------------------------------------------------------------
extern "C" {
    fn SDL_Init(flags: u32) -> i32;
    fn SDL_SetVideoMode(w: i32, h: i32, bpp: i32, flags: u32) -> *mut u8;
    fn SDL_Flip(surf: *mut u8) -> i32;
    fn SDL_PumpEvents();
    fn SDL_PollEvent(ev: *mut u8) -> i32;
    fn SDL_GetMouseState(x: *mut i32, y: *mut i32) -> u8;
    fn SDL_Quit();
    fn SDL_GetError() -> *const u8;
    fn setenv(name: *const u8, val: *const u8, overwrite: i32) -> i32;
}

// SDL 1.2 `SDL_Surface` field offsets, ARM 32-bit — {flags@0 format@4 w@8
// h@12 pitch@16 pixels@20 hwdata@24}, cross-checked on-device against
// PLAYBOOK_FlipHWSurface (reads hwdata) and the 720x720/pitch2880 surface.
const OFF_W: usize = 8;
const OFF_H: usize = 12;
const OFF_PITCH: usize = 16;
const OFF_PIXELS: usize = 20;

/// SDL 1.2 event codes.
const EV_MOTION: u8 = 0x04;
const EV_DOWN: u8 = 0x05;
const EV_UP: u8 = 0x06;
const EV_QUIT: u8 = 0x0C;

/// A rendered, flippable framebuffer window.
pub struct Sdl {
    surf: *mut u8,
    /// Direct view on the surface's back buffer (RGBX, u32 pixels).
    pub fb: &'static mut [u32],
    /// Stride in **pixels** (surface pitch / 4).
    pub stride: usize,
    pub w: i32,
    pub h: i32,
}

/// Where init failed + SDL's message, for bring-up on a screen-remote device.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InitError {
    SdlInit { rc: i32 },
    NoSurface,
    BadSurface { w: i32, h: i32, pitch: i32 },
}

impl InitError {
    /// SDL_GetError() message copied into `buf`; returns its length.
    pub fn message(self, buf: &mut [u8]) -> usize {
        unsafe {
            let e = SDL_GetError();
            let mut n = 0;
            if !e.is_null() {
                while n < buf.len() && *e.add(n) != 0 {
                    buf[n] = *e.add(n);
                    n += 1;
                }
            }
            n
        }
    }
}

/// `SDL_Init(VIDEO)` + `SDL_SetVideoMode(w, h, 32, HWSURFACE|DOUBLEBUF)`.
///
/// The env `SDL_VIDEODRIVER=playbook` is set for you (matches Term49; the
/// device build has only that driver anyway). The caller's `LD_LIBRARY_PATH`
/// must already cover the bundled `native/lib` plus `/proc/boot` (libm) and
/// `/base/lib` (libasound) — see `toolkit/pack_bar.py --wrapper`.
///
/// # Safety
/// FFI into SDL; the returned `fb` aliases the driver's back buffer and is
/// valid only while no other SDL call runs (fine — single-threaded).
pub unsafe fn init(w: i32, h: i32) -> Result<Sdl, InitError> {
    setenv(b"SDL_VIDEODRIVER\0".as_ptr(), b"playbook\0".as_ptr(), 1);
    let rc = SDL_Init(INIT_VIDEO);
    if rc != 0 {
        return Err(InitError::SdlInit { rc });
    }
    let surf = SDL_SetVideoMode(w, h, 32, HWSURFACE | DOUBLEBUF);
    if surf.is_null() {
        return Err(InitError::NoSurface);
    }
    let sw = *(surf.add(OFF_W) as *const i32);
    let sh = *(surf.add(OFF_H) as *const i32);
    let pitch = *(surf.add(OFF_PITCH) as *const i32);
    let pixels = *(surf.add(OFF_PIXELS) as *const *mut u32);
    if pixels.is_null() || pitch <= 0 || sw <= 0 || sh <= 0 {
        return Err(InitError::BadSurface { w: sw, h: sh, pitch });
    }
    let stride = (pitch / 4) as usize;
    let fb = core::slice::from_raw_parts_mut(pixels, stride * sh as usize);
    Ok(Sdl { surf, fb, stride, w: sw, h: sh })
}

impl Sdl {
    /// Present the back buffer (page flip).
    pub fn flip(&self) -> i32 {
        unsafe { SDL_Flip(self.surf) }
    }
    /// Raw SDL_Surface pointer (for diagnostic logging).
    pub fn surf_ptr(&self) -> usize {
        self.surf as usize
    }
}

/// Keyboard/mouse state pump (PLAYBOOK driver pulls bps/screen events here).
pub fn pump_events() {
    unsafe { SDL_PumpEvents() }
}

/// One SDL event, decoded from the 1.2 union.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    /// Mouse/touch button went down at (x, y).
    Down { x: i32, y: i32 },
    Up { x: i32, y: i32 },
    Motion { x: i32, y: i32 },
    /// Window got a quit signal.
    Quit,
    /// Any other code (key events etc.) — raw type byte + coords if present.
    Other(u8),
}

/// Pop one queued event (call until None after [`pump_events`]).
pub fn poll_event() -> Option<Event> {
    // SDL_Event union max size for 1.2 on 32-bit ARM.
    let mut ev = [0u8; 56];
    let (got, code, x, y) = unsafe {
        let got = SDL_PollEvent(ev.as_mut_ptr());
        if got == 0 {
            return None;
        }
        (got, ev[0], mx(ev.as_ptr()), my(ev.as_ptr()))
    };
    let _ = got;
    match code {
        EV_QUIT => Some(Event::Quit),
        EV_MOTION => Some(Event::Motion { x, y }),
        EV_DOWN => Some(Event::Down { x, y }),
        EV_UP => Some(Event::Up { x, y }),
        c => Some(Event::Other(c)),
    }
}

// SDL 1.2 mouse events: x@4 (Sint16), y@6 (Sint16).
#[inline]
unsafe fn mx(p: *const u8) -> i32 {
    *(p.add(4) as *const i16) as i32
}
#[inline]
unsafe fn my(p: *const u8) -> i32 {
    *(p.add(6) as *const i16) as i32
}

/// Instant (x, y, button-mask) — bit0 = left/held. The PLAYBOOK driver maps
/// raw touches through SDL's mouse layer; mask & 1 != 0 means currently down.
pub fn mouse_state() -> (i32, i32, u8) {
    let mut x = 0i32;
    let mut y = 0i32;
    let b = unsafe { SDL_GetMouseState(&mut x, &mut y) };
    (x, y, b)
}

/// Tear down SDL subsystems.
pub fn quit() {
    unsafe { SDL_Quit() }
}

/// Emit into THIS crate (the executable) every symbol libSDL12.so and the
/// QNX GLES libs import from the main program. **Must be expanded exactly
/// once in each binary's own source** (see module docs — the linker will not
/// pull these out of an rlib).
#[macro_export]
macro_rules! sdl_link_prims {
    () => {
        // libSDL12 playbook driver input callbacks (Term49 exports these;
        // missing exports abort inside SDL_Init via lazy PLT binding).
        #[no_mangle]
        pub extern "C" fn handleKeyboardEvent(ev: *mut u8) -> i32 {
            let _ = ev;
            0
        }
        #[no_mangle]
        pub extern "C" fn handle_virtualkeyboard_event(ev: *mut u8) -> i32 {
            let _ = ev;
            0
        }
        #[no_mangle]
        pub extern "C" fn indicate_event_input(ev: *mut u8) -> i32 {
            let _ = ev;
            0
        }
        #[no_mangle]
        pub extern "C" fn lock_input() -> i32 {
            0
        }
        #[no_mangle]
        pub extern "C" fn unlock_input() {}

        // GCC frame registry: QNX libGLESv1_CM/libEGL import these from the
        // executable (gnueabi libgcc would normally provide them at link).
        #[no_mangle]
        pub extern "C" fn __register_frame_info(_begin: *const u8, _obj: *mut u8) {}
        #[no_mangle]
        pub extern "C" fn __deregister_frame_info(_begin: *const u8) {}
        #[no_mangle]
        pub extern "C" fn __register_frame(_begin: *const u8) {}
        #[no_mangle]
        pub extern "C" fn __deregister_frame(_begin: *const u8) {}

        // EGL trace hooks referenced by the bundled GLES drivers.
        #[no_mangle]
        pub extern "C" fn _egl_tls() -> *mut u8 {
            ::core::ptr::null_mut()
        }
        #[no_mangle]
        pub extern "C" fn _egl_default_context() -> *mut u8 {
            ::core::ptr::null_mut()
        }
    };
}
