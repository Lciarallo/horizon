//! The Nintendo Switch homebrew platform layer (libnx). Empty on other targets.
//!
//! `shim/` holds the C side, built by `build.rs` with devkitA64: the startup hook
//! (`userAppInit`) that makes Rust's thread-locals work, the POSIX calls newlib lacks,
//! and wrappers around libnx's inline pad and applet functions.

#![cfg_attr(not(all(target_os = "horizon", target_arch = "aarch64")), allow(unused))]

#[cfg(all(target_os = "horizon", target_arch = "aarch64"))]
mod imp;
#[cfg(all(target_os = "horizon", target_arch = "aarch64"))]
pub use imp::*;
