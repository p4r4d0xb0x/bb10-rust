//! ChaCha20-Poly1305 AEAD (RFC 8439 §2.8).
use crate::chacha20;
use crate::poly1305;

/// one-time Poly1305 key = first 32 bytes of chacha20 block counter=0
pub fn poly_key(out: &mut [u8; 32], key: &[u8; 32], nonce: &[u8; 12]) {
    let mut b = [0u8; 64];
    chacha20::keystream_block(&mut b, key, nonce, 0);
    out.copy_from_slice(&b[..32]);
}

fn mac(out: &mut [u8; 16], polyk: &[u8; 32], aad: &[u8], ct: &[u8]) {
    let mut m = [0u8; 1024 * 16];
    let mut n = 0usize;
    m[..aad.len()].copy_from_slice(aad);
    n += aad.len();
    let pad = (16 - (aad.len() % 16)) % 16;
    n += pad;
    m[n..n + ct.len()].copy_from_slice(ct);
    n += ct.len();
    let pad = (16 - (ct.len() % 16)) % 16;
    n += pad; // mandatory: length block must start on a 16-byte boundary
    let la = (aad.len() as u64).to_le_bytes();
    let lc = (ct.len() as u64).to_le_bytes();
    m[n..n + 8].copy_from_slice(&la);
    m[n + 8..n + 16].copy_from_slice(&lc);
    n += 16;
    poly1305::tag(out, polyk, &m[..n]);
}

/// encrypt in place: ct buffer = plaintext bytes; tag appended by caller use
pub fn seal(tag_out: &mut [u8; 16], key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], data: &mut [u8]) {
    let mut polyk = [0u8; 32];
    poly_key(&mut polyk, key, nonce);
    chacha20::xor_stream(key, nonce, 1, data);
    mac(tag_out, &polyk, aad, data);
}

/// returns false on tag mismatch; data becomes plaintext on success
pub fn open(tag_in: &[u8; 16], key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], data: &mut [u8]) -> bool {
    let mut polyk = [0u8; 32];
    poly_key(&mut polyk, key, nonce);
    let mut t = [0u8; 16];
    mac(&mut t, &polyk, aad, data);
    if !poly1305::verify(&t, tag_in) {
        return false;
    }
    chacha20::xor_stream(key, nonce, 1, data);
    true
}
