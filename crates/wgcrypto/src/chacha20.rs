//! ChaCha20 core (RFC 8439 §2.3-2.4). 12-byte nonce + u32 counter.
const CONST: [u32; 4] = [0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574];

#[inline]
fn rotl(v: u32, c: u32) -> u32 {
    (v << c) | (v >> (32 - c))
}

#[inline]
fn qr(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] ^= s[a];
    s[d] = rotl(s[d], 16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] ^= s[c];
    s[b] = rotl(s[b], 12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] ^= s[a];
    s[d] = rotl(s[d], 8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] ^= s[c];
    s[b] = rotl(s[b], 7);
}

fn block_bytes(out: &mut [u8; 64], key: &[u8; 32], nonce: &[u8; 12], counter: u32) {
    let mut s = [0u32; 16];
    s[0..4].copy_from_slice(&CONST);
    for i in 0..8 {
        s[4 + i] = u32::from_le_bytes([
            key[4 * i],
            key[4 * i + 1],
            key[4 * i + 2],
            key[4 * i + 3],
        ]);
    }
    s[12] = counter;
    for i in 0..3 {
        s[13 + i] = u32::from_le_bytes([
            nonce[4 * i],
            nonce[4 * i + 1],
            nonce[4 * i + 2],
            nonce[4 * i + 3],
        ]);
    }
    let w = s;
    for _ in 0..10 {
        qr(&mut s, 0, 4, 8, 12);
        qr(&mut s, 1, 5, 9, 13);
        qr(&mut s, 2, 6, 10, 14);
        qr(&mut s, 3, 7, 11, 15);
        qr(&mut s, 0, 5, 10, 15);
        qr(&mut s, 1, 6, 11, 12);
        qr(&mut s, 2, 7, 8, 13);
        qr(&mut s, 3, 4, 9, 14);
    }
    for i in 0..16 {
        let v = s[i].wrapping_add(w[i]).to_le_bytes();
        out[4 * i] = v[0];
        out[4 * i + 1] = v[1];
        out[4 * i + 2] = v[2];
        out[4 * i + 3] = v[3];
    }
}

/// Full 64-byte keystream block (counter in low half of RFC's 8-byte field via 12B nonce).
pub fn keystream_block(out: &mut [u8; 64], key: &[u8; 32], nonce: &[u8; 12], counter: u32) {
    block_bytes(out, key, nonce, counter);
}

/// XOR data with the keystream starting at `counter` (encrypt == decrypt).
pub fn xor_stream(key: &[u8; 32], nonce: &[u8; 12], counter: u32, data: &mut [u8]) {
    let mut ks = [0u8; 64];
    let mut ctr = counter;
    let mut off = 0usize;
    while off < data.len() {
        block_bytes(&mut ks, key, nonce, ctr);
        let n = if data.len() - off < 64 {
            data.len() - off
        } else {
            64
        };
        for i in 0..n {
            data[off + i] ^= ks[i];
        }
        off += n;
        ctr = ctr.wrapping_add(1);
    }
}
