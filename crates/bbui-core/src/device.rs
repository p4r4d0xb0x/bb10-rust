//! Device shell: BB10 Screen/BPS loaded at runtime via dlopen (feature "device").
//!
//! No SDK linkage: dlopen/dlsym are stock libc.a (verified), libscreen/libbps
//! resolve on-device. All constants cross-checked against target_10_2_0_1155
//! screen.h + QNX Screen docs; unknowns (buffer map property) are probed at
//! runtime with fallbacks. `open`-style logging goes through a caller hook.

use core::mem::transmute;

#[allow(non_camel_case_types)]
pub type c_void = u8;

extern "C" {
    fn dlopen(path: *const u8, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const u8) -> *mut c_void;
    fn dlerror() -> *const u8;
    fn open(p: *const u8, o: i32, ...) -> i32;
    fn write(fd: i32, b: *const u8, n: usize) -> isize;
    fn time(t: *mut u64) -> u64;
}

const RTLD_LAZY: i32 = 0x0001;
const O_WRONLY: i32 = 0x1;
const O_CREAT: i32 = 0o400;
const O_TRUNC: i32 = 0x200; // QNX/POSIX value

// ---- Screen constants (verified in bb10qnx screen.h 10.2 / QNX docs) ----
pub const SC_APPLICATION_CONTEXT: i32 = 0;
pub const SC_CHILD_WINDOW: i32 = 1;
pub const SC_USAGE_READ: i32 = 1 << 1;
pub const SC_USAGE_WRITE: i32 = 1 << 2;
pub const SC_FORMAT_RGBA8888: i32 = 8;
pub const SC_PROPERTY_BUFFER_COUNT: i32 = 4;
pub const SC_PROPERTY_BUFFER_SIZE: i32 = 5;
pub const SC_PROPERTY_POSITION: i32 = 35;
pub const SC_PROPERTY_SIZE: i32 = 40;
pub const SC_PROPERTY_FORMAT: i32 = 14;
pub const SC_PROPERTY_USAGE: i32 = 48;
pub const SC_PROPERTY_VISIBLE: i32 = 51;
pub const SC_PROPERTY_STRIDE: i32 = 44;
pub const SC_PROPERTY_RENDER_BUFFER_COUNT: i32 = 53;
pub const SC_PROPERTY_RENDER_BUFFERS: i32 = 37;
pub const SC_PROPERTY_GROUP: i32 = 18;
pub const SC_PROPERTY_TYPE: i32 = 47; // event/window type (screen.h)
pub const SC_PROPERTY_POINTER: i32 = 34; // buffer CPU ptr (10.2 header)
pub const SC_EVENT_NONE: i32 = 0;
pub const SC_EVENT_MTOUCH_TOUCH: i32 = 100;
pub const SC_EVENT_MTOUCH_MOVE: i32 = 101;
pub const SC_EVENT_MTOUCH_RELEASE: i32 = 102;
pub const SC_PROPERTY_TOUCH_ID: i32 = 73;
pub const SC_EVENT_KEYBOARD: i32 = 7;
pub const SC_PROPERTY_KEY_SYM: i32 = 28;
pub const SC_WAIT_IDLE: i32 = 1 << 0;
// navigator event codes (bps/navigator.h)
pub const NAV_INVOKE: i32 = 0x01;
pub const NAV_EXIT: i32 = 0x02;
pub const NAV_WINDOW_STATE: i32 = 0x03;
pub const NAV_WINDOW_ACTIVE: i32 = 0x0a;
pub const NAV_WINDOW_INACTIVE: i32 = 0x0b;

// ---- fn pointer types ----
type FnCtx = unsafe extern "C" fn(*mut *mut c_void, i32) -> i32;
type FnWin = unsafe extern "C" fn(*mut *mut c_void, *mut c_void) -> i32;
type FnWinI = unsafe extern "C" fn(*mut c_void, i32) -> i32;
type FnPost = unsafe extern "C" fn(*mut c_void, *mut c_void, i32, *const i32, i32) -> i32;
type FnSetIV = unsafe extern "C" fn(*mut c_void, i32, *const i32) -> i32;
type FnSetCV = unsafe extern "C" fn(*mut c_void, i32, i32, *const u8) -> i32;
type FnGetIV = unsafe extern "C" fn(*mut c_void, i32, *mut i32) -> i32;
type FnGetPV = unsafe extern "C" fn(*mut c_void, i32, *mut *mut c_void) -> i32;
type FnGetEv = unsafe extern "C" fn(*mut c_void, *mut c_void, u64) -> i32;
type FnBGetEv = unsafe extern "C" fn(*mut *mut c_void, i32) -> i32;
type FnEvDom = unsafe extern "C" fn(*mut c_void) -> i32;
type FnEvCode = unsafe extern "C" fn(*mut c_void) -> i32;
type FnVoid = unsafe extern "C" fn() -> i32;
type FnStr = unsafe extern "C" fn(*mut c_void) -> *const u8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dev {
    None,
    ScreenEv,
    Touch { kind: i32, x: i32, y: i32, id: i32 },
    Key { sym: i32 },
    Nav { code: i32 },
    NavGroupId, // group id captured after Nav INVOKE/state
}

