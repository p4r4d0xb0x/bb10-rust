// wgd.rs — userspace WireGuard client daemon for BB10 (EXP-RUST-010 Stage C).
// Single-threaded select() loop: tun0 (raw IP, verified) + UDP socket.
// Conf file drives identity/peer; event-driven timers (retransmit 5s,
// re-handshake 120s, keepalive 30s). v4 self-daemonize (fork+_exit).
#![no_std]
#![no_main]
#![allow(non_snake_case, non_upper_case_globals)]

extern crate wgcrypto;
use wgcrypto::noise;
use wgcrypto::noise::Key;

core::arch::global_asm!("mov pc, r0");

extern "C" {
    fn open(p: *const u8, o: i32, ...) -> i32;
    fn read(fd: i32, b: *mut u8, n: usize) -> isize;
    fn write(fd: i32, b: *const u8, n: usize) -> isize;
    fn close(fd: i32) -> i32;
    fn fcntl(fd: i32, c: i32, ...) -> i32;
    fn ioctl(fd: i32, c: u32, a: *const i32) -> i32;
    fn socket(d: i32, t: i32, p: i32) -> i32;
    fn bind(fd: i32, a: *const u8, l: i32) -> i32;
    fn sendto(fd: i32, b: *const u8, n: usize, f: i32, sa: *const u8, l: i32) -> isize;
    fn recvfrom(fd: i32, b: *mut u8, n: usize, f: i32, sa: *mut u8, sl: *mut u32) -> isize;
    fn select(n: i32, r: *mut u32, w: *mut u32, e: *mut u32, tv: *mut u8) -> i32;
    fn time(t: *mut u64) -> u64;
    fn fork() -> i32;
    fn setsid() -> i32;
    fn _exit(c: i32) -> !;
    fn strlen(s: *const u8) -> usize;
    fn snprintf(b: *mut u8, n: usize, f: *const u8, ...) -> i32;
    fn __get_errno_ptr() -> *mut i32;
}

const AF_INET: i32 = 2;
const SOCK_DGRAM: i32 = 2; // QNX: STREAM=1, DGRAM=2 (udpprobe-verified)
const O_RDWR: i32 = 2;
const O_CREAT: i32 = 0o400;
const O_NONBLOCK: i32 = 0o200;
const F_SETFL: i32 = 4;
const TUNSIFHEAD: u32 = 0x8004_7442; // _IOW('t',66,int)

static mut LFD: i32 = -1;
static mut LB: [u8; 512] = [0; 512];

unsafe fn log(s: &[u8]) {
    if LFD < 0 {
        LFD = open(b"/accounts/devuser/wgd.log\0".as_ptr(), 0o1 | O_CREAT | O_NONBLOCK, 0o644);
    }
    if LFD >= 0 {
        write(LFD, s.as_ptr(), s.len());
    }
}
unsafe fn logn(tag: &[u8], v: i64) {
    let mut off = 0usize;
    while off < tag.len() {
        LB[off] = tag[off];
        off += 1;
    }
    if v < 0 {
        LB[off] = b'-';
        off += 1;
    }
    let mut x = if v < 0 { -(v as i64) } else { v };
    if x == 0 {
        LB[off] = b'0';
        off += 1;
    } else {
        let mut t = [0u8; 24];
        let mut n = 0usize;
        while x > 0 {
            t[n] = b'0' + (x % 10) as u8;
            x /= 10;
            n += 1;
        }
        while n > 0 {
            n -= 1;
            LB[off] = t[n];
            off += 1;
        }
    }
    LB[off] = b'\n';
    log(&LB[..off + 1]);
}
unsafe fn log_hex(tag: &[u8], b: &[u8]) {
    let mut off = 0usize;
    while off < tag.len() {
        LB[off] = tag[off];
        off += 1;
    }
    for x in b {
        LB[off] = b"0123456789abcdef"[(x >> 4) as usize];
        LB[off + 1] = b"0123456789abcdef"[(x & 15) as usize];
        off += 2;
    }
    LB[off] = b'\n';
    log(&LB[..off + 1]);
}
unsafe fn err() -> i32 {
    *__get_errno_ptr()
}

