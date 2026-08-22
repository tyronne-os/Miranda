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

/// Canonical ARKit-52 blend shape channel indices — **the single source of
/// truth for this project.**
///
/// # Why this module exists
///
/// Before it, three mutually incompatible index schemes were circulating in
/// this repo's own documentation:
///
/// - Apple's canonical `ARFaceAnchor.BlendShapeLocation` serialization
///   order (`jawOpen` = 17, `mouthClose` = 18, `eyeBlinkRight` = 7)
/// - `.kiro/steering/pipeline-1-aws-native.md`'s viseme table
///   (`jawOpen` = #0, `mouthClose` = #19, `mouthFunnel` = #22,
///   `tongueOut` = #51)
/// - WO-3's own task directive (`eyeBlinkLeft` = 0, `eyeBlinkRight` = 1,
///   `eyeSquintLeft` = 18)
///
/// Those cannot all be correct, and a wrong blend shape index is the
/// nastiest class of bug in this subsystem: it compiles, it passes any
/// test that only checks value ranges, and it fails *only* as a wrong
/// muscle moving on EVE's face. Exactly the silent-failure profile the
/// project's CAT-5 tier exists to guard against.
///
/// This module adopts **Apple's canonical ordering** (the order
/// `ARFaceAnchor.blendShapes` is conventionally serialized in, and the
/// order every ARKit-compatible rig and exporter expects). The other two
/// schemes are superseded. Every table, oscillator, and solver in this
/// project must reference channels through these constants **by name** —
/// never by a bare integer literal — so that a mapping mistake becomes a
/// compile error or an obviously-wrong name at the call site rather than
/// an invisible off-by-N.
pub mod arkit {
    // ---- Left eye (0-6) ----
    pub const EYE_BLINK_LEFT: usize = 0;
    pub const EYE_LOOK_DOWN_LEFT: usize = 1;
    pub const EYE_LOOK_IN_LEFT: usize = 2;
    pub const EYE_LOOK_OUT_LEFT: usize = 3;
    pub const EYE_LOOK_UP_LEFT: usize = 4;
    pub const EYE_SQUINT_LEFT: usize = 5;
    pub const EYE_WIDE_LEFT: usize = 6;

    // ---- Right eye (7-13) ----
    pub const EYE_BLINK_RIGHT: usize = 7;
    pub const EYE_LOOK_DOWN_RIGHT: usize = 8;
    pub const EYE_LOOK_IN_RIGHT: usize = 9;
    pub const EYE_LOOK_OUT_RIGHT: usize = 10;
    pub const EYE_LOOK_UP_RIGHT: usize = 11;
    pub const EYE_SQUINT_RIGHT: usize = 12;
    pub const EYE_WIDE_RIGHT: usize = 13;

    // ---- Jaw (14-17) ----
    pub const JAW_FORWARD: usize = 14;
    pub const JAW_LEFT: usize = 15;
    pub const JAW_RIGHT: usize = 16;
    pub const JAW_OPEN: usize = 17;

    // ---- Mouth (18-40) ----
    pub const MOUTH_CLOSE: usize = 18;
    pub const MOUTH_FUNNEL: usize = 19;
    pub const MOUTH_PUCKER: usize = 20;
    pub const MOUTH_LEFT: usize = 21;
    pub const MOUTH_RIGHT: usize = 22;
    pub const MOUTH_SMILE_LEFT: usize = 23;
    pub const MOUTH_SMILE_RIGHT: usize = 24;
    pub const MOUTH_FROWN_LEFT: usize = 25;
    pub const MOUTH_FROWN_RIGHT: usize = 26;
    pub const MOUTH_DIMPLE_LEFT: usize = 27;
    pub const MOUTH_DIMPLE_RIGHT: usize = 28;
    pub const MOUTH_STRETCH_LEFT: usize = 29;
    pub const MOUTH_STRETCH_RIGHT: usize = 30;
    pub const MOUTH_ROLL_LOWER: usize = 31;
    pub const MOUTH_ROLL_UPPER: usize = 32;
    pub const MOUTH_SHRUG_LOWER: usize = 33;
    pub const MOUTH_SHRUG_UPPER: usize = 34;
    pub const MOUTH_PRESS_LEFT: usize = 35;
    pub const MOUTH_PRESS_RIGHT: usize = 36;
    pub const MOUTH_LOWER_DOWN_LEFT: usize = 37;
    pub const MOUTH_LOWER_DOWN_RIGHT: usize = 38;
    pub const MOUTH_UPPER_UP_LEFT: usize = 39;
    pub const MOUTH_UPPER_UP_RIGHT: usize = 40;