pub struct Shell {
    pub w: usize,
    pub h: usize,
    pub stride: usize, // pixels
    pub px: *mut u32,
    ctx: *mut c_void,
    win: *mut c_void,
    buf: *mut c_void,
    ev: *mut c_void,
    // libscreen
    s_destroy_ctx: FnWin, // reuse sig (ptr,extra)->int? no: define separately below
    create_window: FnWin,
    create_win_bufs: unsafe extern "C" fn(*mut c_void, i32) -> i32,
    post: FnPost,
    wait_post: unsafe extern "C" fn(*mut c_void, i32) -> i32,
    set_win_iv: FnSetIV,
    set_win_cv: FnSetCV,
    create_win_group: Option<unsafe extern "C" fn(*mut c_void, *const u8) -> i32>,
    join_win_grp: Option<unsafe extern "C" fn(*mut c_void, *const u8) -> i32>,
    create_window_type: unsafe extern "C" fn(*mut *mut c_void, *mut c_void, i32) -> i32,
    destroy_win: unsafe extern "C" fn(*mut c_void) -> i32,
    get_win_cv: unsafe extern "C" fn(*mut c_void, i32, i32, *mut u8) -> i32,
    get_win_iv: FnGetIV,
    get_win_pv: FnGetPV,
    get_buf_iv: FnGetIV,
    get_buf_pv: FnGetPV,
    get_event: FnGetEv,
    get_ev_iv: FnSetIV, // (ev, pname, *mut i32) same layout as set iv
    // libbps (optional; null if not loaded)
    pub bps: bool,
    b_init: Option<FnVoid>,
    b_get_event: Option<FnBGetEv>,
    b_ev_domain: Option<FnEvDom>,
    b_ev_code: Option<FnEvCode>,
    scr_get_domain: Option<FnVoid>,
    scr_request: Option<unsafe extern "C" fn(*mut c_void) -> i32>,
    scr_ev_from_bps: Option<unsafe extern "C" fn(*mut c_void) -> *mut c_void>,
    nav_get_domain: Option<FnVoid>,
    nav_request: Option<unsafe extern "C" fn(i32) -> i32>,
    nav_groupid: Option<FnStr>,
    nav_win_group: Option<unsafe extern "C" fn(*mut *mut u8) -> i32>,
    pub group: [u8; 128], // group id bytes, NUL-terminated (windowgroup:// URIs up to ~70 chars)
    group_len: usize,
    joined: bool,
    parented: bool,  // our window itself parents a group -> no cv-join needed
    binit_rc: i32,   // bps_initialize() rc (999 = never called)
    navreq_rc: i32,  // navigator_request_events() rc (999 = never called)
    lfd: i32,
}

const NULL: *mut c_void = core::ptr::null_mut();

#[inline]
fn cstr<const N: usize>(buf: &mut [u8; N], s: &[u8]) -> *const u8 {
    buf[..s.len()].copy_from_slice(s);
    buf[s.len()] = 0;
    buf.as_ptr()
}

unsafe fn sym(h: *mut c_void, name: &[u8]) -> *mut c_void {
    let mut b = [0u8; 64];
    dlsym(h, cstr(&mut b, name))
}

impl Shell {
    /// mode: true = use BPS event pump (navigator-launched app),
    /// false = pure libscreen blocking events (headless test).
    /// Open the diagnostic log. First choice is the SHARED documents dir
    /// (/accounts/1000/shared/documents/logs/exp_app.log): reachable from
    /// BOTH devuser uid=100 (devmode apps) and per-app sandbox uid (devmode
    /// false apps, which cannot write /var/tmp at all).
    fn open_diag_log() -> i32 {
        extern "C" {
            fn mkdir(p: *const u8, m: u32) -> i32;
        }
        // ensure dir (already exists on stock BB10; no-op otherwise)
        unsafe {
            mkdir(b"/accounts/1000/shared/documents/logs\0".as_ptr(), 0o777);
        }
        const CAND: [&[u8]; 2] = [
            b"/accounts/1000/shared/documents/logs/exp_app.log\0",
            b"/var/tmp/bbui.log\0",
        ];
        for c in CAND {
            let fd = unsafe { open(c.as_ptr(), O_WRONLY | O_CREAT | O_TRUNC, 0o666) };
            if fd >= 0 {
                return fd;
            }
        }
        -1
    }

