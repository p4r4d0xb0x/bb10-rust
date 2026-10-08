//! HKDF over HMAC-BLAKE2s-256 (RFC 5869 shape, RFC 2104 HMAC with an
//! UNKEYED blake2s hash core — deliberately NOT blake2s native keyed mode).
//! Matches wireguard-go noise-helpers.go KDF1/KDF2/KDF3.
#![allow(clippy::needless_range_loop)]

const BLKSZ: usize = 64;

/// HMAC-BLAKE2s-256(key, msg) -> 32 bytes
pub fn hmac(out: &mut [u8; 32], key: &[u8], msg: &[u8]) {
    let mut k = [0u8; BLKSZ];
    if key.len() > BLKSZ {
        let mut h = [0u8; 32];
        crate::blake2s::hash(&mut h, key);
        k[..32].copy_from_slice(&h);
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    // ipad pass
    let mut buf = [0u8; BLKSZ];
    for i in 0..BLKSZ {
        buf[i] = k[i] ^ 0x36;
    }
    let mut inner = [0u8; 32];
    hash2(&mut inner, &buf, msg);
    // opad pass
    for i in 0..BLKSZ {
        buf[i] = k[i] ^ 0x5c;
    }
    hash2(out, &buf, &inner);
}

/// blake2s(a || b) streaming (avoids allocating concatenations)
fn hash2(out: &mut [u8; 32], a: &[u8], b: &[u8]) {
    let mut s = crate::blake2s::State::new_unkeyed();
    s.update(a);
    s.update(b);
    s.finalize(out);
}

/// T1 = HMAC(HMAC(ikm, info), 0x01)
pub fn kdf1(t0: &mut [u8; 32], ikm: &[u8], info: &[u8]) {
    let mut prk = [0u8; 32];
    hmac(&mut prk, ikm, info);
    hmac(t0, &prk, &[1u8]);
}

/// (T1, T2) = HKDF(ikm, info) with 32-byte outputs
pub fn kdf2(t0: &mut [u8; 32], t1: &mut [u8; 32], ikm: &[u8], info: &[u8]) {
    let mut prk = [0u8; 32];
    hmac(&mut prk, ikm, info);
    hmac(t0, &prk, &[1u8]);
    hmac_pair(t1, &prk, t0, &[2u8]);
}

/// (T1, T2, T3)
pub fn kdf3(t0: &mut [u8; 32], t1: &mut [u8; 32], t2: &mut [u8; 32], ikm: &[u8], info: &[u8]) {
    let mut prk = [0u8; 32];
    hmac(&mut prk, ikm, info);
    hmac(t0, &prk, &[1u8]);
    hmac_pair(t1, &prk, t0, &[2u8]);
    hmac_pair(t2, &prk, t1, &[3u8]);
}

/// HMAC(key, in0 || in1) without building the concat
fn hmac_pair(out: &mut [u8; 32], key: &[u8], in0: &[u8], in1: &[u8]) {
    let mut k = [0u8; BLKSZ];
    if key.len() > BLKSZ {
        let mut h = [0u8; 32];
        crate::blake2s::hash(&mut h, key);
        k[..32].copy_from_slice(&h);
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut buf = [0u8; BLKSZ];
    for i in 0..BLKSZ {
        buf[i] = k[i] ^ 0x36;
    }
    let mut s = crate::blake2s::State::new_unkeyed();
    s.update(&buf);
    s.update(in0);
    s.update(in1);
    let mut inner = [0u8; 32];
    s.finalize(&mut inner);
    for i in 0..BLKSZ {
        buf[i] = k[i] ^ 0x5c;
    }
    hash2(out, &buf, &inner);
}
