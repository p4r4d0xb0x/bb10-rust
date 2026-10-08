//! Internal field unit tests (host-side): isolate field bugs from ladder bugs.
use crate::field;

fn to_hex(f: &field::Fe) -> String {
    let mut b = [0u8; 32];
    field::to_bytes(&mut b, f);
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

#[test]
fn roundtrip_small() {
    let mut f = field::ZERO;
    let mut inb = [0u8; 32];
    inb[..8].copy_from_slice(&0x0123456789abcdef_u64.to_le_bytes());
    field::from_bytes(&mut f, &inb);
    assert_eq!(to_hex(&f), "efcdab8967452301000000000000000000000000000000000000000000000000");
}

#[test]
fn mul_small() {
    // 3 * 3 = 9
    let mut a = field::ZERO;
    a[0] = 3;
    let mut r = field::ZERO;
    field::mul(&mut r, &a, &a);
    let mut e = field::ZERO;
    e[0] = 9;
    field::reduce(&mut r);
    field::reduce(&mut e);
    assert_eq!(to_hex(&r), to_hex(&e));
}

#[test]
fn invert_consistency() {
    // for x = 12345: x * x^-1 == 1
    let mut x = field::ZERO;
    x[0] = 12345;
    let mut xi = field::ZERO;
    field::invert(&mut xi, &x);
    let mut prod = field::ZERO;
    field::mul(&mut prod, &x, &xi);
    field::reduce(&mut prod);
    assert_eq!(to_hex(&prod), {
        let mut o = [0u8; 32];
        o[0] = 1;
        o.iter().map(|x| format!("{:02x}", x)).collect::<String>()
    });
}