    pub unsafe fn init(w: usize, h: usize, use_bps: bool) -> Option<Shell> {
        // O_TRUNC+0666: previous instance may be owned by another uid (apps
        // vs devuser) -> plain O_CREAT open then FAILS silently and every
        // subsequent log line vanishes (observed: tap-run log empty).
        let mut lfd = Self::open_diag_log();
        let lg = |lfd: i32, s: &[u8]| {
            if lfd >= 0 {
                write(lfd, s.as_ptr(), s.len());
            }
        };
        lg(lfd, b"init: enter\n");

        let mut hs: *mut c_void = dlopen(b"/usr/lib/libscreen.so.1\0".as_ptr(), RTLD_LAZY);
        lg(lfd, b"try /usr/lib\n");
        if hs.is_null() {
            let e = dlerror();
            if !e.is_null() {
                let mut buf = [0u8; 200];
                let mut n = 0usize;
                while n < 199 && *e.add(n) != 0 { buf[n] = *e.add(n); n += 1; }
                lg(lfd, &buf[..n]);
                lg(lfd, b"\n");
            }
        }
        if hs.is_null() {
            hs = dlopen(b"/base/usr/lib/libscreen.so.1\0".as_ptr(), RTLD_LAZY);
            lg(lfd, b"try /base/usr/lib\n");
        }
        if hs.is_null() {
            hs = dlopen(b"libscreen.so.1\0".as_ptr(), RTLD_LAZY);
            lg(lfd, b"try bare\n");
        }
        if hs.is_null() {
            lg(lfd, b"init: dlopen libscreen FAIL all paths\n");
            return None;
        }
        let create_ctx: *mut c_void = sym(hs, b"screen_create_context");
        if create_ctx.is_null() {
            lg(lfd, b"init: no screen_create_context\n");
            return None;
        }
        let mk = |n: &[u8]| sym(hs, n);

        macro_rules! fp {
            ($n:expr, $ty:ty) => {{
                let p = mk($n);
                if p.is_null() {
                    lg(lfd, b"init: missing sym\n");
                    return None;
                }
                transmute::<*mut c_void, $ty>(p)
            }};
        }
        macro_rules! optfp {
            ($n:expr, $ty:ty) => {{
                let p = mk($n);
                if p.is_null() { None } else { Some(transmute::<*mut c_void, $ty>(p)) }
            }};
        }

        let mut bps = false;
        let mut hb: *mut c_void = core::ptr::null_mut();
        let mut hb3: *mut c_void = core::ptr::null_mut();
        if use_bps {
            hb = dlopen(b"/usr/lib/libbps.so.1\0".as_ptr(), RTLD_LAZY);
            if hb.is_null() {
                hb = dlopen(b"/base/usr/lib/libbps.so.1\0".as_ptr(), RTLD_LAZY);
            }
            if hb.is_null() {
                hb = dlopen(b"libbps.so.1\0".as_ptr(), RTLD_LAZY);
            }
            // navigator_* symbols live in the NEWER libbps.so.3 on this build
            hb3 = dlopen(b"/usr/lib/libbps.so.3\0".as_ptr(), RTLD_LAZY);
            if hb3.is_null() {
                hb3 = dlopen(b"/base/usr/lib/libbps.so.3\0".as_ptr(), RTLD_LAZY);
            }
            if hb.is_null() && hb3.is_null() {
                lg(lfd, b"init: dlopen libbps FAIL all paths\n");
            } else {
                bps = true;
            }
        }
        // bps/navigator symbols resolve against the libbps handles, NOT libscreen.
        // IMPORTANT: prefer so.3 for EVERYTHING — so.1 and so.3 are separate
        // implementations with private state; mixing bps_initialize(so1) with
        // navigator_*(so3) crashes (verified: crash right after 'shell up').
        let mkb = |n: &[u8]| -> *mut c_void {
            if !hb3.is_null() {
                let p = sym(hb3, n);
                if !p.is_null() {
                    return p;
                }
            }
            if !hb.is_null() {
                return sym(hb, n);
            }
            core::ptr::null_mut()
        };
        macro_rules! mbopt {
            ($n:expr, $ty:ty) => {{
                let p = mkb($n);
                if p.is_null() { None } else { Some(transmute::<*mut c_void, $ty>(p)) }
            }};
        }

        let mut sh = Shell {
            w, h, stride: w, px: core::ptr::null_mut(),
            ctx: NULL, win: NULL, buf: NULL, ev: NULL,
            s_destroy_ctx: fp!(b"screen_destroy_context\0", FnWin),
            create_window: fp!(b"screen_create_window\0", FnWin),
            create_win_bufs: fp!(b"screen_create_window_buffers\0", unsafe extern "C" fn(*mut c_void, i32) -> i32),
            post: fp!(b"screen_post_window\0", FnPost),
            wait_post: fp!(b"screen_wait_post\0", unsafe extern "C" fn(*mut c_void, i32) -> i32),
            set_win_iv: fp!(b"screen_set_window_property_iv\0", FnSetIV),
            set_win_cv: fp!(b"screen_set_window_property_cv\0", FnSetCV),
            create_win_group: optfp!(b"screen_create_window_group\0", unsafe extern "C" fn(*mut c_void, *const u8) -> i32),
            join_win_grp: optfp!(b"screen_join_window_group\0", unsafe extern "C" fn(*mut c_void, *const u8) -> i32),
            create_window_type: fp!(b"screen_create_window_type\0", unsafe extern "C" fn(*mut *mut c_void, *mut c_void, i32) -> i32),
            destroy_win: fp!(b"screen_destroy_window\0", unsafe extern "C" fn(*mut c_void) -> i32),
            get_win_cv: fp!(b"screen_get_window_property_cv\0", unsafe extern "C" fn(*mut c_void, i32, i32, *mut u8) -> i32),
            get_win_iv: fp!(b"screen_get_window_property_iv\0", FnGetIV),
            get_win_pv: fp!(b"screen_get_window_property_pv\0", FnGetPV),
            get_buf_iv: fp!(b"screen_get_buffer_property_iv\0", FnGetIV),
            get_buf_pv: fp!(b"screen_get_buffer_property_pv\0", FnGetPV),
            get_event: fp!(b"screen_get_event\0", FnGetEv),
            get_ev_iv: fp!(b"screen_get_event_property_iv\0", FnSetIV),
            bps,
            b_init: mbopt!(b"bps_initialize\0", FnVoid),
            b_get_event: mbopt!(b"bps_get_event\0", FnBGetEv),
            b_ev_domain: mbopt!(b"bps_event_get_domain\0", FnEvDom),
            b_ev_code: mbopt!(b"bps_event_get_code\0", FnEvCode),
            scr_get_domain: mbopt!(b"screen_get_domain\0", FnVoid),
            scr_request: mbopt!(b"screen_request_events\0", unsafe extern "C" fn(*mut c_void) -> i32),
            scr_ev_from_bps: mbopt!(b"screen_event_get_event\0", unsafe extern "C" fn(*mut c_void) -> *mut c_void),
            nav_get_domain: mbopt!(b"navigator_get_domain\0", FnVoid),
            nav_request: mbopt!(b"navigator_request_events\0", unsafe extern "C" fn(i32) -> i32),
            nav_groupid: mbopt!(b"navigator_event_get_groupid\0", FnStr),
            nav_win_group: mbopt!(b"navigator_get_window_group\0", unsafe extern "C" fn(*mut *mut u8) -> i32),
            group: [0u8; 128], group_len: 0, joined: false,
            parented: false,
            binit_rc: 999, navreq_rc: 999, lfd,
        };
        // note: bps=false but hb non-null means syms still resolvable:
        if !bps && (!hb.is_null() || !hb3.is_null()) {
            sh.b_init = mbopt!(b"bps_initialize\0", FnVoid);
            sh.b_get_event = mbopt!(b"bps_get_event\0", FnBGetEv);
            sh.b_ev_domain = mbopt!(b"bps_event_get_domain\0", FnEvDom);
            sh.b_ev_code = mbopt!(b"bps_event_get_code\0", FnEvCode);
            sh.scr_get_domain = mbopt!(b"screen_get_domain\0", FnVoid);
            sh.scr_request = mbopt!(b"screen_request_events\0", unsafe extern "C" fn(*mut c_void) -> i32);
            sh.scr_ev_from_bps = mbopt!(b"screen_event_get_event\0", unsafe extern "C" fn(*mut c_void) -> *mut c_void);
            sh.nav_get_domain = mbopt!(b"navigator_get_domain\0", FnVoid);
            sh.nav_request = mbopt!(b"navigator_request_events\0", unsafe extern "C" fn(i32) -> i32);
            sh.nav_groupid = mbopt!(b"navigator_event_get_groupid\0", FnStr);
            sh.nav_win_group = mbopt!(b"navigator_get_window_group\0", unsafe extern "C" fn(*mut *mut u8) -> i32);
        }

        let cc: FnCtx = transmute(create_ctx);
        if cc(&mut sh.ctx, SC_APPLICATION_CONTEXT) != 0 {
            lg(lfd, b"init: create_context FAIL\n");
            return None;
        }
        if (sh.create_window)(&mut sh.win, sh.ctx) != 0 {
            lg(lfd, b"init: create_window FAIL\n");
            return None;
        }
        let bufs = [2i32];
        (sh.set_win_iv)(sh.win, SC_PROPERTY_BUFFER_COUNT, bufs.as_ptr());
        let sz = [w as i32, h as i32];
        (sh.set_win_iv)(sh.win, SC_PROPERTY_BUFFER_SIZE, sz.as_ptr());
        let fmt = [SC_FORMAT_RGBA8888];
        (sh.set_win_iv)(sh.win, SC_PROPERTY_FORMAT, fmt.as_ptr());
        // READ|WRITE: compositor must be ABLE to sample the buffer; a
        // WRITE-only buffer posts fine (rc=0) but composites BLACK (observed:
        // white probe 'post ok' yet screen stayed black).
        let usage = [SC_USAGE_READ | SC_USAGE_WRITE];
        (sh.set_win_iv)(sh.win, SC_PROPERTY_USAGE, usage.as_ptr());
        let vis = [1i32];
        (sh.set_win_iv)(sh.win, SC_PROPERTY_VISIBLE, vis.as_ptr());
        if (sh.create_win_bufs)(sh.win, 2) != 0 {
            lg(lfd, b"init: create_window_buffers FAIL\n");
            return None;
        }

        // get first render buffer
        let mut bufs_arr = [NULL; 2];
        if (sh.get_win_pv)(sh.win, SC_PROPERTY_RENDER_BUFFERS, bufs_arr.as_mut_ptr() as *mut *mut c_void) != 0
            || bufs_arr[0].is_null()
        {
            lg(lfd, b"init: get RENDER_BUFFERS FAIL\n");
            return None;
        }
        sh.buf = bufs_arr[0];

        // stride (bytes) -> pixels
        let mut stride_b: i32 = 0;
        (sh.get_buf_iv)(sh.buf, SC_PROPERTY_STRIDE, &mut stride_b);
        if stride_b >= (w * 4) as i32 {
            sh.stride = (stride_b / 4) as usize;
        }

        // CPU pointer: try POINTER first, then RENDER_BUFFERS-as-mapped (10.3)
        let mut p: *mut c_void = NULL;
        if (sh.get_buf_pv)(sh.buf, SC_PROPERTY_POINTER, &mut p) != 0 || p.is_null() {
            lg(lfd, b"buf POINTER null; trying front-buffer fallback\n");
        }
        if p.is_null() {
            // 10.3 libscreen may map render buffers into app memory at create
            (sh.get_buf_pv)(sh.buf, SC_PROPERTY_RENDER_BUFFERS, &mut p);
        }
        if p.is_null() {
            lg(lfd, b"init: buffer NOT CPU-visible (need external buffer path)\n");
            return None;
        }
        sh.px = p as *mut u32;

        let create_event: FnWin = fp!(b"screen_create_event\0", FnWin);
        if create_event(&mut sh.ev, sh.ctx) != 0 {
            lg(lfd, b"init: create_event FAIL\n");
            return None;
        }

        if bps && sh.b_init.is_some() {
            let bi = sh.b_init.unwrap();
            sh.binit_rc = bi();
            if sh.binit_rc != 0 {
                lg(lfd, b"bps_initialize FAIL\n");
                sh.bps = false;
            } else {
                if let (Some(rd), Some(req)) = (sh.scr_get_domain, sh.scr_request) {
                    let _ = rd();
                    let _ = req(sh.ctx);
                }
                if let Some(nr) = sh.nav_request {
                    sh.navreq_rc = nr(0);
                }
                lg(lfd, b"init: bps pump armed\n");
            }
        } else {
            lg(lfd, if !bps { b"init: bps SKIPPED (dlopen fail)\n" } else { b"init: bps SKIPPED (bps_initialize sym NULL)\n" });
        }
        lg(lfd, b"init: OK\n");
        Some(sh)
    }