// ---- mini conf ----
static mut CONF: [u8; 1024] = [0; 1024];
#[derive(Clone, Copy)]
struct Allowed {
    ip: [u8; 4],
    bits: u8,
}
static mut PRIV: Key = [0; 32];
static mut PEER: Key = [0; 32];
static mut PSK: Key = [0; 32];
static mut EP: [u8; 4] = [0; 4];
static mut EP_PORT: u16 = 51820;
static mut ALLOW: [Allowed; 8] = [Allowed { ip: [0; 4], bits: 0 }; 8];
static mut NALLOW: usize = 0;

unsafe fn hexval(c: u8) -> i8 {
    match c {
        b'0'..=b'9' => (c - b'0') as i8,
        b'a'..=b'f' => (c - b'a' + 10) as i8,
        b'A'..=b'F' => (c - b'A' + 10) as i8,
        _ => -1,
    }
}
/// decode 64 hex chars at s[i..] into out32; returns idx after, or -1
unsafe fn get_hex64(s: &[u8], mut i: usize, out: &mut Key) -> i32 {
    let mut j = i;
    while j < s.len() && (s[j] == b' ' || s[j] == b'\t') {
        j += 1;
    }
    if j + 64 > s.len() {
        return -1;
    }
    for k in 0..32 {
        let hi = hexval(s[j + 2 * k]);
        let lo = hexval(s[j + 2 * k + 1]);
        if hi < 0 || lo < 0 {
            return -1;
        }
        out[k] = ((hi << 4) | lo) as u8;
    }
    (j + 64) as i32
}
unsafe fn get_quad(s: &[u8], mut i: usize, out: &mut [u8; 4]) -> i32 {
    let mut part = 0u32;
    let mut k = 0usize;
    let mut digits = 0usize;
    while i < s.len() && k < 4 {
        let c = s[i];
        if c >= b'0' && c <= b'9' {
            part = part * 10 + (c - b'0') as u32;
            digits += 1;
            if part > 255 {
                return -1;
            }
            i += 1;
        } else if c == b'.' {
            if digits == 0 {
                return -1;
            }
            out[k] = part as u8;
            k += 1;
            part = 0;
            digits = 0;
            i += 1;
        } else {
            break;
        }
    }
    if k == 3 && digits > 0 {
        out[3] = part as u8;
        i as i32
    } else {
        -1
    }
}