    // ---- Brows (41-45) ----
    pub const BROW_DOWN_LEFT: usize = 41;
    pub const BROW_DOWN_RIGHT: usize = 42;
    pub const BROW_INNER_UP: usize = 43;
    pub const BROW_OUTER_UP_LEFT: usize = 44;
    pub const BROW_OUTER_UP_RIGHT: usize = 45;

    // ---- Cheeks (46-48) ----
    pub const CHEEK_PUFF: usize = 46;
    pub const CHEEK_SQUINT_LEFT: usize = 47;
    pub const CHEEK_SQUINT_RIGHT: usize = 48;

    // ---- Nose (49-50) ----
    pub const NOSE_SNEER_LEFT: usize = 49;
    pub const NOSE_SNEER_RIGHT: usize = 50;

    // ---- Tongue (51) ----
    pub const TONGUE_OUT: usize = 51;

    /// All 52 channel names in canonical index order. Index `i` of this
    /// array is the name of channel `i` — used for diagnostics, telemetry
    /// labels, and the tests that prove this module is internally
    /// consistent.
    pub const CHANNEL_NAMES: [&str; super::BLENDSHAPE_COUNT] = [
        "eyeBlinkLeft",
        "eyeLookDownLeft",
        "eyeLookInLeft",
        "eyeLookOutLeft",
        "eyeLookUpLeft",
        "eyeSquintLeft",
        "eyeWideLeft",
        "eyeBlinkRight",
        "eyeLookDownRight",
        "eyeLookInRight",
        "eyeLookOutRight",
        "eyeLookUpRight",
        "eyeSquintRight",
        "eyeWideRight",
        "jawForward",
        "jawLeft",
        "jawRight",
        "jawOpen",
        "mouthClose",
        "mouthFunnel",
        "mouthPucker",
        "mouthLeft",
        "mouthRight",
        "mouthSmileLeft",
        "mouthSmileRight",
        "mouthFrownLeft",
        "mouthFrownRight",
        "mouthDimpleLeft",
        "mouthDimpleRight",
        "mouthStretchLeft",
        "mouthStretchRight",
        "mouthRollLower",
        "mouthRollUpper",
        "mouthShrugLower",
        "mouthShrugUpper",
        "mouthPressLeft",
        "mouthPressRight",
        "mouthLowerDownLeft",
        "mouthLowerDownRight",
        "mouthUpperUpLeft",
        "mouthUpperUpRight",
        "browDownLeft",
        "browDownRight",
        "browInnerUp",
        "browOuterUpLeft",
        "browOuterUpRight",
        "cheekPuff",
        "cheekSquintLeft",
        "cheekSquintRight",
        "noseSneerLeft",
        "noseSneerRight",
        "tongueOut",
    ];

