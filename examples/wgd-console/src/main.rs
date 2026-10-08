//! wgd-console — WireGuard daemon dashboard for BlackBerry Classic.
//!
//! Device-proven reference app for the bb10-rust pipeline: renders through
//! libSDL12 (bb10_sdl) at 720x720, watches the on-device watchdog's log and
//! control files, and sends commands back through drop-files. Built with
//! `toolkit/build.sh wgd-console examples/wgd-console` (never plain cargo).
#![no_std]
#![no_main]

extern crate core;
extern crate bbui_core;
extern crate bb10_sdl;

use bb10_sdl as sdl;
use bbui_core::canvas::Canvas;
use bbui_core::color;
use bbui_core::event::inside;
use bbui_core::fmt;
use bbui_core::widget;

// CRITICAL: prims must live in the executable's own object (see bb10-sdl
// module docs). Expanding here is what makes SDL_Init survive.
sdl::sdl_link_prims!();

extern "C" {
    fn open(p: *const u8, o: i32, ...) -> i32;
    fn read(fd: i32, b: *mut u8, n: usize) -> isize;
    fn write(fd: i32, b: *const u8, n: usize) -> isize;
    fn close(fd: i32) -> i32;
    fn access(p: *const u8, m: i32) -> i32;
    fn _exit(c: i32) -> !;
    fn time(t: *mut u64) -> u64;
    fn stat(p: *const u8, st: *mut u8) -> i32;
    fn getpid() -> i32;
    fn getuid() -> i32;
    fn socket(d: i32, t: i32, p: i32) -> i32;
    fn ioctl(fd: i32, c: u32, a: *mut u8) -> i32;
    fn usleep(u: i32) -> i32;
    fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
}

const O_WRONLY: i32 = 0o1;
const O_CREAT: i32 = 0o400;
const O_TRUNC: i32 = 0o1000;
const AF_INET: i32 = 2;
const SOCK_DGRAM: i32 = 2;
const SIOCGIFFLAGS: u32 = 0xC020_6911;

const W: i32 = 720;
const H: i32 = 720;

static mut LOG_BUF: [u8; 4096] = [0; 4096];
static mut LOG_LEN: usize = 0;
static mut LOG_MTIME: u64 = 0;
static mut TOAST: [u8; 40] = [0; 40];
static mut TOAST_LEN: usize = 0;
static mut TOAST_T: u64 = 0;

// All under /accounts/devuser (world-rwx dir): the single source of truth
// the watchdog already maintains — no duplicated copies to keep in sync.
const LOG_PATH: &[u8] = b"/accounts/devuser/wgd.log\0";
const PID_PATH: &[u8] = b"/accounts/devuser/wgd_watch.pid\0";
const CMD_REBIND: &[u8] = b"/accounts/devuser/wgcmd_rebind\0";
const CMD_FT: &[u8] = b"/accounts/devuser/wgcmd_ft\0";
const FT_PATH: &[u8] = b"/accounts/devuser/wgd_fulltunnel.on\0";
const BOOT_LOG: &[u8] = b"/accounts/1000/shared/documents/logs/exp_sdl.log\0";

// bring-up log: every step is visible from SSH with zero device-side poking
static mut LFD: i32 = -1;
unsafe fn dlog(s: &[u8]) {
    if LFD >= 0 {
        write(LFD, s.as_ptr(), s.len());
    }
}
unsafe fn dlog_int(prefix: &[u8], v: i64) {
    dlog(prefix);
    let mut nb = [0u8; 24];
    let mut n = 0usize;
    let av = if v < 0 { nb[n] = b'-'; n += 1; -v } else { v };
    let mut tmp = [0u8; 20];
    let mut t = 0usize;
    if av == 0 { tmp[t] = b'0'; t += 1; }
    let mut x = av;
    while x > 0 { tmp[t] = b'0' + (x % 10) as u8; x /= 10; t += 1; }
    while t > 0 { t -= 1; nb[n] = tmp[t]; n += 1; }
    nb[n] = b'\n';
    dlog(&nb[..n + 1]);
}

