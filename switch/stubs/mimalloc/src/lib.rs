//! mimalloc, under its own name. On the Nintendo Switch it runs on primitives of its own
//! (third_party/libmimalloc-sys: src/prim/horizon), which take its arenas from newlib's
//! heap; newlib's malloc alone, behind one lock, made loading many times slower
//! (docs/switch.md). Kept as a facade so the Switch can fall back to newlib's heap.
#![no_std]

pub use real_mimalloc::*;
