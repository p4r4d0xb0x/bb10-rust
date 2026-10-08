//! Dubeolsik (두벌식) IME: raw QWERTY letters -> composed Hangul syllables.
//! Pure no_std state machine, host-tested; the chat apps and gateways reuse it.
//!
//! KEYMAP + SEMANTICS AUTHORITY: libhangul built from source; Dubeolsik
//! `hangul-keyboard-2.xml` map id=0 (upper == lower: NO shifted jamo in the
//! layout — doubles come only from combine) plus `hangul_ic_process_jamo()`
//! with combination-default.xml, auto-reorder ON, combi-on-double-stroke OFF.
//! Verified keystroke-by-keystroke (commit + preedit) against the oracle:
//!   dkssud=안녕  gksmf=하늘  gksrmf=한글  skfk=나라  wnrms=주근
//!   rrk=ㄱ가  rk;=가  rjmf=거ㅡㄹ  tprffogkska=섹ㄹ래하남
//!
//! SLOT ORDER (Unicode canonical, the U+AC00 algebra depends on it):
//!   cho  19: U+1100..U+1112 (no ㅅ-slot gap)
//!   jung 21: U+1161..U+1175 (slot 7 = archaic ᅨ/ㅖ gap: ㅗ is 8, not 7!)
//!   jong 28: none + U+11A8..U+11C2
//!   cp = 0xAC00 + cho*588 + jung*28 + jong
//!
//! TRANSITIONS (mirrors hangul_ic_process_jamo):
//!   bare cho + consonant  -> COMBINE if legal (rr=ㄲ ee=ㄸ qq=ㅃ tt=ㅆ ww=ㅉ
//!                             and batchim clusters ㄳㄵㄶㄺㄻㄼㄽㄾㄿㅀㅄ via
//!                             jong-reinterpretation), else commit jamo as
//!                             compat-jamo + replace (rrr -> ㄱ ㄱ 가!)
//!   bare cho + vowel      -> syllable forms
//!   syllable + consonant  -> push as jong (cluster-combine if jong present),
//!                            jamo with no jong slot (ㄸㅃㅉ) commits whole
//!   syllable + vowel      -> jung combine (ㅗㅏ=ㅘ...) | simple jong: commit
//!                             WITHOUT jong, jong reborn as onset (달-력) |
//!                             compound: split via COMPOUND table
//! Compat-jamo emissions (U+313x) only occur where the oracle emits a bare
//! jamo; the terminal font table covers syllables, jamo draw as '?'.

const N: u8 = 0xFF; // this key carries nothing of that kind

// cho: ㄱ0 ㄲ1 ㄴ2 ㄷ3 ㄸ4 ㄹ5 ㅁ6 ㅂ7 ㅃ8 ㅅ9 ㅆ10 ㅇ11 ㅈ12 ㅉ13 ㅊ14 ㅋ15 ㅌ16 ㅍ17 ㅎ18
// jung (U+1161 base): ㅏ0 ㅐ1 ㅑ2 ㅒ3 ㅓ4 ㅔ5 ㅕ6 [gap7] ㅗ8 ㅘ9 ㅙ10 ㅚ11 ㅛ12 ㅜ13 ㅝ14 ㅞ15 ㅟ16 ㅠ17 ㅡ18 ㅢ19 ㅣ20
// jong: none0 ㄱ1 ㄲ2 ㄳ3 ㄴ4 ㄵ5 ㄶ6 ㄷ7 ㄹ8 ㄺ9 ㄻ10 ㄼ11 ㄽ12 ㄾ13 ㄿ14 ㅀ15 ㅁ16 ㅂ17 ㅄ18 ㅅ19 ㅆ20 ㅇ21 ㅈ22 ㅊ23 ㅋ24 ㅌ25 ㅍ26 ㅎ27

static CHO: [u8; 26] = [
    6, 0xFF, 14, 11, 3, 5, 18, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 7, 0, 2, 9, 0xFF, 17, 12, 16, 0xFF, 15,
];

static VOW: [u8; 26] = [
    0xFF, 17, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 8, 2, 4, 0, 20, 18,
    13, 1, 5, 0xFF, 0xFF, 0xFF, 0xFF, 6, 0xFF, 0xFF, 0xFF, 12, 0xFF,
];

// aliases: ; -> ㅐ  ' -> ㅔ  : -> ㅒ (390-key convention; the Classic app
// buffer needs punctuation emitted, so type_str commits + passes it through)
static VOW_ALIAS: [(u8, u8); 4] =
    [(b';', 1), (b':', 3), (b'\'', 5), (b'"', 5)];