    #[inline]
    pub unsafe fn log(&self, s: &[u8]) {
        if self.lfd >= 0 {
            write(self.lfd, s.as_ptr(), s.len());
        }
    }

    /// (bps_initialize rc, navigator_request_events rc); 999 = never called.
    #[inline]
    pub fn nav_init_rc(&self) -> (i32, i32) {
        (self.binit_rc, self.navreq_rc)
    }

    /// True once window joined navigator group (app visible). Pure-screen
    /// mode treats join as unnecessary (standalone window).
    pub fn has_group(&self) -> bool {
        self.group_len > 0
    }

    #[inline]
    pub fn is_joined(&self) -> bool {
        self.joined
    }

    /// Pump one event with timeout_ms. timeout<=0 => non-blocking poll.
    /// Pure mode: blocking wait with ms (0 = try once).
    pub unsafe fn pump(&mut self, timeout_ms: i32) -> Dev {
        if self.bps && self.b_get_event.is_some() {
            let bg = self.b_get_event.unwrap();
            let mut bev: *mut c_void = NULL;
            if bg(&mut bev, timeout_ms.max(1)) != 0 || bev.is_null() {
                return Dev::None;
            }
            let dom = (self.b_ev_domain.unwrap())(bev);
            let code = (self.b_ev_code.unwrap())(bev);
            if Some(dom) == self.scr_get_domain.map(|f| f()) {
                if let Some(conv) = self.scr_ev_from_bps {
                    let sev = conv(bev);
                    if !sev.is_null() {
                        return self.decode_screen(sev);
                    }
                }
                Dev::ScreenEv
            } else if Some(dom) == self.nav_get_domain.map(|f| f()) {
                if code == NAV_INVOKE || code == NAV_WINDOW_STATE || code == NAV_WINDOW_ACTIVE {
                    if let Some(gf) = self.nav_groupid {
                        let gs = gf(bev);
                        if !gs.is_null() {
                            // literal "none" = navigator has NO registered
                            // group for us (devmode launch symptom). Do NOT
                            // treat as a group — would mask the fallback path.
                            // "none\0": n@0 o@1 n@2 e@3 NUL@4 (off-by-one fix)
                            if *gs == b'n' && *gs.add(1) == b'o'
                                && *gs.add(2) == b'n' && *gs.add(3) == b'e'
                                && *gs.add(4) == b'\0'
                            {
                                self.log(b"nav gid='none' (unregistered)\n");
                            } else {
                                self.copy_group(gs);
                            }
                        }
                    }
                }
                Dev::Nav { code }
            } else {
                Dev::None
            }
        } else {
            // pure screen blocking event
            let us = (timeout_ms.max(1) as u64) * 1_000_000;
            if (self.get_event)(self.ctx, self.ev, us) != 0 {
                let mut ty: i32 = SC_EVENT_NONE;
                (self.get_ev_iv)(self.ev, SC_PROPERTY_TYPE, &mut ty);
                if ty == SC_EVENT_NONE {
                    return Dev::None;
                }
            }
            self.decode_screen(self.ev)
        }
    }

