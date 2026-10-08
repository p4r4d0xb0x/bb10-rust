//! Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s — WireGuard handshake.
//! Literal port of wireguard-go device/noise-protocol.go (v1.0 semantics).
//! Functional core: every step is a pure fn over explicit state; I/O lives elsewhere.
#![allow(clippy::needless_range_loop)]

use crate::aead;
use crate::blake2s;
use crate::hkdf;
use crate::x25519;

pub const MSG_INITIATION_LEN: usize = 148;
pub const MSG_RESPONSE_LEN: usize = 92;
pub const MSG_TRANSPORT_HDR_LEN: usize = 16;

const NOISE_CONSTRUCTION: &[u8] = b"Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s";
const WG_IDENTIFIER: &[u8] = b"WireGuard v1 zx2c4 Jason@zx2c4.com";

pub type Key = [u8; 32];

/// tai64n.now(): 0x400000000000000a + unix_secs (BE u64), nanos & !0xffffff (BE u32)
pub fn tai64n_now(unix_secs: u64, nanos: u32) -> [u8; 12] {
    let mut t = [0u8; 12];
    t[..8].copy_from_slice(&(0x4000_0000_0000_000au64 + unix_secs).to_be_bytes());
    t[8..].copy_from_slice(&(nanos & !0x00ff_ffffu32).to_be_bytes());
    t
}

fn clamp(sk: &mut [u8; 32]) {
    sk[0] &= 248;
    sk[31] = (sk[31] & 127) | 64;
}

/// public key of a (possibly unclamped) private key — x25519(clamp(sk), 9)
pub fn pub_of(sk: &Key) -> Key {
    let mut p = [0u8; 32];
    x25519::public_key(&mut p, sk);
    p
}

fn ss_of(sk: &Key, pk: &Key) -> Key {
    let mut e = *sk;
    clamp(&mut e);
    let mut ss = [0u8; 32];
    x25519::scalarmul(&mut ss, &e, pk);
    ss
}

/// public wrapper: X25519(clamp(sk), pk) — used for precomputed SSS
pub fn shared_secret(sk: &Key, pk: &Key) -> Key {
    ss_of(sk, pk)
}

fn mix_hash(h: &mut Key, data1: &[u8], data2: &[u8]) {
    let mut s = blake2s::State::new_unkeyed();
    s.update(h);
    s.update(data1);
    if !data2.is_empty() {
        s.update(data2);
    }
    s.finalize(h);
}

fn mix_key(ck: &mut Key, input: &[u8]) {
    let cur = *ck;
    hkdf::kdf1(ck, &cur, input);
}

/// symmetric handshake state
#[derive(Clone, Copy)]
pub struct SymState {
    pub hash: Key,
    pub chain_key: Key,
}

/// initial constants: ck0 = H(NoiseConstruction), h0 = H(ck0 || WGIdentifier)
pub fn initial_state() -> SymState {
    let mut ck = [0u8; 32];
    blake2s::hash(&mut ck, NOISE_CONSTRUCTION);
    let mut h = ck;
    mix_hash(&mut h, WG_IDENTIFIER, &[]);
    SymState { hash: h, chain_key: ck }
}

pub struct InitiationOut {
    pub msg: [u8; MSG_INITIATION_LEN],
    pub state: SymState,
    pub local_e_sk: Key,
}