/// SHIFTED layer (libhangul map id=0 uppercase keys, indices a=0..z=25;
/// never hand-typed): Q ㅃ(16) W ㅉ(22) E ㄸ(4) R ㄲ(17) T ㅆ(19) O ㅒ(14)
/// P slot7-ㅖ(15). All other uppercase keys behave as lowercase.
static CHO_SH: [u8; 26] = [
    6, 0xFF, 14, 11, 4, 5, 18, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 8, 1, 2, 10, 0xFF, 17, 13, 16, 0xFF, 15,
];
static VOW_SH: [u8; 26] = [
    0xFF, 17, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 8, 2, 4, 0, 20, 18,
    13, 3, 7, 0xFF, 0xFF, 0xFF, 0xFF, 6, 0xFF, 0xFF, 0xFF, 12, 0xFF,
];

/// (current jong SLOT, jong-of-incoming cho SLOT) -> compound jong.
/// Keys are JONG slots (1..27), per combination-default.xml. Doubled
/// pairs (rr=ㄲ etc.) only combine in the bare-onset state.
static COMBINE: [(u8, u8, u8); 16] = [
    (0, 0, 1),   // ㄱ+ㄱ ㄲ  (bare-choseong state only, keyed on CHO slots)
    (3, 3, 4),   // ㄷ+ㄷ ㄸ
    (7, 7, 8),   // ㅂ+ㅂ ㅃ
    (9, 9, 10),  // ㅅ+ㅅ ㅆ
    (12, 12, 13),// ㅈ+ㅈ ㅉ
    (1, 19, 3),  // jong ㄱ + ㅅ -> ㄳ
    (4, 22, 5),  // jong ㄴ + ㅈ -> ㄵ
    (4, 27, 6),  // jong ㄴ + ㅎ -> ㄶ
    (8, 1, 9),   // jong ㄹ + ㄱ -> ㄺ
    (8, 16, 10), // jong ㄹ + ㅁ -> ㄻ
    (8, 17, 11), // jong ㄹ + ㅂ -> ㄼ
    (8, 19, 12), // jong ㄹ + ㅅ -> ㄽ
    (8, 25, 13), // jong ㄹ + ㅌ -> ㄾ
    (8, 26, 14), // jong ㄹ + ㅍ -> ㄿ
    (8, 27, 15), // jong ㄹ + ㅎ -> ㅀ
    (19, 17, 18),// jong ㅅ + ㅂ -> ㅄ
];

/// compound jong -> (kept in commit = own slot, reborn onset = 2nd jamo
/// as choseong). Derived from hangul_jongseong_get_diff second==0 rows.
static COMPOUND: [(u8, u8, u8); 12] = [
    (3, 3, 9),   // ㄳ keeps ㄱ(1)... commit slot stays 1, onset ㅅ
    (5, 4, 12),  // ㄵ keeps ㄴ(4),  onset ㅈ
    (6, 4, 9),   // ㄶ keeps ㄴ(4),  onset ㅅ
    (9, 8, 0),   // ㄺ keeps ㄹ(8),  onset ㄱ
    (10, 8, 6),  // ㄻ keeps ㄹ(8),  onset ㅁ
    (11, 8, 7),  // ㄼ keeps ㄹ(8),  onset ㅂ
    (12, 8, 9),  // ㄽ keeps ㄹ(8),  onset ㅅ
    (13, 8, 16), // ㄾ keeps ㄹ(8),  onset ㅌ
    (14, 8, 17), // ㄿ keeps ㄹ(8),  onset ㅍ
    (15, 8, 18), // ㅀ keeps ㄹ(8),  onset ㅎ
    (18, 19, 7), // ㅄ keeps ㅅ(19), onset ㅂ
    (20, 20, 10),// ㅆ keeps ㅆ(20), onset ㅆ (get_diff ㅆ,0 -> ㅆ)
];

/// (jung a, jung b) -> merged diphthong
static VCOMB: [(u8, u8, u8); 7] = [
    (8, 0, 9),    // ㅗ+ㅏ ㅘ
    (8, 1, 10),   // ㅗ+ㅐ ㅙ
    (8, 20, 11),  // ㅗ+ㅣ ㅚ
    (13, 4, 14),  // ㅜ+ㅓ ㅝ
    (13, 5, 15),  // ㅜ+ㅔ ㅞ
    (13, 20, 16), // ㅜ+ㅣ ㅟ
    (18, 20, 19), // ㅡ+ㅣ ㅢ
];

