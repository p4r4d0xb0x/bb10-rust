//! Pure text helpers for no_std (int/u64 formatting, fixed decimals,
//! durations, bytes — dashboard needs all four).

/// Signed int -> decimal. Returns (slice of buf, digits) or (b"?",1).
pub fn i64_to_buf<'a>(v: i64, buf: &'a mut [u8]) -> (&'a [u8], usize) {
    let neg = v < 0;
    let mut mag = if neg { (v as i128).unsigned_abs() as u64 } else { v as u64 };
    let mut n = 0usize;
    let mut tmp = [0u8; 20];
    loop {
        tmp[n] = b'0' + (mag % 10) as u8;
        n += 1;
        mag /= 10;
        if mag == 0 {
            break;
        }
    }
    if neg {
        buf[0] = b'-';
        for i in 0..n {
            buf[1 + i] = tmp[n - 1 - i];
        }
        (&buf[..1 + n], 1 + n)
    } else {
        for i in 0..n {
            buf[i] = tmp[n - 1 - i];
        }
        (&buf[..n], n)
    }
}

/// Unsigned -> decimal.
pub fn u64_to_buf<'a>(v: u64, buf: &'a mut [u8]) -> (&'a [u8], usize) {
    i64_to_buf(v.min(i64::MAX as u64) as i64, buf)
}

/// Fixed-point decimal: v/scale with `frac` digits (e.g. 40900/1000,3 -> "40.900").
pub fn decimal<'a>(v: u64, scale: u64, frac: usize, buf: &'a mut [u8]) -> &'a [u8] {
    let whole = v / scale;
    let mut wb = [0u8; 20];
    let (w, mut n) = u64_to_buf(whole, &mut wb);
    buf[..n].copy_from_slice(w);
    if frac > 0 {
        buf[n] = b'.';
        n += 1;
        let rem = v % scale;
        let mut mult = scale / 10; // digit k uses scale/10^(k+1)
        if mult == 0 {
            mult = 1;
        }
        for _ in 0..frac {
            buf[n] = b'0' + ((rem / mult) % 10) as u8;
            n += 1;
            mult /= 10;
            if mult == 0 {
                mult = 1;
            }
        }
    }
    &buf[..n]
}

/// Seconds -> "3d 04:11:07" style compact ("41s", "7m12s", "2h05m", "3d04h").
pub fn duration<'a>(secs: u64, buf: &'a mut [u8]) -> &'a [u8] {
    if secs < 60 {
        let (s, n) = u64_to_buf(secs, buf);
        buf[n] = b's';
        &buf[..n + 1]
    } else if secs < 3600 {
        let m = secs / 60;
        let s = secs % 60;
        let (_, n) = u64_to_buf(m, buf);
        buf[n] = b'm';
        buf[n + 1] = b'0' + (s / 10) as u8;
        buf[n + 2] = b'0' + (s % 10) as u8;
        buf[n + 3] = b's';
        &buf[..n + 4]
    } else if secs < 86400 {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        let (_, n) = u64_to_buf(h, buf);
        buf[n] = b'h';
        buf[n + 1] = b'0' + (m / 10) as u8;
        buf[n + 2] = b'0' + (m % 10) as u8;
        buf[n + 3] = b'm';
        &buf[..n + 4]
    } else {
        let d = secs / 86400;
        let h = (secs % 86400) / 3600;
        let (_, n) = u64_to_buf(d, buf);
        buf[n] = b'd';
        buf[n + 1] = b'0' + (h / 10) as u8;
        buf[n + 2] = b'0' + (h % 10) as u8;
        buf[n + 3] = b'h';
        &buf[..n + 4]
    }
}

/// Byte count -> "512B" / "3.4K" / "1.2M".
pub fn bytes<'a>(v: u64, buf: &'a mut [u8]) -> &'a [u8] {
    if v < 1024 {
        let (s, n) = u64_to_buf(v, buf);
        buf[n] = b'B';
        &buf[..n + 1]
    } else if v < 1024 * 1024 {
        let t = (v * 10 + 512) / 1024;
        let n = append_scaled(t, b'K', buf);
        &buf[..n]
    } else {
        let t = (v * 10 + (1024 * 1024 / 2)) / (1024 * 1024);
        let n = append_scaled(t, b'M', buf);
        &buf[..n]
    }
}

fn append_scaled(tenths: u64, suffix: u8, buf: &mut [u8]) -> usize {
    let whole = tenths / 10;
    let frac = tenths % 10;
    let (_, mut n) = u64_to_buf(whole, buf);
    buf[n] = b'.';
    n += 1;
    buf[n] = b'0' + frac as u8;
    n += 1;
    buf[n] = suffix;
    n + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ints() {
        let mut b = [0u8; 24];
        assert_eq!(i64_to_buf(-1234, &mut b).0, b"-1234");
        assert_eq!(u64_to_buf(0, &mut b).0, b"0");
        assert_eq!(u64_to_buf(9876543210, &mut b).0, b"9876543210");
    }
    #[test]
    fn decimals() {
        let mut b = [0u8; 32];
        assert_eq!(decimal(40900, 1000, 3, &mut b), b"40.900");
        assert_eq!(decimal(1500, 1000, 1, &mut b), b"1.5");
        assert_eq!(decimal(7, 1000, 2, &mut b), b"0.00");
    }
    #[test]
    fn durations() {
        let mut b = [0u8; 16];
        assert_eq!(duration(9, &mut b), b"9s");
        assert_eq!(duration(73, &mut b), b"1m13s");
        assert_eq!(duration(3725, &mut b), b"1h02m");
        assert_eq!(duration(260000, &mut b), b"3d00h");
    }
    #[test]
    fn byte_units() {
        let mut b = [0u8; 16];
        assert_eq!(bytes(512, &mut b), b"512B");
        assert_eq!(bytes(3481, &mut b), b"3.4K");
        assert_eq!(bytes(1_258_291, &mut b), b"1.2M");
    }

    #[test]
    fn int_neg_and_zero() {
        let mut b = [0u8; 16];
        assert_eq!(i64_to_buf(-42, &mut b).0, b"-42");
        assert_eq!(i64_to_buf(0, &mut b).0, b"0");
        assert_eq!(i64_to_buf(-9999999, &mut b).0, b"-9999999");
    }

}