unsafe fn load_conf() -> bool {
    let fd = open(b"/accounts/devuser/wgd.conf\0".as_ptr(), 0, 0);
    if fd < 0 {
        log(b"conf open FAIL\n");
        return false;
    }
    let n = read(fd, CONF.as_mut_ptr(), 1024);
    close(fd);
    if n <= 0 {
        return false;
    }
    let s = &CONF[..n as usize];
    let mut i = 0usize;
    while i < s.len() {
        let start = i;
        while i < s.len() && s[i] != b'\n' {
            i += 1;
        }
        let line = &s[start..i];
        i += 1;
        if line.len() > 1 && line[0] == b'p' {
            // private / preshared / peer_pub  (all share the 'p' prefix!)
            if let Some(x) = strip_key(line, b"private") {
                let r = get_hex64(line, x, &mut PRIV);
                if r < 0 {
                    log(b"conf: bad private\n");
                    return false;
                }
            } else if let Some(x) = strip_key(line, b"preshared") {
                let r = get_hex64(line, x, &mut PSK);
                if r < 0 {
                    return false;
                }
            } else if let Some(x) = strip_key(line, b"peer_pub") {
                let r = get_hex64(line, x, &mut PEER);
                if r < 0 {
                    return false;
                }
            }
        } else if line.len() > 1 && line[0] == b'e' {
            if let Some(x) = strip_key(line, b"endpoint_ip") {
                let mut q = [0u8; 4];
                let r = get_quad(line, x, &mut q);
                if r < 0 {
                    return false;
                }
                EP = q;
            } else if let Some(x) = strip_key(line, b"endpoint_port") {
                let mut p = 0u32;
                let mut j = x;
                while j < line.len() && line[j] >= b'0' && line[j] <= b'9' {
                    p = p * 10 + (line[j] - b'0') as u32;
                    j += 1;
                }
                EP_PORT = p as u16;
            }
        } else if line.len() > 1 && line[0] == b'a' {
            if let Some(x) = strip_key(line, b"allow") {
                if NALLOW < 8 {
                    let mut q = [0u8; 4];
                    let r = get_quad(line, x, &mut q);
                    if r > 0 {
                        let mut bits = 32u8;
                        let mut j = r as usize;
                        if j < line.len() && line[j] == b'/' {
                            j += 1;
                            bits = 0;
                            while j < line.len() && line[j] >= b'0' && line[j] <= b'9' {
                                bits = bits * 10 + (line[j] - b'0');
                                j += 1;
                            }
                        }
                        ALLOW[NALLOW] = Allowed { ip: q, bits };
                        NALLOW += 1;
                    }
                }
            }
        }
    }
    // sanity: private and peer set?
    let mut z = true;
    for b in PRIV.iter() {
        if *b != 0 {
            z = false;
        }
    }
    if z {
        log(b"conf: missing private\n");
        return false;
    }
    true
}
/// line == "key=..." prefix; returns index after '='
unsafe fn strip_key<'a>(line: &'a [u8], key: &[u8]) -> Option<usize> {
    if line.len() <= key.len() + 1 || line[key.len()] != b'=' {
        return None;
    }
    for i in 0..key.len() {
        if line[i] != key[i] {
            return None;
        }
    }
    let mut j = key.len() + 1;
    while j < line.len() && (line[j] == b' ' || line[j] == b'\t') {
        j += 1;
    }
    Some(j)
}
fn allowed_ip(dst: &[u8; 4]) -> bool {
    unsafe {
        for k in 0..NALLOW {
            let a = &ALLOW[k];
            if a.bits > 32 {
                continue;
            }
            let mut ok = true;
            for b in 0..4 {
                let bb = b as u16;
                let bits = a.bits as u16;
                let keep = if bits >= (bb + 1) * 8 {
                    8u16
                } else if bits <= bb * 8 {
                    0u16
                } else {
                    bits - bb * 8
                };
                let m: u8 = if keep == 0 {
                    0
                } else {
                    (0xFFu16 << (8 - keep)) as u8
                };
                if dst[b] & m != a.ip[b] & m {
                    ok = false;
                    break;
                }
            }
            if ok {
                return true;
            }
        }
        false
    }
}

// ---- handshake/transport state ----
static mut UDP: i32 = -1;
static mut TUN: i32 = -1;
static mut SA_EP: [u8; 16] = [0; 16];

struct HsPending {
    active: bool,
    state: noise::SymState,
    e_sk: Key,
    sender: u32,
    msg: [u8; noise::MSG_INITIATION_LEN],
    t_send: u64,
}
static mut PENDING: HsPending = HsPending {
    active: false,
    state: unsafe { core::mem::transmute_copy(&[0u8; 128]) },
    e_sk: [0; 32],
    sender: 0,
    msg: [0; noise::MSG_INITIATION_LEN],
    t_send: 0,
};
struct SrvResp { active: bool, their_idx: u32, msg: [u8; noise::MSG_RESPONSE_LEN], }
static mut SRV_RESP: SrvResp = SrvResp { active: false, their_idx: 0, msg: [0; noise::MSG_RESPONSE_LEN] };
static mut S_KEYS: Option<noise::SymKeys> = None;
static mut RX_IDX: u32 = 0;
static mut OUR_IDX: u32 = 0;
static mut TX_CTR: u64 = 0;
static mut RX_MAX: u64 = 0;
static mut T_HS: u64 = 0;
static mut T_KEEP: u64 = 0;
static mut T_RX: u64 = 0;
static mut T_BOOT: u64 = 0;
static mut TRIGGER_SENT: bool = false;
static mut T_FIRSTPEND: u64 = 0;      // v5: when current PENDING first went active
static mut SEND_FAILS: i32 = 0;        // v5: consecutive UDP send failures -> rebind
static mut T_REBIND: u64 = 0;          // v5: last socket rebind time
static mut T_BEAT: u64 = 0;            // v5: heartbeat 60s
static mut T_LASTOK: u64 = 0;          // v5: last successful tx/rx timestamp (liveness clock)
static mut T_SELLAST: u64 = 0;         // v5: last select-error log time (storm throttle)
static mut OUR_PUB: Key = [0; 32];