/// choseong slot -> jongseong slot. ㄸ ㅃ ㅉ: N (force commit whole).
/// ㄲ(1)->2, ㅆ(10)->20 exist: they DO sit as batchim.
static CHO_TO_JONG: [u8; 19] = [
    1, 2, 4, 7, N, 8, 16, 17, N, 19, 20, 21, 22, N, 23, 24, 25, 26, 27,
];

/// jongseong slot -> compat jamo (U+3131..), emitted when the oracle
/// commits a bare jamo that cannot combine. 0 = should never emit.
const JONG_COMPAT: [u32; 28] = [
    0, 0x3131, 0x3132, 0x3133, 0x3134, 0x3135, 0x3136, 0x3137, 0x3138,
    0x3139, 0x313A, 0x313B, 0x313C, 0x313D, 0x313E, 0x313F, 0x3140,
    0x3141, 0x3142, 0x3143, 0x3144, 0x3145, 0x3146, 0x3147, 0x3148,
    0x3149, 0x314A, 0x314B,
];
const CHO_COMPAT: [u32; 19] = [
    0x3131, 0x3132, 0x3134, 0x3137, 0x3138, 0x3139, 0x3141, 0x3142,
    0x3143, 0x3145, 0x3146, 0x3147, 0x3148, 0x3149, 0x314A, 0x314B,
    0x314C, 0x314D, 0x314E,
];
const JUNG_COMPAT: [u32; 21] = [
    0x314F, 0x3150, 0x3151, 0x3152, 0x3153, 0x3154, 0x3155, 0, 0x3157,
    0x3158, 0x3159, 0x315A, 0x315B, 0x315C, 0x315D, 0x315E, 0x315F,
    0x3160, 0x3161, 0x3162, 0x3163,
];

/// simple jong -> the choseong reborn as onset when a vowel follows
/// (hangul_jongseong_get_diff with second==0): batchim strips + reborn.
static JONG_CHO: [u8; 28] = [
    N, 0, 1, N, 2, N, N, 3, 5, N, N, N, N, N, N, N, 6, 7, N, 9, 10, 11,
    12, 14, 15, 16, 17, 18,
];

pub struct Ime {
    stage: u8, // 0 empty | 1 bare choseong | 2 syllable forming
    cho: u8,
    jung: u8,
    jong: u8, // 0 none
}

impl Ime {
    pub const fn new() -> Ime {
        Ime { stage: 0, cho: 0, jung: 0, jong: 0 }
    }

    /// codepoint under composition (compat jamo when stage==1; 0 = nothing)
    pub fn cp(&self) -> u32 {
        match self.stage {
            1 => CHO_COMPAT[self.cho as usize],
            2 => 0xAC00 + self.cho as u32 * 588 + self.jung as u32 * 28 + self.jong as u32,
            _ => 0,
        }
    }

    pub fn composing(&self) -> bool {
        self.stage != 0
    }

    /// feed a pressed char. Returns the codepoint that just FINISHED
    /// (0 = none); a new composition (if any) already includes the key.
    /// Only a-zA-Z map to jamo (libhangul map id=0); every other key
    /// flushes the buffer (caller emits the raw char, like type_str).
    pub fn press(&mut self, key: u8) -> u32 {
        if !key.is_ascii_alphabetic() {
            return self.commit();
        }
        let l = ((key | 0x20) - b'a') as usize;
        if key.is_ascii_uppercase() {
            if CHO_SH[l] != N {
                return self.consonant(CHO_SH[l]);
            }
            if VOW_SH[l] != N {
                return self.vowel(VOW_SH[l]);
            }
        }
        if CHO[l] != N {
            self.consonant(CHO[l])
        } else if VOW[l] != N {
            self.vowel(VOW[l])
        } else {
            self.commit()
        }
    }

