//! WO-2 — ASR (speech-to-text) implementations for Pipeline 2.
//!
//! Pipeline 1's ASR lives outside this crate entirely (browser-side
//! capture into `client-services/ace-controller`, transcribed by a hosted
//! API). This module is the local/native path: parakeet.cpp via FFI, no
//! network call, no cloud dependency.
//!
//! `parakeet_ffi` is gated on the `parakeet_available` cfg, which
//! `build.rs` sets only when it actually finds this project's prebuilt
//! parakeet.cpp library. That keeps `cargo build` green on machines
//! without it instead of hard-failing the whole workspace over a
//! Pipeline-2-only dependency — see `build.rs` for the full rationale.

#[cfg(parakeet_available)]
pub mod parakeet_ffi;
