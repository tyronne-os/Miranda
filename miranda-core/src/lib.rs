//! miranda-core — shared types and constants used across every Miranda-Engine
//! crate (blendshape counts, spherical-harmonic coefficient counts, common
//! error types). This crate has no dependencies on any other workspace
//! crate — it is the foundation everything else builds on (see WO-1 design.md).

/// Sample rate for all audio on the Miranda IPC bus. Fixed at 16 kHz mono —
/// the target format for Parakeet/ASR-class models. Do not make this
/// runtime-configurable in v1; it is a pipeline-science constant.
pub const AUDIO_SAMPLE_RATE_HZ: u32 = 16_000;

/// Number of audio frames (samples) per `AudioChunk`. 160 frames at 16 kHz is
/// exactly 10 ms — the standard VAD/ASR streaming chunk size (Parakeet,
/// Whisper, and Amazon Transcribe Streaming all use 10 ms frames).
pub const AUDIO_CHUNK_FRAMES: usize = 160;

/// Number of ARKit standard face blend shape channels. This is the full
/// facial animation rig (eyes, brows, mouth, jaw, cheeks) that drives EVE's
/// live expression. Fixed by the pipeline science — do not parameterize.
pub const BLENDSHAPE_COUNT: usize = 52;

/// Number of L2 spherical harmonic lighting coefficients. Encodes the
/// low-frequency ambient lighting environment used by the Gaussian-splat
/// renderer (WO-5) to relight EVE dynamically.
pub const SH_COEFF_COUNT: usize = 9;

/// A raw audio chunk from the microphone or a VAD pre-buffer.
///
/// `#[repr(C)]` fixes the memory layout to match the C ABI so this struct is
/// identical whether read from Rust, from a future C FFI binding (e.g.
/// parakeet.cpp in WO-2), or from any other language sharing this bus.
///
/// `bytemuck::Pod` and `bytemuck::Zeroable` are compile-time proofs that this
/// struct has no padding, no uninitialized bytes, and no invalid bit
/// patterns — required for safe casting to/from `&[u8]` when writing to and
/// reading from the shared-memory mmap in `miranda-ipc`.
///
/// Total size: 8 (timestamp_us) + 4 (sample_rate) + 4 (frame_count)
/// + 640 (samples: 160 × 4 bytes) = 656 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct AudioChunk {
    /// Microseconds since harness start.
    pub timestamp_us: u64,
    /// Always `AUDIO_SAMPLE_RATE_HZ` in v1.
    pub sample_rate: u32,
    /// Number of valid samples in `samples` (may be less than
    /// `AUDIO_CHUNK_FRAMES` for a partial final chunk).
    pub frame_count: u32,
    /// Raw f32 PCM samples, mono.
    pub samples: [f32; AUDIO_CHUNK_FRAMES],
}

/// The 52 ARKit blend shape weights that drive facial animation on EVE.
/// These are normalized weights in `[0.0, 1.0]`. WO-3 (ARKit-52 SIMD
/// kinematics) is the primary writer under Pipeline 2; the Polly viseme
/// adapter is the primary writer under Pipeline 1. The renderer (WO-5) is
/// the primary reader in both.
///
/// Total size: 8 (timestamp_us) + 208 (weights: 52 × 4 bytes) = 216 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BlendshapeFrame {
    /// Microseconds since harness start.
    pub timestamp_us: u64,
    /// ARKit-52 standard blend shape weights.
    pub weights: [f32; BLENDSHAPE_COUNT],
}

/// L2 spherical harmonic lighting coefficients — 9 floats encoding the
/// low-frequency ambient lighting environment. Used by the Gaussian-splat
/// renderer (WO-5) to relight EVE's render dynamically.
///
/// Total size: 8 (timestamp_us) + 36 (coefficients: 9 × 4 bytes) + 4
/// (explicit tail padding) = 48 bytes.
///
/// Note on the padding field: `design.md` states this struct's size as 44
/// bytes (8 + 36), but that arithmetic does not account for `#[repr(C)]`
/// alignment rules. Because `timestamp_us: u64` forces 8-byte alignment on
/// the whole struct, and a C-ABI struct's total size must be a multiple of
/// its alignment, the compiler would otherwise insert 4 bytes of *hidden*
/// trailing padding to round 44 up to 48 — and `bytemuck::Pod` correctly
/// refuses to derive over hidden padding, since those bytes would be
/// uninitialized and byte-casting over them would be unsound. `_padding` is
/// an explicit, zero-initialized field that accounts for those same 4
/// bytes, making the layout Pod-safe. The real, correct size is 48 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SphericalHarmonics {
    /// Microseconds since harness start.
    pub timestamp_us: u64,
    /// L2 spherical harmonic coefficients.
    pub coefficients: [f32; SH_COEFF_COUNT],
    /// Explicit tail padding to satisfy `#[repr(C)]`'s size-is-multiple-of-
    /// alignment rule without leaving hidden, uninitialized padding bytes.
    /// Always zero; not semantically meaningful.
    pub _padding: [u8; 4],
}

/// Returned when a ring buffer on the Miranda IPC bus is full and a producer
/// attempts to push.
///
/// Per WO-1 REQ-6, the bus must never silently drop a payload and never
/// corrupt existing buffer contents when full — it returns this typed error
/// so the caller decides the policy (drop, retry, or escalate as a real
/// telemetry event that THE VANITY can surface as backpressure on that node).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackpressureError {
    /// Which ring rejected the push.
    pub ring: RingId,
    /// Capacity of that ring in slots, for telemetry/diagnostics.
    pub capacity: usize,
}

/// Identifies which of the three rings on the bus a `BackpressureError`
/// came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingId {
    /// The `AudioChunk` ring.
    Audio,
    /// The `BlendshapeFrame` ring.
    Blendshape,
    /// The `SphericalHarmonics` ring.
    SphericalHarmonics,
}

impl core::fmt::Display for BackpressureError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{:?} ring is full ({} slots); consumer is not draining fast enough",
            self.ring, self.capacity
        )
    }
}

impl std::error::Error for BackpressureError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time-adjacent size check: confirms the struct layouts match
    /// the byte sizes fixed by the pipeline science in design.md. If this
    /// test fails, either a field was added/removed or padding crept in
    /// (which would also fail the `Pod` derive at compile time first).
    ///
    /// `SphericalHarmonics` is 48 bytes, not the 44 stated in design.md's
    /// informal arithmetic (8 + 36) — see the doc comment on that struct
    /// for why `#[repr(C)]` alignment requires 4 explicit padding bytes.
    #[test]
    fn struct_sizes_match_design_spec() {
        assert_eq!(std::mem::size_of::<AudioChunk>(), 656);
        assert_eq!(std::mem::size_of::<BlendshapeFrame>(), 216);
        assert_eq!(std::mem::size_of::<SphericalHarmonics>(), 48);
    }
}
