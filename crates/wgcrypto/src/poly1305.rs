//! Poly1305 (RFC 8439 §2.5): accumulator in base-2^26 limbs, u64-only (no u128).
#![allow(clippy::needless_range_loop)]

const B: u64 = 1 << 26;
const M: u64 = B - 1;

/// p = 2^130-5 = (B-5) + (B-1)*B + ... + (B-1)*B^4
const P: [u64; 5] = [B - 5, M, M, M, M];

/// LE bytes (n bytes) + stop-bit 0x01 at position n -> 5 limbs
fn block_to_limbs(src: &[u8], out: &mut [u64; 5]) {
    *out = [0; 5];
    let mut acc: u64 = 0;
    let mut nb: u32 = 0;
    let mut idx = 0usize;
    for i in 0..=src.len() {
        let byte = if i < src.len() { src[i] } else { 1u8 };
        acc |= (byte as u64) << nb;
        nb += 8;
        if nb >= 26 && idx < 5 {
            out[idx] = acc & M;
            acc >>= 26;
            nb -= 26;
            idx += 1;
        }
    }
    if idx < 5 {
        out[idx] = acc;
    }
}

/// raw 16 LE bytes -> 5 limbs (no stop bit)
fn bytes16_to_limbs(src: &[u8], out: &mut [u64; 5]) {
    *out = [0; 5];
    let mut acc: u64 = 0;
    let mut nb: u32 = 0;
    let mut idx = 0usize;
    for i in 0..16 {
        acc |= (src[i] as u64) << nb;
        nb += 8;
        if nb >= 26 {
            out[idx] = acc & M;
            acc >>= 26;
            nb -= 26;
            idx += 1;
        }
    }
    out[idx] = acc;
}

fn fold_carry(h: &mut [u64; 5]) {
    for _ in 0..2 {
        let c = h[4] >> 26;
        h[4] &= M;
        h[0] += c * 5;
        for i in 0..4 {
            let c = h[i] >> 26;
            h[i] &= M;
            h[i + 1] += c;
        }
    }
}

fn ge_p(h: &[u64; 5]) -> bool {
    let mut i = 5usize;
    while i > 0 {
        i -= 1;
        if h[i] > P[i] {
            return true;
        }
        if h[i] < P[i] {
            return false;
        }
    }
    true // equal counts as >=
}

fn sub_p(h: &mut [u64; 5]) {
    let mut borrow: i64 = 0;
    for i in 0..5 {
        let d = h[i] as i64 - P[i] as i64 + borrow;
        h[i] = (d & (M as i64)) as u64;
        borrow = d >> 26;
    }
    let _ = borrow;
}

/// h (canonical < p) + s, serialize low 128 bits LE
fn add_s_and_serialize(out: &mut [u8; 16], h: &[u64; 5], s: &[u64; 5]) {
    let mut t = [0u64; 6];
    for i in 0..5 {
        t[i] = h[i] + s[i];
    }
    for i in 0..5 {
        let c = t[i] >> 26;
        t[i] &= M;
        t[i + 1] += c;
    }
    // low 64 bits = t0 | t1<<26 | (t2 & 0xfff)<<52 ; high = t2>>12 | t3<<14 | (t4&0xffffff)<<40
    let lo = t[0] | (t[1] << 26) | ((t[2] & 0xfff) << 52);
    let hi = (t[2] >> 12) | (t[3] << 14) | ((t[4] & 0xff_ffff) << 40);
    for i in 0..8 {
        out[i] = (lo >> (8 * i)) as u8;
    }
    for i in 0..8 {
        out[8 + i] = (hi >> (8 * i)) as u8;
    }
}

/// key: 32 bytes; out: 16-byte tag over msg
pub fn tag(out: &mut [u8; 16], key: &[u8; 32], msg: &[u8]) {
    let mut rb = [0u8; 16];
    rb.copy_from_slice(&key[..16]);
    rb[3] &= 15;
    rb[7] &= 15;
    rb[11] &= 15;
    rb[15] &= 15;
    rb[4] &= 252;
    rb[8] &= 252;
    rb[12] &= 252;
    let mut r = [0u64; 5];
    bytes16_to_limbs(&rb, &mut r);
    let mut s = [0u64; 5];
    bytes16_to_limbs(&key[16..32], &mut s);

    let mut h = [0u64; 5];
    let mut off = 0usize;
    while off < msg.len() {
        let n = if msg.len() - off >= 16 { 16 } else { msg.len() - off };
        let mut blk = [0u64; 5];
        block_to_limbs(&msg[off..off + n], &mut blk);
        for i in 0..5 {
            h[i] += blk[i];
        }
        // schoolbook 5x5 with 2^130 fold *5; limbs < ~2^27 here, products < 2^55, sums < 2^58 fits u64
        let mut a = [0u64; 9];
        for i in 0..5 {
            for j in 0..5 {
                a[i + j] += h[i] * r[j];
            }
        }
        for k in 5..9 {
            a[k - 5] += a[k] * 5;
        }
        h[0] = a[0];
        h[1] = a[1];
        h[2] = a[2];
        h[3] = a[3];
        h[4] = a[4];
        fold_carry(&mut h);
        // value < 2p after fold => may need TWO conditional subtractions
        if ge_p(&h) {
            sub_p(&mut h);
        }
        if ge_p(&h) {
            sub_p(&mut h);
        }
        off += n;
    }

    fold_carry(&mut h);
    if ge_p(&h) {
        sub_p(&mut h);
    }
    if ge_p(&h) {
        sub_p(&mut h);
    }
    add_s_and_serialize(out, &h, &s);
}

pub fn verify(tag_actual: &[u8; 16], tag_expected: &[u8; 16]) -> bool {
    let mut diff = 0u8;
    for i in 0..16 {
        diff |= tag_actual[i] ^ tag_expected[i];
    }
    diff == 0
}