    /// Direct group acquisition WITHOUT waiting for Nav INVOKE:
    /// navigator_get_window_group() returns "windowgroup://<app-id>" for the
    /// CURRENT process (derived from app env), no event round-trip needed.
    /// Observed on device: home-screen tap launches never deliver INVOKE to
    /// our bps pump, so INVOKE-based join alone can never fire.
    pub unsafe fn try_group_direct(&mut self) -> bool {
        // Step 1: does our window ALREADY have a group (get_window_property_cv
        // SCREEN_PROPERTY_GROUP)? Native apps that skip navigator usually land
        // in an "auto-..." group — joining THAT is valid and visible.
        let mut gname = [0u8; 128];
        let grc = (self.get_win_cv)(self.win, SC_PROPERTY_GROUP, 128, gname.as_mut_ptr());
        self.log_int(b"owngrp rc=", grc);
        if grc == 0 {
            // NUL-terminated string in gname
            let mut n = 0;
            while n < 127 && gname[n] != 0 {
                n += 1;
            }
            if n > 0 {
                self.log(b"owngrp name=");
                let mut gb = [0u8; 130];
                gb[..n].copy_from_slice(&gname[..n]);
                gb[n] = b'\n';
                self.log(&gb[..n + 1]);
                self.copy_group(gname.as_ptr());
                return true;
            }
        }
        // Step 2: parent a NEW group on our own window (NULL -> auto-name).
        // Makes our window the group parent so it is composited as top-level
        // regardless of navigator.
        let cgrc = match self.create_win_group {
            Some(cg) => cg(self.win, core::ptr::null()),
            None => {
                self.log(b"create_win_group sym NULL\n");
                -1
            }
        };
        self.log(b"create_win_group rc=");
        self.log_int(b"", cgrc);
        if cgrc == 0 {
            self.parented = true;
            let grc = (self.get_win_cv)(self.win, SC_PROPERTY_GROUP, 128, gname.as_mut_ptr());
            if grc == 0 {
                let mut n = 0;
                while n < 127 && gname[n] != 0 {
                    n += 1;
                }
                if n > 0 {
                    self.log(b"newgrp name=");
                    let mut gb = [0u8; 130];
                    gb[..n].copy_from_slice(&gname[..n]);
                    gb[n] = b'\n';
                    self.log(&gb[..n + 1]);
                    self.copy_group(gname.as_ptr());
                    return true;
                }
            }
        }
        false
    }

