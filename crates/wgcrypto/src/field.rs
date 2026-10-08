//! Field elements mod 2^255-19: uniform radix 2^51, 5 x u64 limbs (donna-c64).
//! u128 accumulators: fine on host; QNX armv7 link check happens in stage B
//! (fallback: donna128 i64 port). Data-independent throughout.
#![allow(clippy::needless_range_loop)]

pub type Fe = [u64; 5];

const M: u64 = (1 << 51) - 1;

/// p limbs: (2^51-19, 2^51-1 x4)
const P: Fe = [M - 18, M, M, M, M];
/// 2p limbs (bias for sub)
const P2: Fe = [2 * (M - 18), 2 * M, 2 * M, 2 * M, 2 * M];

pub const ZERO: Fe = [0; 5];
pub const ONE: Fe = [1, 0, 0, 0, 0];

pub fn add(o: &mut Fe, f: &Fe, g: &Fe) {
    for i in 0..5 {
        o[i] = f[i] + g[i];
    }
}

pub fn sub(o: &mut Fe, f: &Fe, g: &Fe) {
    for i in 0..5 {
        o[i] = f[i] + P2[i] - g[i];
    }
}

#[inline]
fn mad(acc: u128, a: u64, b: u64) -> u128 {
    acc + (a as u128) * (b as u128)
}

#[inline]
fn mad19(acc: u128, a: u64, b: u64) -> u128 {
    acc + 19 * (a as u128) * (b as u128)
}

/// settle u128 accumulators into limbs (< ~2^51 + eps), folding limb4 excess *19
fn settle(acc: &mut [u128; 5], o: &mut Fe) {
    let mut c = acc[0] >> 51;
    o[0] = (acc[0] as u64) & M;
    acc[1] += c;
    c = acc[1] >> 51;
    o[1] = (acc[1] as u64) & M;
    acc[2] += c;
    c = acc[2] >> 51;
    o[2] = (acc[2] as u64) & M;
    acc[3] += c;
    c = acc[3] >> 51;
    o[3] = (acc[3] as u64) & M;
    acc[4] += c;
    c = acc[4] >> 51;
    o[4] = (acc[4] as u64) & M;
    o[0] += 19 * (c as u64);
    // one ripple for limb0 overflow
    c = (o[0] >> 51) as u128;
    o[0] &= M;
    o[1] += c as u64;
}

/// o = f * g mod p (inputs limbs < 2^52)
pub fn mul(o: &mut Fe, f: &Fe, g: &Fe) {
    let (f0, f1, f2, f3, f4) = (f[0], f[1], f[2], f[3], f[4]);
    let (g0, g1, g2, g3, g4) = (g[0], g[1], g[2], g[3], g[4]);
    let mut acc = [0u128; 5];
    acc[0] = mad(0, f0, g0);
    acc[0] = mad19(acc[0], f1, g4);
    acc[0] = mad19(acc[0], f2, g3);
    acc[0] = mad19(acc[0], f3, g2);
    acc[0] = mad19(acc[0], f4, g1);
    acc[1] = mad(0, f0, g1);
    acc[1] = mad(acc[1], f1, g0);
    acc[1] = mad19(acc[1], f2, g4);
    acc[1] = mad19(acc[1], f3, g3);
    acc[1] = mad19(acc[1], f4, g2);
    acc[2] = mad(0, f0, g2);
    acc[2] = mad(acc[2], f1, g1);
    acc[2] = mad(acc[2], f2, g0);
    acc[2] = mad19(acc[2], f3, g4);
    acc[2] = mad19(acc[2], f4, g3);
    acc[3] = mad(0, f0, g3);
    acc[3] = mad(acc[3], f1, g2);
    acc[3] = mad(acc[3], f2, g1);
    acc[3] = mad(acc[3], f3, g0);
    acc[3] = mad19(acc[3], f4, g4);
    acc[4] = mad(0, f0, g4);
    acc[4] = mad(acc[4], f1, g3);
    acc[4] = mad(acc[4], f2, g2);
    acc[4] = mad(acc[4], f3, g1);
    acc[4] = mad(acc[4], f4, g0);
    settle(&mut acc, o);
}