// Crash forensics: fatal signals get logged before exit (bring-up aid).
extern "C" fn crash_h(sig: i32) {
    unsafe {
        dlog(b"SIGNAL ");
        dlog_int(b"", sig as i64);
        _exit(90 + sig);
    }
}
fn install_crash_handlers() {
    unsafe {
        signal(11, crash_h); // SIGSEGV
        signal(6, crash_h);  // SIGABRT
        signal(7, crash_h);  // SIGBUS
        signal(4, crash_h);  // SIGILL
        signal(8, crash_h);  // SIGFPE
    }
}

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! {
    unsafe {
        let fd = open(
            b"/accounts/1000/shared/documents/logs/exp_sdl.panic\0".as_ptr(),
            O_WRONLY | O_CREAT | O_TRUNC,
            0o666,
        );
        if fd >= 0 {
            let msg = b"panic in wgd-console\n";
            write(fd, msg.as_ptr(), msg.len());
            close(fd);
        }
        _exit(101)
    }
}

// --- daemon-side readers (pure file/ioctl shell; no SDL here) ---------------

unsafe fn mtime_of(path: &[u8]) -> u64 {
    let mut stbuf = [0u8; 128];
    if stat(path.as_ptr(), stbuf.as_mut_ptr()) != 0 {
        return 0;
    }
    let now = time(core::ptr::null_mut());
    let mut best = 0u64;
    for off in (0..=124).step_by(4) {
        let v = (stbuf[off] as u64)
            | ((stbuf[off + 1] as u64) << 8)
            | ((stbuf[off + 2] as u64) << 16)
            | ((stbuf[off + 3] as u64) << 24);
        if v > 1_700_000_000 && v <= now + 31_557_600 {
            best = v;
        }
    }
    best
}

unsafe fn read_tail(path: &[u8]) {
    LOG_LEN = 0;
    LOG_MTIME = mtime_of(path);
    let fd = open(path.as_ptr(), 0, 0);
    if fd < 0 {
        return;
    }
    // streaming two-window tail: keep the last full 4KB without seeking
    let mut chunk = [0u8; 4096];
    let mut carry = [0u8; 4096];
    let mut carry_n = 0usize;
    loop {
        let n = read(fd, chunk.as_mut_ptr(), 4096);
        if n <= 0 {
            break;
        }
        let nl = n as usize;
        if nl >= 4096 {
            carry[..4096].copy_from_slice(&chunk);
            carry_n = 4096;
        } else {
            let prev = core::cmp::min(carry_n, 4096 - nl);
            let mut tmp = [0u8; 8192];
            tmp[..prev].copy_from_slice(&carry[carry_n - prev..]);
            tmp[prev..prev + nl].copy_from_slice(&chunk[..nl]);
            LOG_BUF[..prev + nl].copy_from_slice(&tmp[..prev + nl]);
            LOG_LEN = prev + nl;
            close(fd);
            return;
        }
    }
    let take = core::cmp::min(carry_n, 4096);
    LOG_BUF[..take].copy_from_slice(&carry[..take]);
    LOG_LEN = take;
    close(fd);
}

fn line_starts(log: &[u8], max: usize) -> ([usize; 12], usize) {
    let mut starts = [0usize; 12];
    let mut count = 1usize;
    let mut i = 0usize;
    while i < log.len() && count < max {
        if log[i] == b'\n' {
            starts[count] = i + 1;
            count += 1;
        }
        i += 1;
    }
    (starts, count)
}

