//! X25519 scalar multiplication (Montgomery ladder, RFC 7748).
use crate::field;

/// out = scalar * u (Curve25519 MXH). Clamps scalar per RFC 7748 §5.
pub fn scalarmul(out: &mut [u8; 32], scalar: &[u8; 32], u_in: &[u8; 32]) {
    let mut e = *scalar;
    e[0] &= 248;
    e[31] &= 127;
    e[31] |= 64;

    let mut x1 = field::ZERO;
    field::from_bytes(&mut x1, u_in);
    let mut x2 = field::ONE;
    let mut z2 = field::ZERO;
    let mut x3 = x1;
    let mut z3 = field::ONE;
    let a24: field::Fe = [121665, 0, 0, 0, 0];

    let mut swap: u64 = 0;
    let mut bitidx = 255usize;
    while bitidx > 0 {
        bitidx -= 1;
        let bit = ((e[bitidx / 8] >> (bitidx % 8)) & 1) as u64;
        swap ^= bit;
        field::cswap(swap, &mut x2, &mut x3);
        field::cswap(swap, &mut z2, &mut z3);
        swap = bit;

        let mut a = field::ZERO;
        let mut b = field::ZERO;
        let mut c = field::ZERO;
        let mut d = field::ZERO;
        field::add(&mut a, &x2, &z2);
        field::sub(&mut b, &x2, &z2);
        field::add(&mut c, &x3, &z3);
        field::sub(&mut d, &x3, &z3);

        let mut da = field::ZERO;
        let mut cb = field::ZERO;
        field::mul(&mut da, &d, &a);
        field::mul(&mut cb, &c, &b);

        let mut t = field::ZERO;
        field::add(&mut t, &da, &cb);
        field::square(&mut x3, &t);
        field::sub(&mut t, &da, &cb);
        let e0 = t;
        field::square(&mut t, &e0);
        field::mul(&mut z3, &x1, &t);

        field::square(&mut da, &a); // AA
        field::square(&mut cb, &b); // BB
        field::mul(&mut x2, &da, &cb);
        field::sub(&mut t, &da, &cb); // E = AA-BB
        field::mul(&mut a, &t, &a24); // a24*E
        let e1 = a;
        field::add(&mut a, &e1, &da); // AA + a24*E
        field::mul(&mut z2, &t, &a);
    }
    field::cswap(swap, &mut x2, &mut x3);
    field::cswap(swap, &mut z2, &mut z3);

    let mut zi = field::ZERO;
    field::invert(&mut zi, &z2);
    let mut r = field::ZERO;
    field::mul(&mut r, &x2, &zi);
    field::to_bytes(out, &r);
}

/// Public key from a private key: out = scalarmul(priv, 9).
pub fn public_key(out: &mut [u8; 32], privk: &[u8; 32]) {
    let mut base = [0u8; 32];
    base[0] = 9;
    scalarmul(out, privk, &base);
}