/// initiator CreateMessageInitiation
pub fn create_initiation(
    st: &SymState,
    local_static_sk: &Key,
    remote_static_pk: &Key,
    sss: &Key,
    e_sk: &Key,
    sender_index: u32,
    unix_secs: u64,
    nanos: u32,
) -> InitiationOut {
    let mut s = *st;
    let e = *e_sk;
    let e_pub = pub_of(&e);
    mix_hash(&mut s.hash, remote_static_pk, &[]);

    let mut msg = [0u8; MSG_INITIATION_LEN];
    msg[..4].copy_from_slice(&1u32.to_le_bytes());
    msg[4..8].copy_from_slice(&sender_index.to_le_bytes());
    msg[8..40].copy_from_slice(&e_pub);

    mix_key(&mut s.chain_key, &e_pub);
    mix_hash(&mut s.hash, &e_pub, &[]);

    // encrypt static key
    let ss = ss_of(&e, remote_static_pk);
    let mut key = [0u8; 32];
    let ck = s.chain_key;
    hkdf::kdf2(&mut s.chain_key, &mut key, &ck, &ss);
    let my_pub = pub_of(local_static_sk);
    {
        let mut ct = [0u8; 32];
        ct.copy_from_slice(&my_pub);
        let mut tag = [0u8; 16];
        aead::seal(&mut tag, &key, &[0u8; 12], &s.hash, &mut ct);
        msg[40..72].copy_from_slice(&ct);
        msg[72..88].copy_from_slice(&tag);
    }
    mix_hash(&mut s.hash, &msg[40..88], &[]);

    // encrypt timestamp
    let ck = s.chain_key;
    hkdf::kdf2(&mut s.chain_key, &mut key, &ck, sss);
    {
        let mut ts: [u8; 12] = tai64n_now(unix_secs, nanos);
        let mut tag = [0u8; 16];
        aead::seal(&mut tag, &key, &[0u8; 12], &s.hash, &mut ts);
        msg[88..100].copy_from_slice(&ts);
        msg[100..116].copy_from_slice(&tag);
    }
    mix_hash(&mut s.hash, &msg[88..116], &[]);
    // MAC1/MAC2 left zero — filled by the rate-limiter path on the wire
    InitiationOut { msg, state: s, local_e_sk: e }
}

pub struct InitState {
    pub state: SymState,
    pub peer_static: Key,
    pub timestamp: [u8; 12],
}

/// responder ConsumeMessageInitiation. Returns None if auth fails.
pub fn consume_initiation(
    my_static_sk: &Key,
    msg: &[u8; MSG_INITIATION_LEN],
) -> Option<InitState> {
    if u32::from_le_bytes(msg[..4].try_into().unwrap()) != 1 {
        return None;
    }
    let mut s = initial_state();
    let my_pub = pub_of(my_static_sk);
    mix_hash(&mut s.hash, &my_pub, &[]);
    let e_pub: Key = msg[8..40].try_into().unwrap();
    mix_hash(&mut s.hash, &e_pub, &[]);
    mix_key(&mut s.chain_key, &e_pub);

    let ss = ss_of(my_static_sk, &e_pub);
    let mut key = [0u8; 32];
    let ck = s.chain_key;
    hkdf::kdf2(&mut s.chain_key, &mut key, &ck, &ss);
    let mut ct: [u8; 32] = msg[40..72].try_into().unwrap();
    let tag: [u8; 16] = msg[72..88].try_into().unwrap();
    if !aead::open(&tag, &key, &[0u8; 12], &s.hash, &mut ct) {
        return None;
    }
    let peer_static = ct;
    mix_hash(&mut s.hash, &msg[40..88], &[]);
    Some(InitState { state: s, peer_static, timestamp: [0u8; 12] })
}

/// continue consuming once the peer (with sss) is known
pub fn consume_initiation_verify_sss(
    init: &mut InitState,
    msg: &[u8; MSG_INITIATION_LEN],
    sss: &Key,
) -> bool {
    let mut key = [0u8; 32];
    let ck = init.state.chain_key;
    hkdf::kdf2(&mut init.state.chain_key, &mut key, &ck, sss);
    let mut ts: [u8; 12] = msg[88..100].try_into().unwrap();
    let tag: [u8; 16] = msg[100..116].try_into().unwrap();
    if !aead::open(&tag, &key, &[0u8; 12], &init.state.hash, &mut ts) {
        return false;
    }
    init.timestamp = ts;
    mix_hash(&mut init.state.hash, &msg[88..116], &[]);
    true
}

pub struct ResponseOut {
    pub msg: [u8; MSG_RESPONSE_LEN],
    pub state: SymState,
}

