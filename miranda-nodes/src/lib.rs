//! miranda-nodes — Work Order 3: the 52-channel ARKit blendshape SIMD
//! solver, Perlin-noise micro-saccades, breathing oscillators, and
//! blendshape velocity clamping.
//!
//! # Two speech sources, one face
//!
//! Speech-driven mouth shapes come from either of two interchangeable
//! sources, chosen per pipeline:
//!
//! - **Pipeline 1 (cloud)**: [`viseme`] — Amazon Polly viseme events
//!   mapped through an interpolation table.
//! - **Pipeline 2 (local)**: the SIMD acoustic-energy solver (WO-3 T7),
//!   computing weights straight from PCM features.
//!
//! Everything downstream of that choice is shared: the autonomic layer
//! (blink, gaze, breath) and the compositor/damper run identically
//! regardless of which source produced the speech weights. That's what
//! makes the speech source a swappable role slot rather than a fork in
//! the whole face pipeline.
//!
//! # The autonomic layer is always running
//!
//! Per the Instant Presence Standard's No-Loop Video Protocol
//! (`eve-ecc-docs/INSTANT-PRESENCE-STANDARD.md`), the oscillators run
//! continuously whether or not anyone is speaking. "Idle" and "alive" are
//! the same code path here — there is deliberately no separate idle mode
//! that a canned loop could ever be substituted into.

pub mod blink;
pub mod breath;
pub mod compositor;
pub mod conversation;
pub mod dispatcher;
pub mod forge;
pub mod gaze;
pub mod memory;
pub mod solver;
pub mod verify;
pub mod viseme;