/// Age of the last line containing `needle`, using its HH:MM:SS stamp.
unsafe fn last_age(log: &[u8], needle: &[u8], now: u64) -> Option<u64> {
    if log.len() < 9 || needle.len() + 1 > log.len() {
        return None;
    }
    let mut i = log.len() - needle.len() - 1;
    if needle.len() > 0 {
        loop {
            if log[i] == needle[0] && i + needle.len() <= log.len() {
                let mut ok = true;
                for k in 0..needle.len() {
                    if log[i + k] != needle[k] {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    let ls = i.saturating_sub(40);
                    let mut j = ls;
                    while j + 8 < i {
                        if log[j + 8] == b':' && log[j + 5] == b':' && (log[j] ^ b'0') <= 9 {
                            let mut secs = 0u64;
                            let mut good = true;
                            for k in 0..8 {
                                if k == 2 || k == 5 {
                                    continue;
                                }
                                let c = log[j + k];
                                if (c ^ b'0') > 9 {
                                    good = false;
                                    break;
                                }
                                secs = secs * 10 + (c - b'0') as u64;
                            }
                            if good {
                                return Some(now.saturating_sub(secs));
                            }
                        }
                        j += 1;
                    }
                    return None;
                }
            }
            if i == 0 {
                break;
            }
            i -= 1;
        }
    }
    None
}

unsafe fn pid_alive(pid_bytes: &[u8]) -> bool {
    let mut path = [0u8; 64];
    let mut i = 0;
    for b in b"/proc/" {
        path[i] = *b;
        i += 1;
    }
    let n = core::cmp::min(pid_bytes.len(), 40);
    path[i..i + n].copy_from_slice(&pid_bytes[..n]);
    i += n;
    for b in b"/as" {
        path[i] = *b;
        i += 1;
    }
    path[i] = 0;
    access(path.as_ptr(), 0) == 0
}

unsafe fn read_pid(path: &[u8], out: &mut [u8]) -> usize {
    let fd = open(path.as_ptr(), 0, 0);
    if fd < 0 {
        return 0;
    }
    let n = read(fd, out.as_mut_ptr(), out.len() - 1);
    close(fd);
    if n <= 0 {
        return 0;
    }
    let mut m = n as usize;
    if let Some(sp) = out[..m].iter().position(|&b| b == b' ' || b == b'\n') {
        m = sp;
    }
    m
}

unsafe fn write_cmd(path: &[u8], s: &[u8]) {
    let fd = open(path.as_ptr(), O_WRONLY | O_CREAT | O_TRUNC, 0o666);
    if fd >= 0 {
        write(fd, s.as_ptr(), s.len());
        close(fd);
    }
}

unsafe fn toast(s: &[u8], now: u64) {
    let n = core::cmp::min(s.len(), TOAST.len());
    TOAST[..n].copy_from_slice(&s[..n]);
    TOAST_LEN = n;
    TOAST_T = now;
}

unsafe fn tun_up() -> Option<bool> {
    let fd = socket(AF_INET, SOCK_DGRAM, 0);
    if fd < 0 {
        return None;
    }
    let mut ir = [0u8; 32];
    ir[0] = b't';
    ir[1] = b'u';
    ir[2] = b'n';
    ir[3] = b'0';
    let rc = ioctl(fd, SIOCGIFFLAGS, ir.as_mut_ptr());
    close(fd);
    if rc != 0 {
        return Some(false);
    }
    let flags = (ir[16] as u16) | ((ir[17] as u16) << 8);
    Some(flags & 0x1 == 1)
}

fn clean_line<'a>(buf: &'a mut [u8; 60], line: &[u8]) -> &'a [u8] {
    let mut n = 0;
    for &b in line {
        if n + 1 >= buf.len() {
            break;
        }
        match b {
            b'\r' | b'\0' => continue,
            0x20..=0x7e => buf[n] = b,
            _ => buf[n] = b'.',
        }
        n += 1;
    }
    &buf[..n]
}

// --- presentation -----------------------------------------------------------