/// responder CreateMessageResponse — DHs mirror wireguard-go:
/// ss1 = localEphemeral x remoteEphemeral, ss2 = localEphemeral x remoteStatic
pub fn create_response(
    init: &InitState,
    e_i_pk: &Key,
    i_spk: &Key,
    e_sk: &Key,
    psk: &Key,
    my_static_sk: &Key,
    sender_index: u32,
    receiver_index: u32,
) -> ResponseOut {
    let mut s = init.state;
    let e = *e_sk;
    let e_pub = pub_of(&e);
    let mut msg = [0u8; MSG_RESPONSE_LEN];
    msg[..4].copy_from_slice(&2u32.to_le_bytes());
    msg[4..8].copy_from_slice(&sender_index.to_le_bytes());
    msg[8..12].copy_from_slice(&receiver_index.to_le_bytes());
    msg[12..44].copy_from_slice(&e_pub);

    mix_hash(&mut s.hash, &e_pub, &[]);
    mix_key(&mut s.chain_key, &e_pub);

    let ss1 = ss_of(&e, e_i_pk);
    mix_key(&mut s.chain_key, &ss1);
    let ss2 = ss_of(&e, i_spk);
    mix_key(&mut s.chain_key, &ss2);
    let _ = my_static_sk;

    let mut tau = [0u8; 32];
    let mut key = [0u8; 32];
    let ck = s.chain_key;
    hkdf::kdf3(&mut s.chain_key, &mut tau, &mut key, &ck, psk);
    mix_hash(&mut s.hash, &tau, &[]);

    let mut empty: [u8; 0] = [];
    let mut tag = [0u8; 16];
    aead::seal(&mut tag, &key, &[0u8; 12], &s.hash, &mut empty);
    msg[44..60].copy_from_slice(&tag);
    mix_hash(&mut s.hash, &msg[44..60], &[]);
    ResponseOut { msg, state: s }
}

/// initiator ConsumeMessageResponse
pub fn consume_response(
    init_state: &SymState,
    my_static_sk: &Key,
    my_e_sk: &Key,
    msg: &[u8; MSG_RESPONSE_LEN],
    psk: &Key,
) -> Option<SymState> {
    if u32::from_le_bytes(msg[..4].try_into().unwrap()) != 2 {
        return None;
    }
    let mut s = *init_state;
    let e_pub: Key = msg[12..44].try_into().unwrap();
    mix_hash(&mut s.hash, &e_pub, &[]);
    mix_key(&mut s.chain_key, &e_pub);

    let ss1 = ss_of(my_e_sk, &e_pub);
    mix_key(&mut s.chain_key, &ss1);
    let ss2 = ss_of(my_static_sk, &e_pub);
    mix_key(&mut s.chain_key, &ss2);

    let mut tau = [0u8; 32];
    let mut key = [0u8; 32];
    let ck = s.chain_key;
    hkdf::kdf3(&mut s.chain_key, &mut tau, &mut key, &ck, psk);
    mix_hash(&mut s.hash, &tau, &[]);

    let tag: [u8; 16] = msg[44..60].try_into().unwrap();
    let mut empty: [u8; 0] = [];
    if !aead::open(&tag, &key, &[0u8; 12], &s.hash, &mut empty) {
        return None;
    }
    mix_hash(&mut s.hash, &msg[44..60], &[]);
    Some(s)
}