unsafe fn initiate(now: u64) {
    let mut esk = [0u8; 32];
    let f = open(b"/dev/urandom\0".as_ptr(), 0, 0);
    if f >= 0 {
        read(f, esk.as_mut_ptr(), 32);
        close(f);
    }
    let sss = noise::shared_secret(&PRIV, &PEER);
    let sender = {
        let mut r = [0u8; 4];
        let f = open(b"/dev/urandom\0".as_ptr(), 0, 0);
        if f >= 0 {
            read(f, r.as_mut_ptr(), 4);
            close(f);
        }
        u32::from_le_bytes(r)
    };
    let out = noise::create_initiation(
        &noise::initial_state(),
        &PRIV,
        &PEER,
        &sss,
        &esk,
        sender,
        now,
        0,
    );
    let mut m = out.msg;
    let mut mk = [0u8; 32];
    noise::mac1_key(&mut mk, &PEER); // receiver = server
    noise::fill_mac1_initiation(&mut m, &mk);
    let r = sendto(UDP, m.as_ptr(), m.len(), 0, SA_EP.as_ptr(), 16);
    if r < 0 {
        logn(b"init sendto err=", err() as i64);
    }
    let was_pending = PENDING.active; // v5
    PENDING.active = true;
    if T_FIRSTPEND == 0 || !was_pending {
        T_FIRSTPEND = now; // v5: clock starts at FIRST attempt, not restarts
    }
    OUR_IDX = sender;
    PENDING.state = out.state;
    PENDING.e_sk = esk;
    PENDING.sender = sender;
    PENDING.msg = m;
    PENDING.t_send = now;
    logn(b"initiate sender=", sender as i64);
}

// v5: recreate the UDP socket after repeated sendto failures. A Wi-Fi/LTE
// interface bounce kills the bound socket (sendto r=-1 forever; observed
// 2026-10-06: tunnel UP but keepalive dead 45min, watchdog fooled by log growth).
// New socket = new source port = fresh NAT path; session must re-handshake.
unsafe fn rebind_socket() -> bool {
    if UDP >= 0 {
        close(UDP);
    }
    UDP = socket(AF_INET, SOCK_DGRAM, 0);
    if UDP < 0 {
        log(b"rebind: socket FAIL\n");
        return false;
    }
    let mut sa_any = [0u8; 16];
    sa_any[0] = 16;
    sa_any[1] = AF_INET as u8;
    bind(UDP, sa_any.as_ptr(), 16);
    SEND_FAILS = 0;
    T_REBIND = time(core::ptr::null_mut());
    log(b"rebind: socket rebound\n");
    true
}

// v5: session liveness. On BB10, a dead path yields EBADMSG(77) on select()
// for the tun fd (observed 2026-10-06: select err=77 storm + keepalive r=-1)
// but sendto itself may keep "succeeding" into a black hole — so recovery is
// keyed on INBOUND/OUTBOUND success age, not send error counts.
unsafe fn note_send_fail(now: u64) {
    SEND_FAILS += 1;
    let _ = now;
}

