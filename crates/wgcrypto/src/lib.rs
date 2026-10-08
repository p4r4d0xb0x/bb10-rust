//! wgcrypto — no_std-ready primitives for a userspace WireGuard client.
//! Each module is one primitive, validated against RFC vectors in tests/.
#![cfg_attr(not(test), no_std)]
#![allow(clippy::needless_range_loop)]

pub mod aead;
pub mod blake2s;
pub mod chacha20;
pub mod field;
pub mod hkdf;
pub mod noise;
pub mod poly1305;
pub mod x25519;