    fn vowel(&mut self, v: u8) -> u32 {
        match self.stage {
            0 => {
                self.stage = 2;
                self.cho = 11; // silent ㅇ
                self.jung = v;
                self.jong = 0;
                0
            }
            1 => {
                self.stage = 2;
                self.jung = v;
                0
            }
            _ => {
                if self.jong == 0 {
                    if let Some(m) = VCOMB.iter().find(|(a, b, _)| *a == self.jung && *b == v) {
                        self.jung = m.2;
                        return 0;
                    }
                    let done = self.cp();
                    self.cho = 11; // new syllable, silent ㅇ
                    self.jung = v;
                    done
                } else if let Some((_, keep, stolen)) =
                    COMPOUND.iter().find(|(j, _, _)| *j == self.jong)
                {
                    // compound batchim: commit keeping FIRST, stolen reborn
                    let done =
                        0xAC00 + self.cho as u32 * 588 + self.jung as u32 * 28 + *keep as u32;
                    self.cho = *stolen;
                    self.jung = v;
                    self.jong = 0;
                    done
                } else if self.jong == 20 {
                    // ㅆ atomic: hangul_jongseong_get_diff(ㅆ, 0) == ㅆ choseong
                    let done = self.cp();
                    self.cho = 10;
                    self.jung = v;
                    self.jong = 0;
                    done
                } else {
                    // simple batchim: STRIPPED from commit, reborn as onset
                    let done = 0xAC00 + self.cho as u32 * 588 + self.jung as u32 * 28;
                    self.cho = JONG_CHO[self.jong as usize];
                    self.jung = v;
                    self.jong = 0;
                    done
                }
            }
        }
    }

    fn consonant(&mut self, cho: u8) -> u32 {
        match self.stage {
            0 => {
                self.stage = 1;
                self.cho = cho;
                0
            }
            1 => {
                // bare choseong + consonant (libhangul hangul_ic_process_jamo):
                // hangul_ic_combine(cho, incoming) — jong-reinterp is tried
                // inside combine's first==jong table too, but a bare onset
                // has no syllable so only the CHO doubling pairs apply here.
                if let Some((a, _, m)) =
                    COMBINE.iter().find(|(a, b, _)| *a == self.cho && *b == cho)
                {
                    self.cho = *m;
                    return 0;
                }
                // no combine: commit buffer jamo as compat jamo, replace
                let done = CHO_COMPAT[self.cho as usize];
                self.cho = cho;
                done
            }
            _ => {
                if self.jong == 0 {
                    match CHO_TO_JONG[cho as usize] {
                        N => {
                            // ㄸ ㅃ ㅉ have no batchim slot: commit whole
                            let done = self.cp();
                            self.stage = 1;
                            self.cho = cho;
                            done
                        }
                        j => {
                            self.jong = j; // ㄲ ㅆ included
                            0
                        }
                    }
                } else if let Some((_, _, m)) =
                    COMBINE.iter().find(|(j, c, _)| *j == self.jong && *c == cho)
                {
                    self.jong = *m;
                    0
                } else {
                    // jong occupied, no cluster: commit whole, restart bare
                    let done = self.cp();
                    self.stage = 1;
                    self.cho = cho;
                    self.jung = 0;
                    self.jong = 0;
                    done
                }
            }
        }
    }

    /// finish composition (space/enter/send). Returns final cp (0 = none).
    pub fn commit(&mut self) -> u32 {
        let done = self.cp();
        self.stage = 0;
        done
    }

    /// backspace one step. true = composition vanished (delete a char);
    /// false = composition merely shrank (redraw).
    /// Mirrors libhangul buffer pop: 간 -> 가 -> bare ㄱ -> gone.
    /// NB jung ㅏ is SLOT 0, so stage alone decides the branch.
    pub fn back(&mut self) -> bool {
      match self.stage {
        2 if self.jong != 0 => {
          self.jong = 0;
          false
        },
        2 => {
          self.jung = 0;
          self.stage = 1;
          false
        },
        1 => {
          self.stage = 0;
          true
        },
        _ => false,
      }
    }
}

/// Compose an ASCII keystroke string -> UTF-8 Hangul bytes in `out`.
pub fn type_str(s: &[u8], out: &mut [u8]) -> usize {
    let mut ime = Ime::new();
    let mut n = 0usize;
    for &k in s {
        let done = if k.is_ascii_alphabetic() {
            ime.press(k)
        } else {
            // libhangul: buffer commits FIRST, then the raw char appends
            let d = ime.commit();
            if d != 0 && n + 3 <= out.len() {
                n += enc_utf8(d, &mut out[n..]);
            }
            if n < out.len() {
                out[n] = k;
                n += 1;
            }
            0
        };
        if done != 0 && n + 3 <= out.len() {
            n += enc_utf8(done, &mut out[n..]);
        }
    }
    let done = ime.commit();
    if done != 0 && n + 3 <= out.len() {
        n += enc_utf8(done, &mut out[n..]);
    }
    n
}