    /// log "prefix<num>\n" with decimal i32 (ksh-free formatting)
    pub unsafe fn log_int(&self, prefix: &[u8], v: i32) {
        self.log(prefix);
        let mut nb = [0u8; 14];
        let mut n = 0usize;
        let av = if v < 0 {
            nb[n] = b'-';
            n += 1;
            -v
        } else {
            v
        };
        let mut tmp = [0u8; 11];
        let mut t = 0usize;
        if av == 0 {
            tmp[t] = b'0';
            t += 1;
        }
        let mut x = av;
        while x > 0 {
            tmp[t] = b'0' + (x % 10) as u8;
            x /= 10;
            t += 1;
        }
        while t > 0 {
            t -= 1;
            nb[n] = tmp[t];
            n += 1;
        }
        nb[n] = b'\n';
        self.log(&nb[..n + 1]);
    }

    /// Force a literal group string (last-resort hardcoded app-id fallback).
    pub unsafe fn set_group_str(&mut self, s: &[u8]) {
        let n = core::cmp::min(s.len(), 127);
        self.group[..n].copy_from_slice(&s[..n]);
        self.group[n] = 0;
        self.group_len = n;
        self.joined = false;
    }

    /// Try group candidates until Screen accepts one (log per attempt).
    pub unsafe fn join_candidates(&mut self, cands: &[&[u8]]) -> bool {
        self.parented = false;
        for c in cands {
            self.set_group_str(c);
            if self.join_group() {
                self.log(b"joined candidate\n");
                return true;
            }
        }
        false
    }