unsafe fn render(now: u64, fb: &mut [u32], stride: usize) {
    let mut cv = Canvas::new(W as usize, H as usize, fb);
    cv.stride = stride;
    cv.fill(color::BG);

    let alive = LOG_MTIME != 0 && now.saturating_sub(LOG_MTIME) < 180;
    widget::titlebar(&mut cv, b"wgd console", if alive { color::OK } else { color::BAD });

    widget::stat(&mut cv, 16, 72, 344, 76, b"DAEMON", if alive { b"alive" } else { b"STALE >180s" });
    match tun_up() {
        Some(true) => widget::stat(&mut cv, 376, 72, 328, 76, b"tun0", b"UP"),
        Some(false) => widget::stat(&mut cv, 376, 72, 328, 76, b"tun0", b"DOWN"),
        None => widget::stat(&mut cv, 376, 72, 328, 76, b"tun0", b"? (ioctl)"),
    }

    let logs = &*core::ptr::addr_of!(LOG_BUF);
    let log = &logs[..LOG_LEN];

    let mut y = 168;
    let mut vb = [0u8; 28];
    match last_age(log, b"HANDSHAKE-OK", now) {
        Some(a) => {
            let d = fmt::duration(a, &mut vb);
            widget::row(&mut cv, y, b"last handshake", &d[..d.len()], if a < 300 { color::OK } else { color::WARN });
        }
        None => widget::row(&mut cv, y, b"last handshake", b"never seen", color::WARN),
    }
    y += 34;
    match last_age(log, b"rx open ok", now) {
        Some(a) => {
            let d = fmt::duration(a, &mut vb);
            widget::row(&mut cv, y, b"last rx", &d[..d.len()], if a < 120 { color::OK } else { color::WARN });
        }
        None => widget::row(&mut cv, y, b"last rx", b"never seen", color::WARN),
    }
    y += 34;
    {
        let mut pb = [0u8; 16];
        let n = read_pid(PID_PATH, &mut pb);
        if n > 0 && pid_alive(&pb[..n]) {
            let mut val = [0u8; 40];
            let mut k = 0;
            for b in b"watch pid " {
                val[k] = *b;
                k += 1;
            }
            for &b in &pb[..n] {
                val[k] = b;
                k += 1;
            }
            widget::row(&mut cv, y, b"watchdog", &val[..k], color::OK);
        } else {
            widget::row(&mut cv, y, b"watchdog", b"NOT RUNNING", color::BAD);
        }
    }
    y += 34;
    let ft = access(FT_PATH.as_ptr(), 0) == 0;
    widget::row(&mut cv, y, b"fulltunnel", if ft { b"ON (file)" } else { b"off" }, if ft { color::WARN } else { color::DIM });

    let py = 356;
    cv.round_rect(16, py, 688, 280, 8, color::PANEL);
    cv.text(30, py + 26, b"wgd.log tail (4KB)", color::DIM);
    cv.hline(30, py + 36, 660, color::LINE);
    if !log.is_empty() {
        let (starts, m) = line_starts(log, 10);
        let mut lb = [0u8; 60];
        for k in 0..m {
            let s = starts[k];
            let e = if k + 1 < m { starts[k + 1].saturating_sub(1) } else { log.len() };
            let line = &log[s..core::cmp::min(e, log.len())];
            cv.text(30, py + 60 + 22 * k as i32, clean_line(&mut lb, line), color::TEXT);
        }
    }

    widget::button(&mut cv, 16, 656, 344, 44, b"REBIND + RE-HS", false);
    let ft_on: &[u8] = if ft { b"FULLTUN: ON" } else { b"FULLTUN: OFF" };
    widget::button(&mut cv, 376, 656, 328, 44, ft_on, ft);

    if TOAST_LEN > 0 && now.saturating_sub(TOAST_T) < 8 {
        cv.text(20, 640, &TOAST[..TOAST_LEN], color::OK);
    }
}

// --- entry ------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *mut *mut u8) -> i32 {
    unsafe {
        LFD = open(BOOT_LOG.as_ptr(), O_WRONLY | O_CREAT | O_TRUNC, 0o666);
        dlog(b"wgd-console boot\n");
        dlog_int(b"uid=", getuid() as i64);
        dlog_int(b"pid=", getpid() as i64);
        install_crash_handlers();

        let w = match sdl::init(W, H) {
            Ok(w) => {
                dlog(b"sdl init ok\n");
                w
            }
            Err(e) => {
                let tag: i64 = match e {
                    sdl::InitError::SdlInit { .. } => 1,
                    sdl::InitError::NoSurface => 2,
                    sdl::InitError::BadSurface { .. } => 3,
                };
                dlog_int(b"sdl init fail tag=", tag);
                let mut mb = [0u8; 200];
                let n = e.message(&mut mb);
                dlog(b"sdl err: ");
                dlog(&mb[..n]);
                dlog(b"\n");
                _exit(5);
            }
        };
        dlog_int(b"surface=", w.surf_ptr() as i64);

        loop {
            let now = time(core::ptr::null_mut());
            let mut click: Option<(i32, i32)> = None;
            sdl::pump_events();
            while let Some(ev) = sdl::poll_event() {
                match ev {
                    sdl::Event::Quit => {
                        dlog(b"sdl quit event\n");
                        sdl::quit();
                        _exit(0);
                    }
                    sdl::Event::Down { x, y } => click = Some((x, y)),
                    _ => {}
                }
            }
            read_tail(LOG_PATH);
            render(now, w.fb, w.stride);
            w.flip();
            if let Some((mx, my)) = click {
                if inside(mx, my, 16, 656, 344, 44) {
                    write_cmd(CMD_REBIND, b"rebind\n");
                    toast(b"sent: rebind (watchdog relaunch)", now);
                } else if inside(mx, my, 376, 656, 328, 44) {
                    if access(FT_PATH.as_ptr(), 0) == 0 {
                        write_cmd(CMD_FT, b"ftoff\n");
                        toast(b"fulltunnel OFF requested", now);
                    } else {
                        write_cmd(CMD_FT, b"fton\n");
                        toast(b"fulltunnel ON requested", now);
                    }
                }
            }
            usleep(250_000); // ~4 FPS, device has a battery to protect
        }
    }
}
