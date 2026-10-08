//! Hangul syllable glyphs (generated 12x22 table) + UTF-8 scalar decoder.
//! Canvas::text routes every byte stream through here: ASCII cell, syllable
//! cell, or '?' — one codepoint = one 12px cell, so all layout math is unchanged.

include!("hangul_data.rs");

/// Glyph bits for codepoint `cp`: Some(bytes) for syllables 가..힣.
pub fn glyph_syllable(cp: u32) -> Option<&'static [u8]> {
    if (0xAC00..0xAC00 + HANGUL_N as u32).contains(&cp) {
        Some(&HANGUL[(cp - 0xAC00) as usize][..])
    } else {
        None
    }
}

/// Decode one UTF-8 scalar at `s[0]`. Returns (codepoint, byte_len).
/// Invalid lead/continuation -> (0xFFFD, consumed+1) so the caller draws '?' and
/// resyncs at the next byte (never desyncs, never loops).
pub fn decode(s: &[u8]) -> (u32, usize) {
    let b0 = s[0];
    if b0 < 0x80 {
        return (b0 as u32, 1);
    }
    let (need, mut cp) = match b0 {
        0xC2..=0xDF => (1usize, (b0 & 0x1F) as u32),
        0xE0..=0xEF => (2, (b0 & 0x0F) as u32),
        0xF0..=0xF4 => (3, (b0 & 0x07) as u32),
        _ => return (0xFFFD, 1), // C0/C1/ED surrogates/F5+/lone continuation
    };
    if s.len() <= need {
        return (0xFFFD, 1);
    }
    for i in 1..=need {
        if s[i] & 0xC0 != 0x80 {
            return (0xFFFD, 1);
        }
        cp = (cp << 6) | (s[i] & 0x3F) as u32;
    }
    let min = match need {
        1 => 0x80u32,
        2 => 0x800,
        _ => 0x10000,
    };
    if cp < min || cp > 0x10FFFF || (0xD800..0xE000).contains(&cp) {
        return (0xFFFD, 1); // overlong / surrogate / out of range
    }
    (cp, need + 1)
}

/// True if byte is a plausible UTF-8 lead for a multi-byte scalar.
pub fn is_lead(b: u8) -> bool {
    (0xC2..=0xF4).contains(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_vectors() {
        assert_eq!(decode(b"A"), (0x41, 1));
        assert_eq!(decode("한".as_bytes()), (0xD55C, 3));
        assert_eq!(decode("힣".as_bytes()), (0xD7A3, 3));
        assert_eq!(decode("가".as_bytes()), (0xAC00, 3));
        assert_eq!(decode("😀".as_bytes()), (0x1F600, 4));
        // overlong C0 80, lone continuation, truncated lead
        assert_eq!(decode(&[0xC0, 0x80]), (0xFFFD, 1));
        assert_eq!(decode(&[0x80]), (0xFFFD, 1));
        assert_eq!(decode(&[0xE2, 0x82]), (0xFFFD, 1));
        // surrogate
        assert_eq!(decode(&[0xED, 0xA0, 0x80]), (0xFFFD, 1));
    }

    #[test]
    fn syllable_slots() {
        assert!(glyph_syllable(0xAC00).is_some()); // 가
        assert!(glyph_syllable(0xD7A3).is_some()); // 힣 = last generated
        assert!(glyph_syllable(0xD7A4).is_none());
        assert!(glyph_syllable(0x41).is_none());
        // 가 must have ink; a middle syllable like 하 must too (old euc bug skipped it)
        assert!(glyph_syllable(0xD558).unwrap().iter().any(|&b| b != 0)); // 하
        assert!(glyph_syllable(0xD574).unwrap().iter().any(|&b| b != 0)); // 해
    }
}