// called once per loop tick: if nothing has gone OUT or come IN for a while,
// the socket/iface pair is stale -> rebind socket + force re-handshake.
unsafe fn liveness(now: u64) {
    let last = if T_LASTOK > T_HS { T_LASTOK } else { T_HS };
    if now - last >= 120 && now - T_REBIND >= 120 {
        if S_KEYS.is_some() || PENDING.active {
            log(b"liveness: no I/O 120s -> rebind+rehandshake\n");
            if rebind_socket() {
                S_KEYS = None;
                PENDING.active = false;
                T_FIRSTPEND = 0;
                TRIGGER_SENT = false;
                initiate(now);
            }
        }
    }
}

unsafe fn send_keepalive(now: u64) {
    if let Some(k) = &*(&S_KEYS as *const Option<noise::SymKeys>) {
        let mut buf = [0u8; 64];
        if let Some(n) = noise::seal_transport(&k.send, TX_CTR, RX_IDX, &[], &mut buf) {
            let r = sendto(UDP, buf.as_ptr(), n, 0, SA_EP.as_ptr(), 16);
            logn(b"keepalive sent r=", r as i64);
            if r >= 0 {
                TX_CTR += 1;
                SEND_FAILS = 0;
                T_LASTOK = now;
            } else {
                logn(b"ka errno=", err() as i64); // v5 debug: 65=ENETUNREACH 51=EHOSTUNREACH 249=EADDRNOTAVAIL
                note_send_fail(now);
            }
        }
    }
    T_KEEP = now;
}