    /// SDL (libSDL12 = Term49's render backend, proven to display on this
    /// device) structure: application window parents a group AND a CHILD
    /// window joins that group; the child carries the visible content.
    /// An application window that merely parents a group renders BLACK on
    /// BB10 WM — only group-member child windows get composited.
    /// Returns true if child is live and self.win/px/buf now target it.
    pub unsafe fn promote_child_render(&mut self) -> bool {
        if self.create_win_group.is_none() || self.join_win_grp.is_none() {
            self.log(b"promote: syms NULL\n");
            return false;
        }
        // 1) parent a group on the app window (auto name)
        if !self.parented {
            if let Some(cg) = self.create_win_group {
                if cg(self.win, core::ptr::null()) != 0 {
                    self.log(b"promote: create_group FAIL\n");
                    return false;
                }
                self.parented = true;
            }
        }
        // 2) read group name from parent window
        let mut gname = [0u8; 128];
        if (self.get_win_cv)(self.win, SC_PROPERTY_GROUP, 128, gname.as_mut_ptr()) != 0 {
            self.log(b"promote: get group name FAIL\n");
            return false;
        }
        let mut n = 0;
        while n < 127 && gname[n] != 0 {
            n += 1;
        }
        self.log(b"promote: group=");
        let mut gb = [0u8; 130];
        gb[..n].copy_from_slice(&gname[..n]);
        gb[n] = b'\n';
        self.log(&gb[..n + 1]);
        // 3) create CHILD window
        let mut child: *mut c_void = NULL;
        if (self.create_window_type)(&mut child, self.ctx, SC_CHILD_WINDOW) != 0 {
            self.log(b"promote: create child FAIL\n");
            return false;
        }
        // 4) join child into group
        let jg = self.join_win_grp.unwrap();
        if jg(child, gname.as_ptr()) != 0 {
            self.log(b"promote: join child FAIL\n");
            (self.destroy_win)(child);
            return false;
        }
        // 5) configure child buffers (READ|WRITE so compositor can sample)
        let bufs = [2i32];
        (self.set_win_iv)(child, SC_PROPERTY_BUFFER_COUNT, bufs.as_ptr());
        let sz = [self.w as i32, self.h as i32];
        (self.set_win_iv)(child, SC_PROPERTY_BUFFER_SIZE, sz.as_ptr());
        let fmt = [SC_FORMAT_RGBA8888];
        (self.set_win_iv)(child, SC_PROPERTY_FORMAT, fmt.as_ptr());
        let usage = [SC_USAGE_READ | SC_USAGE_WRITE];
        (self.set_win_iv)(child, SC_PROPERTY_USAGE, usage.as_ptr());
        let pos = [0i32, 0i32];
        (self.set_win_iv)(child, SC_PROPERTY_POSITION, pos.as_ptr());
        if (self.create_win_bufs)(child, 2) != 0 {
            self.log(b"promote: child bufs FAIL\n");
            (self.destroy_win)(child);
            return false;
        }
        let mut carr = [NULL; 2];
        if (self.get_win_pv)(child, SC_PROPERTY_RENDER_BUFFERS, carr.as_mut_ptr() as *mut *mut c_void) != 0
            || carr[0].is_null()
        {
            self.log(b"promote: child RENDER_BUFFERS FAIL\n");
            (self.destroy_win)(child);
            return false;
        }
        let mut p: *mut c_void = NULL;
        if (self.get_buf_pv)(carr[0], SC_PROPERTY_POINTER, &mut p) != 0 || p.is_null() {
            self.log(b"promote: child POINTER FAIL\n");
            (self.destroy_win)(child);
            return false;
        }
        self.buf = carr[0];
        self.px = p as *mut u32;
        self.win = child;
        let vis = [1i32];
        (self.set_win_iv)(self.win, SC_PROPERTY_VISIBLE, vis.as_ptr());
        self.joined = true;
        self.log(b"promote: child live\n");
        true
    }