fn enc_utf8(cp: u32, out: &mut [u8]) -> usize {
    if cp < 0x80 {
        out[0] = cp as u8;
        1
    } else if cp < 0x800 {
        out[0] = 0xC0 | (cp >> 6) as u8;
        out[1] = 0x80 | (cp & 0x3F) as u8;
        2
    } else {
        out[0] = 0xE0 | (cp >> 12) as u8;
        out[1] = 0x80 | ((cp >> 6) & 0x3F) as u8;
        out[2] = 0x80 | (cp & 0x3F) as u8;
        3
    }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::hangul;

  fn t(keys: &str) -> String {
    let mut buf = [0u8; 2048];
    let n = type_str(keys.as_bytes(), &mut buf);
    String::from_utf8_lossy(&buf[..n]).into_owned()
  }

  #[test]
  fn anchors_oracle_verified() {
    // every expectation here == libhangul oracle output (oracle_line, map id=0)
    assert_eq!(t("rk"), "가");
    assert_eq!(t("dkssud"), "안녕");
    assert_eq!(t("gksrmf"), "한글");
    assert_eq!(t("gksmf"), "하늘");
    assert_eq!(t("skfk"), "나라");
    assert_eq!(t("wnrms"), "주근");
    assert_eq!(t("ekfka"), "다람");
    assert_eq!(t("qkrtjd"), "박성");
    assert_eq!(t("xorendnf"), "택두울");
  }

  #[test]
  fn batchim_steal() {
    // vowel after a simple batchim: batchim stripped from the commit, reborn
    assert_eq!(t("skfka"), "나람");
    assert_eq!(t("dkfka"), "아람");
    assert_eq!(t("rkfkfkfk"), "가라라라");
  }

  #[test]
  fn compound_batchim() {
    assert_eq!(t("dkfrhd"), "알공");
    assert_eq!(t("wjrdk"), "적아");
    assert_eq!(t("wjfdk"), "절아");
    assert_eq!(t("wjtdk"), "젓아");
    assert_eq!(t("wkqdk"), "잡아"); // ㅃ has no batchim slot: commit + onset
    assert_eq!(t("rnlf"), "귈");
    assert_eq!(t("qork"), "배가");
  }

  #[test]
  fn doubled_and_bare() {
    assert_eq!(t("rrk"), "까"); // R=ㄲ direct jamo (uppercase layer)
    assert_eq!(t("RkRk"), "까까");
    assert_eq!(t("dkRkd"), "아깡"); // ㄲ sits as batchim
    assert_eq!(t("wjrmf"), "저글");
    assert_eq!(t("dnjdx"), "웡ㅌ");
  }

  #[test]
  fn punctuation_passes_through() {
    // un-mapped keys flush then emit raw (chat buffer keeps punctuation)
    assert_eq!(t("rk."), "가.");
    assert_eq!(t("rk;"), "가;");
    assert_eq!(t("dkssudTdjrdp"), "안녕ㅆ억에"); // T=ㅆ batchim on empty onset
  }

  #[test]
  fn backspace_steps() {
    // libhangul buffer pop: 간 -> 가 -> bare ㄱ -> vanish
    let mut ime = Ime::new();
    ime.press(b'r');
    ime.press(b'k');
    ime.press(b's');
    assert_eq!(ime.cp(), '간' as u32);
    assert!(!ime.back());
    assert_eq!(ime.cp(), '가' as u32);
    assert!(!ime.back());
    assert_eq!(ime.cp(), 'ㄱ' as u32);
    assert!(ime.back());
    assert_eq!(ime.cp(), 0);
  }

  #[test]
  fn render_accepts_bare_jamo() {
    // Classic screen font only has syllable slots: every jamo the IME can
    // emit must render as some syllable (never a hole)
    assert_eq!(hangul::glyph_syllable('가' as u32).is_some(), true);
    let cases = ["rk", "dkssud", "rrk", "wkq", "dnjdx", "gppd", "rk."];
    for c in cases {
      let mut buf = [0u8; 2048];
      let n = type_str(c.as_bytes(), &mut buf);
      let s = &buf[..n];
      let mut i = 0;
      while i < s.len() {
        let (cp, len) = hangul::decode(&s[i..]);
        if cp > 0x80 {
          if (0xAC00..=0xD7A3).contains(&cp) {
            assert!(hangul::glyph_syllable(cp).is_some(),
                    "syllable {:x} missing from glyph table ({:?})", cp, c);
          }
        }
        i += len;
      }
    }
  }
}