/// transport session keys; initiator=true -> send=C1,recv=C2 (wireguard-go BeginSymmetricSession)
pub fn begin_symmetric(s: &SymState, initiator: bool) -> SymKeys {
    let mut c1 = [0u8; 32];
    let mut c2 = [0u8; 32];
    hkdf::kdf2(&mut c1, &mut c2, &s.chain_key, &[]);
    if initiator {
        SymKeys { send: c1, recv: c2 }
    } else {
        SymKeys { send: c2, recv: c1 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SymKeys {
    pub send: Key,
    pub recv: Key,
}

/// transport nonce = 0^4 || counter (LE u64).
/// WG kernel: `counter |= nonce[4..8] << 32`, nonce prefix = key_id in header.
/// Confirmed against live kernel (EXP-RUST-010C-5 layout battery: only this
/// layout opens ctr>=1; LE-first opened only ctr==0).
pub fn counter_nonce(counter: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&counter.to_le_bytes());
    n
}

/// generic seal into caller-provided buffer; returns total length or None if too long
pub fn seal_transport(
    key: &Key,
    counter: u64,
    receiver: u32,
    plain: &[u8],
    buf: &mut [u8],
) -> Option<usize> {
    let total = MSG_TRANSPORT_HDR_LEN + plain.len() + 16;
    if buf.len() < total {
        return None;
    }
    buf[..4].copy_from_slice(&4u32.to_le_bytes());
    buf[4..8].copy_from_slice(&receiver.to_le_bytes());
    buf[8..16].copy_from_slice(&counter.to_le_bytes());
    buf[MSG_TRANSPORT_HDR_LEN..MSG_TRANSPORT_HDR_LEN + plain.len()].copy_from_slice(plain);
    let mut tag = [0u8; 16];
    aead::seal(
        &mut tag,
        key,
        &counter_nonce(counter),
        &[],
        &mut buf[MSG_TRANSPORT_HDR_LEN..MSG_TRANSPORT_HDR_LEN + plain.len()],
    );
    buf[MSG_TRANSPORT_HDR_LEN + plain.len()..total].copy_from_slice(&tag);
    Some(total)
}

/// open a transport message; plaintext appended to `out`, returns plaintext length.
pub fn open_transport(key: &Key, msg: &[u8], out: &mut [u8]) -> Option<(u64, usize)> {
    if msg.len() < MSG_TRANSPORT_HDR_LEN + 16 {
        return None;
    }
    if u32::from_le_bytes(msg[..4].try_into().unwrap()) != 4 {
        return None;
    }
    let counter = u64::from_le_bytes(msg[8..16].try_into().unwrap());
    let n = msg.len() - MSG_TRANSPORT_HDR_LEN - 16;
    if out.len() < n {
        return None;
    }
    out[..n].copy_from_slice(&msg[MSG_TRANSPORT_HDR_LEN..MSG_TRANSPORT_HDR_LEN + n]);
    let tag: [u8; 16] = msg[MSG_TRANSPORT_HDR_LEN + n..].try_into().unwrap();
    if !aead::open(&tag, key, &counter_nonce(counter), &[], &mut out[..n]) {
        return None;
    }
    Some((counter, n))
}
/// WG cookie.c precompute_key: UNKEYED blake2s-256(label || pubkey)
pub fn precompute_key(out: &mut Key, label8: &[u8; 8], pubkey: &Key) {
  let mut s = blake2s::State::new_unkeyed();
  s.update(label8);
  s.update(pubkey);
  s.finalize(out);
}

/// 16-byte mac1 key for a static pubkey (kernel: key = blake2s("mac1----"||pk))
pub fn mac1_key(out: &mut Key, static_pk: &Key) {
  precompute_key(out, b"mac1----", static_pk);
}

/// fill mac1 into initiation msg bytes 116..132 (keyed blake2s-128 over msg[..116])
pub fn fill_mac1_initiation(msg: &mut [u8; MSG_INITIATION_LEN], key: &[u8; 32]) {
  let mut mac = [0u8; 16];
  blake2s::hash_keyed16(&mut mac, &msg[..116], key);
  msg[116..132].copy_from_slice(&mac);
}

/// verify mac1 on a received initiation (len 148, mac1 at 116..132)
pub fn check_mac1_initiation(msg: &[u8; MSG_INITIATION_LEN], key: &[u8; 32]) -> bool {
  let mut mac = [0u8; 16];
  blake2s::hash_keyed16(&mut mac, &msg[..116], key);
  poly_eq(&mac, &msg[116..132])
}

/// fill mac1 into response msg bytes 60..76 (keyed over msg[..60])
pub fn fill_mac1_response(msg: &mut [u8; MSG_RESPONSE_LEN], key: &[u8; 32]) {
  let mut mac = [0u8; 16];
  blake2s::hash_keyed16(&mut mac, &msg[..60], key);
  msg[60..76].copy_from_slice(&mac);
}

/// verify mac1 on a received response (mac1 at 60..76)
pub fn check_mac1_response(msg: &[u8; MSG_RESPONSE_LEN], key: &[u8; 32]) -> bool {
  let mut mac = [0u8; 16];
  blake2s::hash_keyed16(&mut mac, &msg[..60], key);
  poly_eq(&mac, &msg[60..76])
}

fn poly_eq(a: &[u8; 16], b: &[u8]) -> bool {
  let mut acc = 0u8;
  for i in 0..16 {
    acc |= a[i] ^ b[i];
  }
  acc == 0
}
