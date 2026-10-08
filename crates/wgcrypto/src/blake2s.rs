//! BLAKE2s (RFC 7693). 32-byte digest, optional key (for HKDF later).
#![allow(clippy::needless_range_loop)]

const IV: [u32; 8] = [
    0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a, 0x510e_527f, 0x9b05_688c, 0x1f83_d9ab,
    0x5be0_cd19,
];

const SIGMA: [[usize; 16]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

#[inline]
fn rotr32(v: u32, c: u32) -> u32 {
    (v >> c) | (v << (32 - c))
}

#[inline]
fn mix(v: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize, x: u32, y: u32) {
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
    v[d] = rotr32(v[d] ^ v[a], 16);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = rotr32(v[b] ^ v[c], 12);
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
    v[d] = rotr32(v[d] ^ v[a], 8);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = rotr32(v[b] ^ v[c], 7);
}

fn compress(h: &mut [u32; 8], block: &[u8; 64], t_lo: u64, t_hi: u64, last: bool) {
    let mut v = [0u32; 16];
    v[..8].copy_from_slice(h);
    v[8..].copy_from_slice(&IV);
    v[12] ^= t_lo as u32;
    v[13] ^= (t_lo >> 32) as u32;
    v[14] ^= t_hi as u32;
    v[15] ^= (t_hi >> 32) as u32;
    if last {
        v[14] ^= 0xffff_ffff;
    }
    let mut m = [0u32; 16];
    for i in 0..16 {
        m[i] = u32::from_le_bytes(block[4 * i..4 * i + 4].try_into().unwrap());
    }
    for r in 0..10 {
        let s = &SIGMA[r];
        mix(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
        mix(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
        mix(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
        mix(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);
        mix(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
        mix(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
        mix(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
        mix(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
    }
    for i in 0..8 {
        h[i] ^= v[i] ^ v[i + 8];
    }
}

/// unkeyed digest
/// streaming BLAKE2s state (BLAKE2 reference buffering semantics:
/// a full 64-byte buffer is compressed only once MORE data arrives, so
/// finalize() always emits the final block with the last-flag set)
pub struct State {
    h: [u32; 8],
    buf: [u8; 64],
    buflen: usize,
    t: u64, // bytes compressed so far (includes the key block)
    nn: u8, // digest length 1..=32 (parameter block nn)
}

impl State {
    fn init(key: &[u8], nn: u8) -> State {
        let mut h = IV;
        let mut p = [0u8; 64];
        p[0] = nn;
        p[1] = key.len() as u8;
        p[2] = 1;
        p[3] = 1;
        for i in 0..8 {
            h[i] ^= u32::from_le_bytes(p[4 * i..4 * i + 4].try_into().unwrap());
        }
        let mut st = State { h, buf: [0u8; 64], buflen: 0, t: 0, nn };
        if !key.is_empty() {
            let mut kb = [0u8; 64];
            kb[..key.len()].copy_from_slice(key);
            st.t = 64;
            compress(&mut st.h, &kb, 64, 0, false);
        }
        st
    }

    pub fn new_unkeyed() -> State {
        State::init(&[], 32)
    }

    /// keyed state (key len <= 32)
    pub fn new_keyed(key: &[u8]) -> State {
        State::init(key, 32)
    }

    /// keyed state with custom digest length 1..=32 (WG mac1: 16)
    pub fn new_keyed_nn(key: &[u8], nn: u8) -> State {
        State::init(key, nn)
    }

    pub fn update(&mut self, mut data: &[u8]) {
        while !data.is_empty() {
            if self.buflen == 64 {
                self.t += 64;
                compress(&mut self.h, &self.buf, self.t & 0xffffffff_ffffffff, 0, false);
                self.buflen = 0;
            }
            let n = core::cmp::min(64 - self.buflen, data.len());
            self.buf[self.buflen..self.buflen + n].copy_from_slice(&data[..n]);
            self.buflen += n;
            data = &data[n..];
        }
    }

    /// consume the state and write the 32-byte digest
    pub fn finalize(self, out: &mut [u8; 32]) {
        self.finalize_n(out)
    }

    /// write self.nn bytes (out longer than nn is zero-tail-safe if sized nn)
    pub fn finalize_n(mut self, out: &mut [u8]) {
        self.t += self.buflen as u64;
        for i in self.buflen..64 {
            self.buf[i] = 0;
        }
        compress(&mut self.h, &self.buf, self.t, 0, true);
        let mut full = [0u8; 32];
        full.copy_from_slice_by32(&self.h);
        out[..self.nn as usize].copy_from_slice(&full[..self.nn as usize]);
    }
}

pub fn hash(out: &mut [u8; 32], msg: &[u8]) {
    let mut s = State::new_unkeyed();
    s.update(msg);
    s.finalize(out);
}

/// keyed digest (key len <= 32)
pub fn hash_keyed(out: &mut [u8; 32], msg: &[u8], key: &[u8]) {
    let mut s = State::new_keyed(key);
    s.update(msg);
    s.finalize(out);
}

/// keyed BLAKE2s-128 (16-byte digest, domain-separated from 256)
pub fn hash_keyed16(out: &mut [u8; 16], msg: &[u8], key: &[u8]) {
    let mut s = State::new_keyed_nn(key, 16);
    s.update(msg);
    s.finalize_n(out);
}

/// keyed digest, arbitrary length out.len() in 1..=32 (WG mac1 uses 16)
pub fn hash_keyed_n(out: &mut [u8], msg: &[u8], key: &[u8]) {
    let mut s = State::new_keyed_nn(key, out.len() as u8);
    s.update(msg);
    s.finalize_n(out);
}

trait CopyFromH {
    fn copy_from_slice_by32(&mut self, h: &[u32; 8]);
}
impl CopyFromH for [u8; 32] {
    fn copy_from_slice_by32(&mut self, h: &[u32; 8]) {
        for i in 0..8 {
            self[4 * i..4 * i + 4].copy_from_slice(&h[i].to_le_bytes());
        }
    }
}