unsafe fn handle_udp(now: u64) {
    let mut buf = [0u8; 2048];
    let mut from = [0u8; 16];
    let mut flen: u32 = 16;
    let n = recvfrom(UDP, buf.as_mut_ptr(), 2048, 0, from.as_mut_ptr(), &mut flen);
    if n < 0 {
        logn(b"udp recv err=", err() as i64);
        return;
    }
    if n < 4 {
        logn(b"udp short n=", n as i64);
        return;
    }
    let typ = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    logn(b"udp in typ=", typ as i64);
    logn(b"udp in len=", n as i64);
    if typ == 2 && n as usize == noise::MSG_RESPONSE_LEN {
        if !PENDING.active {
            log(b"resp but no pending\n");
            return;
        }
        let rcv = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
        if rcv != PENDING.sender {
            return;
        }
        let mut mk = [0u8; 32];
        noise::mac1_key(&mut mk, &OUR_PUB); // receiver = us
        if !noise::check_mac1_response(buf[..noise::MSG_RESPONSE_LEN].try_into().unwrap(), &mk) {
            log(b"resp mac1 FAIL\n");
            return;
        }
        let mut rm = [0u8; noise::MSG_RESPONSE_LEN];
        rm.copy_from_slice(&buf[..noise::MSG_RESPONSE_LEN]);
        if let Some(st) = noise::consume_response(&PENDING.state, &PRIV, &PENDING.e_sk, &rm, &PSK) {
            let keys = noise::begin_symmetric(&st, true);
            RX_IDX = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
            S_KEYS = Some(keys);
            // Our TX_CTR persists across handshakes (WG invariant).
            // Server restarts its sending counter at 0 per session.
            RX_MAX = 0;
            PENDING.active = false;
            T_FIRSTPEND = 0; // v5: pending-cycle over
            T_HS = now;
            T_KEEP = now;
            T_RX = now;
            TRIGGER_SENT = false;
            T_LASTOK = now;
            log(b"HANDSHAKE-OK\n");
            log_hex(b"keys.send=", &keys.send);
            log_hex(b"keys.recv=", &keys.recv);
        } else {
            log(b"resp consume FAIL\n");
        }
    } else if typ == 4 && S_KEYS.is_some() {
        let rcv = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        if rcv != OUR_IDX {
            logn(b"t4 wrong recv-index=", rcv as i64);
            return;
        }
        let keys = (*(&S_KEYS as *const Option<noise::SymKeys>)).as_ref().unwrap();
        let mut out = [0u8; 2048];
        if let Some((ctr, len)) = noise::open_transport(&keys.recv, &buf[..n as usize], &mut out) {
            // WG replay: counters start at 0; RX_MAX tracks next-expected (exclusive)
            if ctr < RX_MAX {
                log(b"replay drop\n");
                return;
            }
            RX_MAX = ctr + 1;
            TRIGGER_SENT = false; // session confirmed live
            T_RX = now;
            // kernel expects AF4 header on write (TUNSIFHEAD=1)
            out.copy_within(0..len, 4);
            out[0] = 0;
            out[1] = 0;
            out[2] = 0;
            out[3] = 2;
            T_LASTOK = now;
            logn(b"rx open ok len=", len as i64);
            log_hex(b"rx head=", &out[4..(4 + if len < 24 { len } else { 24 })]);
            let wr = {
                // QNX io-pkt may require write len == IP total len (+AF4 hdr);
                // WG pads transport plaintext to block size — truncate the pad.
                let mut wl = len;
                if len >= 20 {
                    let tot = ((out[2] as usize) << 8) | out[3] as usize;
                    if tot >= 20 && tot < len {
                        wl = tot;
                    }
                }
                write(TUN, out.as_ptr(), wl + 4)
            };
            logn(b"tun write r=", wr as i64);
        } else {
            log(b"t4 open FAIL\n");
        }
    } else if typ == 1 && n as usize == noise::MSG_INITIATION_LEN {
        // Server-initiated handshake -> respond; its session REPLACES ours
        // (WG single-keypair-per-peer model; TX_CTR survives).
        let mut im = [0u8; noise::MSG_INITIATION_LEN];
        im.copy_from_slice(&buf[..noise::MSG_INITIATION_LEN]);
        let mut mk = [0u8; 32];
        noise::mac1_key(&mut mk, &OUR_PUB); // receiver = us
        if !noise::check_mac1_initiation(&im, &mk) {
            log(b"srv init mac1 FAIL\n");
            return;
        }
        let their_sender = u32::from_le_bytes([im[4], im[5], im[6], im[7]]);
        if SRV_RESP.active && SRV_RESP.their_idx == their_sender {
            // retransmission of the SAME initiation: answer identically
            sendto(UDP, SRV_RESP.msg.as_ptr(), SRV_RESP.msg.len(), 0, from.as_mut_ptr(), flen as i32);
            return;
        }
        SRV_RESP.active = false;
        if let Some(mut ini) = noise::consume_initiation(&PRIV, &im) {
            let sss = noise::shared_secret(&PRIV, &ini.peer_static);
            if !noise::consume_initiation_verify_sss(&mut ini, &im, &sss) {
                log(b"srv init sss FAIL\n");
                return;
            }
            let e_i_pk: Key = im[8..40].try_into().unwrap();
            let mut esk = [0u8; 32];
            let f = open(b"/dev/urandom\0".as_ptr(), 0, 0);
            if f >= 0 { read(f, esk.as_mut_ptr(), 32); close(f); }
            let mut r4 = [0u8; 4];
            let f = open(b"/dev/urandom\0".as_ptr(), 0, 0);
            if f >= 0 { read(f, r4.as_mut_ptr(), 4); close(f); }
            let my_idx = u32::from_le_bytes(r4);
            let out = noise::create_response(
                &ini, &e_i_pk, &ini.peer_static, &esk, &PSK, &PRIV, my_idx, their_sender,
            );
            let mut rm = out.msg;
            noise::mac1_key(&mut mk, &ini.peer_static); // receiver = them
            noise::fill_mac1_response(&mut rm, &mk);
            sendto(UDP, rm.as_ptr(), rm.len(), 0, from.as_mut_ptr(), flen as i32);
            S_KEYS = Some(noise::begin_symmetric(&out.state, false));
            RX_MAX = 0;
            OUR_IDX = my_idx;             // inbound receiver field must equal this
            RX_IDX = their_sender;        // our tx receiver field
            PENDING.active = false;       // our handshake attempt is moot now
            SRV_RESP.active = true;
            SRV_RESP.their_idx = their_sender;
            SRV_RESP.msg = rm;
            T_HS = now;
            T_KEEP = now;
            T_RX = now;
            TRIGGER_SENT = false;
            log(b"srv-handshake RESPONDED\n");
        } else {
            log(b"srv init consume FAIL\n");
        }
    } else if typ == 3 {
        log(b"cookie-reply (ignored)\n");
    }
}