    /// Looks up a channel index by its ARKit name. Returns `None` for an
    /// unknown name rather than panicking, so a caller parsing an external
    /// rig manifest can report a clear error instead of aborting.
    pub fn index_of(name: &str) -> Option<usize> {
        CHANNEL_NAMES.iter().position(|&n| n == name)
    }
}

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

    /// The `arkit` constants and `CHANNEL_NAMES` are two parallel
    /// representations of the same mapping, so they can drift apart
    /// silently if edited independently. This test makes that drift a
    /// build failure: every named constant must point at the array slot
    /// bearing its own camelCase name.
    #[test]
    fn arkit_constants_agree_with_channel_names() {
        use arkit::*;
        let pairs: &[(usize, &str)] = &[
            (EYE_BLINK_LEFT, "eyeBlinkLeft"),
            (EYE_LOOK_DOWN_LEFT, "eyeLookDownLeft"),
            (EYE_LOOK_IN_LEFT, "eyeLookInLeft"),
            (EYE_LOOK_OUT_LEFT, "eyeLookOutLeft"),
            (EYE_LOOK_UP_LEFT, "eyeLookUpLeft"),
            (EYE_SQUINT_LEFT, "eyeSquintLeft"),
            (EYE_WIDE_LEFT, "eyeWideLeft"),
            (EYE_BLINK_RIGHT, "eyeBlinkRight"),
            (EYE_LOOK_DOWN_RIGHT, "eyeLookDownRight"),
            (EYE_LOOK_IN_RIGHT, "eyeLookInRight"),
            (EYE_LOOK_OUT_RIGHT, "eyeLookOutRight"),
            (EYE_LOOK_UP_RIGHT, "eyeLookUpRight"),
            (EYE_SQUINT_RIGHT, "eyeSquintRight"),
            (EYE_WIDE_RIGHT, "eyeWideRight"),
            (JAW_FORWARD, "jawForward"),
            (JAW_LEFT, "jawLeft"),
            (JAW_RIGHT, "jawRight"),
            (JAW_OPEN, "jawOpen"),
            (MOUTH_CLOSE, "mouthClose"),
            (MOUTH_FUNNEL, "mouthFunnel"),
            (MOUTH_PUCKER, "mouthPucker"),
            (MOUTH_LEFT, "mouthLeft"),
            (MOUTH_RIGHT, "mouthRight"),
            (MOUTH_SMILE_LEFT, "mouthSmileLeft"),
            (MOUTH_SMILE_RIGHT, "mouthSmileRight"),
            (MOUTH_FROWN_LEFT, "mouthFrownLeft"),
            (MOUTH_FROWN_RIGHT, "mouthFrownRight"),
            (MOUTH_DIMPLE_LEFT, "mouthDimpleLeft"),
            (MOUTH_DIMPLE_RIGHT, "mouthDimpleRight"),
            (MOUTH_STRETCH_LEFT, "mouthStretchLeft"),
            (MOUTH_STRETCH_RIGHT, "mouthStretchRight"),
            (MOUTH_ROLL_LOWER, "mouthRollLower"),
            (MOUTH_ROLL_UPPER, "mouthRollUpper"),
            (MOUTH_SHRUG_LOWER, "mouthShrugLower"),
            (MOUTH_SHRUG_UPPER, "mouthShrugUpper"),
            (MOUTH_PRESS_LEFT, "mouthPressLeft"),
            (MOUTH_PRESS_RIGHT, "mouthPressRight"),
            (MOUTH_LOWER_DOWN_LEFT, "mouthLowerDownLeft"),
            (MOUTH_LOWER_DOWN_RIGHT, "mouthLowerDownRight"),
            (MOUTH_UPPER_UP_LEFT, "mouthUpperUpLeft"),
            (MOUTH_UPPER_UP_RIGHT, "mouthUpperUpRight"),
            (BROW_DOWN_LEFT, "browDownLeft"),
            (BROW_DOWN_RIGHT, "browDownRight"),
            (BROW_INNER_UP, "browInnerUp"),
            (BROW_OUTER_UP_LEFT, "browOuterUpLeft"),
            (BROW_OUTER_UP_RIGHT, "browOuterUpRight"),
            (CHEEK_PUFF, "cheekPuff"),
            (CHEEK_SQUINT_LEFT, "cheekSquintLeft"),
            (CHEEK_SQUINT_RIGHT, "cheekSquintRight"),
            (NOSE_SNEER_LEFT, "noseSneerLeft"),
            (NOSE_SNEER_RIGHT, "noseSneerRight"),
            (TONGUE_OUT, "tongueOut"),
        ];

        assert_eq!(
            pairs.len(),
            BLENDSHAPE_COUNT,
            "every one of the 52 channels must be covered by this test"
        );

        for &(idx, name) in pairs {
            assert_eq!(
                arkit::CHANNEL_NAMES[idx], name,
                "constant for {name} points at index {idx}, which is named \
                 '{}' — the constants and CHANNEL_NAMES have drifted apart",
                arkit::CHANNEL_NAMES[idx]
            );
            assert_eq!(
                arkit::index_of(name),
                Some(idx),
                "index_of({name}) disagrees with its named constant"
            );
        }
    }

    /// No duplicate indices and full coverage of 0..52 — a copy-paste slip
    /// that gave two channels the same index would otherwise silently mean
    /// one muscle is unreachable and another is double-driven.
    #[test]
    fn arkit_indices_are_a_complete_bijection() {
        let mut seen = [false; BLENDSHAPE_COUNT];
        for (i, name) in arkit::CHANNEL_NAMES.iter().enumerate() {
            assert!(!seen[i], "index {i} listed twice");
            seen[i] = true;
            assert!(!name.is_empty(), "channel {i} has an empty name");
            // Names must be unique too, else index_of() would be ambiguous.
            let first = arkit::CHANNEL_NAMES.iter().position(|n| n == name).unwrap();
            assert_eq!(first, i, "duplicate channel name {name:?}");
        }
        assert!(seen.iter().all(|&s| s), "not all 52 indices are covered");
    }

    #[test]
    fn arkit_index_of_rejects_unknown_names() {
        assert_eq!(arkit::index_of("notARealChannel"), None);
        assert_eq!(arkit::index_of(""), None);
        // Case matters — ARKit names are camelCase and a case-folded match
        // would let a subtly-wrong manifest through.
        assert_eq!(arkit::index_of("jawopen"), None);
    }
}
