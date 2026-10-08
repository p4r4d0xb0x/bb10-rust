//! bbui-core — tiny UI framework for BlackBerry Classic (720x720, QNX Screen).
//!
//! functional core / imperative shell:
//!   color/font/fmt/canvas/widget/event  pure logic, host-unit-tested
//!   screen                              the ONLY unsafe FFI boundary
//!
//! Screen/libbps are dlopen'd at runtime on-device (no SDK link-time deps);
//! every init step returns a distinct error code so a bring-up failure is
//! locatable from a single printed number (device is screen-remote).
#![cfg_attr(target_arch = "arm", no_std)]

#[cfg(target_arch = "arm")]
extern crate core;

pub mod canvas;
pub mod color;
pub mod event;
pub mod fmt;
pub mod font;
pub mod hangul;
pub mod ime;
pub mod widget;

#[cfg(target_arch = "arm")]
pub mod device;