unsafe fn handle_tun(now: u64) {
    let mut pkt = [0u8; 2048];
    let n = read(TUN, pkt.as_mut_ptr(), 2048);
    if n < 0 {
        return; // EWOULDBLOCK — nothing pending
    }
    logn(b"tun read n=", n as i64);
    log_hex(b"tun head=", &pkt[..if n < 8 { n as usize } else { 8 }]);
    // TUNSIFHEAD: [0,0,0,2] AF4 prefix, then raw IPv4
    if n < 32 || pkt[0] != 0 || pkt[1] != 0 || pkt[2] != 0 || pkt[3] != 2 || pkt[4] >> 4 != 4 {
        log(b"tun hdr reject\n");
        return;
    }
    pkt.copy_within(4..n as usize, 0);
    let n = n - 4;
    let dst = [pkt[16], pkt[17], pkt[18], pkt[19]];
    if !allowed_ip(&dst) {
        logn(b"dst not allowed ", dst[0] as i64 + (dst[1] as i64 * 256) + (dst[2] as i64 * 65536) + (dst[3] as i64 * 16777216));
        return;
    }
    let keys = match &*(&S_KEYS as *const Option<noise::SymKeys>) {
        Some(k) => k,
        None => {
            if !PENDING.active && now - T_HS >= 0 {
                initiate(now); // start handshake on demand
            }
            return;
        }
    };
    let mut buf = [0u8; 2200];
    if let Some(tot) = noise::seal_transport(&keys.send, TX_CTR, RX_IDX, &pkt[..n as usize], &mut buf) {
        if sendto(UDP, buf.as_ptr(), tot, 0, SA_EP.as_ptr(), 16) >= 0 {
            TX_CTR += 1;
            T_LASTOK = now;
            logn(b"tx sealed len=", tot as i64);
            SEND_FAILS = 0;
        } else {
            logn(b"tx sendto err=", err() as i64);
            note_send_fail(now);
        }
    }
}