    /// True when our window parents its own auto group (nav unregistered).
    #[inline]
    pub fn is_parented(&self) -> bool {
        self.parented
    }

    fn copy_group(&mut self, gs: *const u8) {
        let mut i = 0;
        while i < 127 {
            let c = unsafe { *gs.add(i) };
            self.group[i] = c;
            if c == 0 { break; }
            i += 1;
        }
        // never leave self.group unterminated: downstream scans read NUL or
        // index i, both must be in-bounds
        if i >= 127 {
            self.group[126] = 0;
            i = 126;
        }
        self.group[i] = 0;
        self.group_len = i;
        self.joined = false;
    }

    unsafe fn decode_screen(&self, ev: *mut c_void) -> Dev {
        let mut ty: i32 = SC_EVENT_NONE;
        (self.get_ev_iv)(ev, SC_PROPERTY_TYPE, &mut ty);
        match ty {
            SC_EVENT_MTOUCH_TOUCH | SC_EVENT_MTOUCH_MOVE | SC_EVENT_MTOUCH_RELEASE => {
                let mut id: i32 = 0;
                (self.get_ev_iv)(ev, SC_PROPERTY_TOUCH_ID, &mut id);
                let mut xy = [0i32; 2];
                (self.get_ev_iv)(ev, SC_PROPERTY_POSITION, xy.as_mut_ptr());
                Dev::Touch { kind: ty, x: xy[0], y: xy[1], id }
            }
            SC_EVENT_KEYBOARD => {
                let mut sym: i32 = 0;
                (self.get_ev_iv)(ev, SC_PROPERTY_KEY_SYM, &mut sym);
                Dev::Key { sym }
            }
            other => Dev::Nav { code: 0x1000 + other }, // raw screen event passthrough
        }
    }

    /// Join current navigator group (must be called after Nav INVOKE captured
    /// group id). Returns true if visible-app window achieved.
    pub unsafe fn join_group(&mut self) -> bool {
        if self.parented {
            // our window IS the group parent -> already composited top-level
            self.joined = true;
            return true;
        }
        if self.group_len == 0 {
            return false;
        }
        let name = &self.group[..self.group_len];
        // group id may contain '0x...;' prefix junk: strip to last hexword
        let s = strip_group(name);
        let rc = (self.set_win_cv)(
            self.win,
            SC_PROPERTY_GROUP,
            s.len() as i32,
            s.as_ptr(),
        );
        if rc == 0 {
            self.joined = true;
            self.log(b"join_group ok\n");
        } else {
            self.log(b"join_group FAIL rc=");
            let mut nb = [0u8; 16];
            let mut n = 0usize;
            let mut v = if rc < 0 { -rc } else { rc };
            if rc < 0 { nb[n] = b'-'; n += 1; }
            let mut tmp = [0u8; 12];
            let mut t = 0usize;
            if v == 0 { tmp[t] = b'0'; t += 1; }
            while v > 0 { tmp[t] = b'0' + (v % 10) as u8; v /= 10; t += 1; }
            while t > 0 && n < 15 { t -= 1; nb[n] = tmp[t]; n += 1; }
            nb[n] = b' ';
            n += 1;
            self.log(&nb[..n]);
            self.log(b"gid=");
            let mut gb = [0u8; 80];
            let gl = core::cmp::min(self.group_len, 70);
            gb[..gl].copy_from_slice(&self.group[..gl]);
            gb[gl] = b'\n';
            self.log(&gb[..gl + 1]);
        }
        rc == 0
    }

    /// Blit-free: caller owns self.px as &mut [u32] and renders; then post.
    pub unsafe fn post_full(&mut self) -> i32 {
        (self.post)(self.win, self.buf, 0, core::ptr::null(), 0)
    }

    pub unsafe fn wait_idle(&mut self) {
        (self.wait_post)(self.win, SC_WAIT_IDLE);
    }

    #[inline]
    pub unsafe fn px_slice(&mut self) -> &mut [u32] {
        core::slice::from_raw_parts_mut(self.px, self.stride * self.h)
    }
}

/// strip leading "0x…;" prefix from BB10 group id ("com…;0x2" forms)
fn strip_group(g: &[u8]) -> &[u8] {
    // BB10 window group: "0x10000001;" style — Screen expects the raw string
    // the app received. We pass through as-is except trim trailing NULs.
    let mut end = g.len();
    while end > 0 && g[end - 1] == 0 {
        end -= 1;
    }
    &g[..end]
}
