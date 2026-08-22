//! miranda-transport — Work Order 4: full-duplex transport layer.
//!
//! Two logical servers share one [`axum`] application and one Tokio runtime:
//!
//! 1. **Data-plane hub** (`/data` WebSocket endpoint): broadcasts binary
//!    [`BlendshapeFrame`] + [`KinematicTransformFrame`] packets to every
//!    connected browser at 60 FPS. This is the DataChannel-equivalent path —
//!    binary framing, minimal per-frame overhead, no JSON on the hot path.
//!
//! 2. **Control-plane telemetry** (`/telemetry` WebSocket endpoint): pushes
//!    JSON metric snapshots (frame build time, dispatcher stats,
//!    circuit-breaker state) to connected management tools at ~2 Hz.
//!
//! # Wire formats
//!
//! ## Data plane — binary packet
//!
//! Each data-plane message is a length-prefixed concatenation of two frames:
//!
//! ```text
//! offset  0 : magic    : [u8; 4]   = b"MRD1"   (version tag)
//! offset  4 : blend_sz : u16 LE    = 216        (sizeof BlendshapeFrame)
//! offset  6 : kin_sz   : u16 LE    = 88         (sizeof KinematicTransformFrame)
//! offset  8 : blend    : [u8; 216] (BlendshapeFrame, repr(C), Pod)
//! offset 224: kin      : [u8; 88]  (KinematicTransformFrame, repr(C), Pod)
//! ─────────────────────────────────────────────────────────────────────────
//! total : 312 bytes
//! ```
//!
//! The magic bytes and size fields allow a receiver to detect protocol
//! mismatches immediately rather than silently misinterpreting bytes.
//! The sizes are little-endian so they round-trip through JavaScript's
//! `DataView.getUint16(offset, true)` without byte-swapping.
//!
//! ## Control plane — JSON telemetry snapshot
//!
//! ```json
//! {
//!   "t_us": 1234567890,
//!   "frames_published": 1801,
//!   "frames_dropped": 0,
//!   "late_frames": 0,
//!   "publish_failures": 0,
//!   "mean_build_us": 11.95,
//!   "max_build_us": 1102.3,
//!   "audio_chunks_consumed": 360,
//!   "data_subscribers": 2,
//!   "telemetry_subscribers": 1,
//!   "circuit_breaker": "closed"
//! }
//! ```
//!
//! # webrtc-rs and the `webrtc-native` feature
//!
//! The `webrtc-native` Cargo feature gates the full `webrtc-rs` crate, which
//! provides DTLS/ICE/SRTP for production NAT traversal. It is not compiled by
//! default because the C++ TLS dependency chain takes several minutes to build
//! on the development machine (Celeron N4500) and because there is no STUN/
//! TURN server available in the offline environment.
//!
//! From a browser's perspective the two paths are **wire-compatible**: both
//! send the same 312-byte binary packet per frame. The difference is the
//! transport layer underneath — plain WebSocket vs. DTLS DataChannel. The
//! browser-side JavaScript only changes the connection constructor, not the
//! frame parser.
//!
//! When AWS connectivity is restored and `--features webrtc-native` is
//! activated, the `DataChannelHub` type alias in this module switches to
//! the real WebRTC signaling path automatically.

pub mod frame;
pub mod hub;
pub mod server;
pub mod telemetry;
pub mod wiring;

pub use hub::DataChannelHub;
pub use server::{ServerConfig, TransportServer};
pub use telemetry::TelemetryHub;
pub use wiring::StatsSource;