unsafe fn fd_set_bit(fds: &mut [u32; 16], fd: i32) {
    fds[(fd / 32) as usize] |= 1 << (fd % 32);
}

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *mut *mut u8) -> i32 {
    unsafe {
        // self-daemonize (v4 pattern)
        let p = fork();
        if p > 0 {
            _exit(0);
        }
        setsid();

        log(b"wgd start\n");
        if !load_conf() {
            log(b"conf FAIL, exit\n");
            _exit(1);
        }
        OUR_PUB = noise::pub_of(&PRIV);
        log_hex(b"our-pub=", &OUR_PUB);
        logn(b"peer-port=", EP_PORT as i64);

        // endpoint sockaddr_in (QNX legacy layout: len@0, family@1, port BE@2, ip@4)
        SA_EP[0] = 16;
        SA_EP[1] = AF_INET as u8;
        SA_EP[2] = (EP_PORT >> 8) as u8;
        SA_EP[3] = (EP_PORT & 0xff) as u8;
        SA_EP[4..8].copy_from_slice(&EP);

        UDP = socket(AF_INET, SOCK_DGRAM, 0);
        if UDP < 0 {
            log(b"udp socket FAIL\n");
            _exit(2);
        }
        let mut sa_any = [0u8; 16];
        sa_any[0] = 16;
        sa_any[1] = AF_INET as u8;
        bind(UDP, sa_any.as_ptr(), 16);

        TUN = open(b"/dev/tun0\0".as_ptr(), O_RDWR, 0);
        if TUN < 0 {
            log(b"tun0 open FAIL (root chmod needed?)\n");
            _exit(3);
        }
        fcntl(TUN, F_SETFL, O_NONBLOCK); // probe-verified: F_SETFL works, open-flag does not
        let one: i32 = 1;
        let ir = ioctl(TUN, TUNSIFHEAD, &one); // AF4 framing both ways (tunprobe-proven)
        logn(b"sifhead1 rc=", ir as i64);

        T_BOOT = time(core::ptr::null_mut());
        T_HS = T_BOOT;
        T_KEEP = T_BOOT;
        log(b"loop start\n");

        // start handshake right away
        initiate(T_BOOT);

        // QNX io-pkt select() never reports tun0 readable (probe-verified):
        // short UDP select tick + unconditional non-blocking tun drain.
        let mut tv = [0u8; 8];
        tv[0] = 1; // 1 second tick -> 200ms below
        loop {
            let mut fds = [0u32; 16];
            fd_set_bit(&mut fds, UDP);
            fd_set_bit(&mut fds, TUN);
            let r = select(
                if UDP > TUN { UDP + 1 } else { TUN + 1 },
                fds.as_mut_ptr(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                tv.as_mut_ptr(),
            );
            let now = time(core::ptr::null_mut());
            if r < 0 {
                let e = err();
                // v5: suppress err-storm (EBADMSG 77 when tun/udp pair goes stale),
                // log first occurrence + every 30s only
                if e != 77 || now.wrapping_sub(T_SELLAST) >= 30 {
                    logn(b"select err=", e as i64);
                    T_SELLAST = now;
                }
            }
            if r > 0 && (fds[(UDP / 32) as usize] >> (UDP % 32)) & 1 == 1 {
                handle_udp(now);
            }
            {
                // drain tun0 unconditionally (select mask unreliable for tun)
                let mut guard = 0;
                while guard < 16 {
                    let before = TX_CTR;
                    handle_tun(now);
                    guard += 1;
                    if TX_CTR == before {
                        break;
                    }
                }
            }
            // timers
            if PENDING.active {
                if now - PENDING.t_send >= 5 {
                    log(b"retransmit\n");
                    let sr = sendto(UDP, PENDING.msg.as_ptr(), PENDING.msg.len(), 0, SA_EP.as_ptr(), 16);
                    if sr >= 0 {
                        SEND_FAILS = 0;
                    } else {
                        logn(b"rtx errno=", err() as i64);
                        note_send_fail(now);
                    }
                    PENDING.t_send = now;
                }
                // v5: a pending handshake older than 60s is dead (e.g. socket died
                // mid-handshake, TRIGGER_SENT latch). Restart fresh, forever.
                if now - T_FIRSTPEND >= 60 {
                    log(b"stale-pending restart\n");
                    PENDING.active = false;
                    initiate(now);
                }
            }
            // WG MacTimer: if nothing decrypted for 2 min, trigger (v5: re-arms
            // every 5 min if session stays dead, no permanent latch).
            if S_KEYS.is_some() && now - T_RX >= 120 {
                if !TRIGGER_SENT {
                    log(b"mac-rehandshake\n");
                    initiate(now);
                    TRIGGER_SENT = true;
                } else if now - T_HS >= 300 {
                    TRIGGER_SENT = false;
                }
            }
            if S_KEYS.is_some() && now - T_KEEP >= 30 {
                send_keepalive(now);
            }
            liveness(now);
            // v5 heartbeat: liveness + state to wgd.log (watchdog froze-gate ally).
            if now - T_BEAT >= 60 {
                T_BEAT = now;
                logn(b"beat up=", (now.wrapping_sub(T_BOOT)) as i64);
                logn(b"beat keys=", if S_KEYS.is_some() { 1 } else { 0 });
            }
        }
    }
}

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