pub fn square(o: &mut Fe, f: &Fe) {
    let (f0, f1, f2, f3, f4) = (f[0], f[1], f[2], f[3], f[4]);
    let mut acc = [0u128; 5];
    // self-pairs (i==j) fold once: f0^2@0, f1^2@2, f2^2@4, 19*f3^2@1, 19*f4^2@3
    acc[0] = mad(0, f0, f0);
    acc[0] = {
        let t = mad19(0, f1, f4);
        let t = mad19(t, f2, f3);
        acc[0] + 2 * t
    };
    acc[1] = {
        let t = mad(0, f0, f1);
        let u = mad19(0, f2, f4);
        2 * t + 2 * u + mad19(0, f3, f3)
    };
    acc[2] = {
        let t = mad(0, f0, f2);
        let u = mad19(0, f3, f4);
        2 * t + 2 * u + mad(0, f1, f1)
    };
    acc[3] = {
        let t = mad(0, f0, f3);
        let t = mad(t, f1, f2);
        2 * t + mad19(0, f4, f4)
    };
    acc[4] = {
        let t = mad(0, f0, f4);
        let t = mad(t, f1, f3);
        2 * t + mad(0, f2, f2)
    };
    settle(&mut acc, o);
}

/// conditional swap (constant-time)
pub fn cswap(swap: u64, a: &mut Fe, b: &mut Fe) {
    let m: u64 = 0u64.wrapping_sub(swap & 1);
    for i in 0..5 {
        let d = (a[i] ^ b[i]) & m;
        a[i] ^= d;
        b[i] ^= d;
    }
}

pub fn from_bytes(f: &mut Fe, inb: &[u8; 32]) {
    let mut b = [0u8; 32];
    b.copy_from_slice(inb);
    b[31] &= 0x7f; // decodeUCoordinate: mask bit 255 only (RFC 7748 §5)
    // bit-serial unpack (called once per ladder run)
    for i in 0..5 {
        f[i] = 0;
    }
    let mut pos = 0usize;
    for byte in 0..32 {
        for bit in 0..8 {
            if (b[byte] >> bit) & 1 == 1 {
                let limb = pos / 51;
                let sh = pos % 51;
                f[limb] |= 1u64 << sh;
            }
            pos += 1;
        }
    }
}

/// f -> canonical [0,p) then LE 32 bytes
pub fn to_bytes(out: &mut [u8; 32], f: &Fe) {
    let mut t = *f;
    reduce(&mut t);
    for i in 0..32 {
        out[i] = 0;
    }
    let mut pos = 0usize;
    for limb in 0..5 {
        let mut v = t[limb];
        for _ in 0..51 {
            if v & 1 == 1 {
                out[pos / 8] |= 1 << (pos % 8);
            }
            v >>= 1;
            pos += 1;
        }
    }
}

/// reduce limbs toward canonical: fold limb4 overflow, compare-subtract p twice
pub fn reduce(o: &mut Fe) {
    for _ in 0..2 {
        let mut c = o[4] >> 51;
        o[4] &= M;
        o[0] += 19 * c;
        for i in 0..4 {
            c = o[i] >> 51;
            o[i] &= M;
            o[i + 1] += c;
        }
        c = o[4] >> 51;
        o[4] &= M;
        o[0] += 19 * c;
        // compare & subtract p if >= p
        let mut gt: u64 = 0;
        let mut lt: u64 = 0;
        let mut i = 5usize;
        while i > 0 {
            i -= 1;
            let g = if o[i] > P[i] { 1 } else { 0 };
            let l = if o[i] < P[i] { 1 } else { 0 };
            gt |= g & !lt;
            lt |= l & !gt;
        }
        if gt == 1 {
            let mut b: i128 = 0;
            for j in 0..5 {
                let d = o[j] as i128 - P[j] as i128 - b;
                o[j] = (d & (M as i128)) as u64;
                b = if d < 0 { 1 } else { 0 };
            }
        }
    }
}

/// o = f^(p-2) — square-and-multiply, data-independent
pub fn invert(o: &mut Fe, f: &Fe) {
    let mut e = [0u8; 32];
    e[0] = 0xeb;
    let mut i = 1;
    while i < 31 {
        e[i] = 0xff;
        i += 1;
    }
    e[31] = 0x7f;
    let mut r = ONE;
    let mut started = false;
    let mut bit = 255usize;
    loop {
        let bv = ((e[(bit - 1) / 8] >> ((bit - 1) % 8)) & 1) as u64;
        if started {
            let tmp = r;
            square(&mut r, &tmp);
        }
        if bv == 1 {
            if started {
                let tmp = r;
                mul(&mut r, &tmp, f);
            } else {
                r = *f;
                started = true;
            }
        }
        if bit == 1 {
            break;
        }
        bit -= 1;
    }
    *o = r;
    reduce(o);
}
