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

/// Identifies which of the four rings on the bus a `BackpressureError`
/// came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingId {
    /// The `AudioChunk` ring.
    Audio,
    /// The `BlendshapeFrame` ring.
    Blendshape,
    /// The `SphericalHarmonics` ring.
    SphericalHarmonics,
    /// The `KinematicTransformFrame` ring (added WO-4).
    Kinematic,
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

// ── KinematicTransformFrame ──────────────────────────────────────────────────

/// Number of skeletal joints carried in [`KinematicTransformFrame`].
///
/// HEAD, NECK, SHOULDER_LEFT, SHOULDER_RIGHT — the four joints the
/// respiratory oscillator (WO-3 T5) and future full-body kinematics need
/// to route. ARKit-52 is face-only and structurally cannot carry these, so
/// they travel on a parallel frame type rather than being smuggled into an
/// unrelated face channel.
pub const KINEMATIC_JOINT_COUNT: usize = 4;

/// Index constants for the joints in [`KinematicTransformFrame`].
///
/// Matches the order the renderer (WO-5) expects: head before neck so the
/// dependency chain is top-down.
pub mod kinematic_joints {
    pub const HEAD: usize = 0;
    pub const NECK: usize = 1;
    pub const SHOULDER_LEFT: usize = 2;
    pub const SHOULDER_RIGHT: usize = 3;

    /// Human-readable names in index order, for telemetry and debug.
    pub const JOINT_NAMES: [&str; super::KINEMATIC_JOINT_COUNT] =
        ["head", "neck", "shoulder_left", "shoulder_right"];
}

/// A unit quaternion representing a joint's local rotation, stored as
/// `[x, y, z, w]` in the standard conventions used by every game-engine
/// and animation tool this project targets (GLM, Three.js, Unity, Omniverse).
///
/// - Identity: `[0.0, 0.0, 0.0, 1.0]`.
/// - `w = 1.0` → no rotation.  `w = 0.0, xyz = axis` → 180° rotation.
/// - **Must be unit-length.** Consumers normalize defensively, but a
///   non-unit quaternion is a producer bug — it will produce scale artefacts
///   on the rendered mesh.
///
/// Size: 4 × f32 = 16 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Quaternion {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quaternion {
    /// The identity rotation — no change from the bind pose.
    pub const IDENTITY: Self = Self { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };

    /// Builds a quaternion from a rotation around the X axis (pitch), in
    /// radians. Positive angle tilts the joint forward (head nods down).
    #[inline]
    pub fn from_angle_x(radians: f32) -> Self {
        let (s, c) = (radians * 0.5).sin_cos();
        Self { x: s, y: 0.0, z: 0.0, w: c }
    }

    /// Builds a quaternion from a rotation around the Y axis (yaw), in
    /// radians.
    #[inline]
    pub fn from_angle_y(radians: f32) -> Self {
        let (s, c) = (radians * 0.5).sin_cos();
        Self { x: 0.0, y: s, z: 0.0, w: c }
    }

    /// Builds a quaternion from a rotation around the Z axis (roll), in
    /// radians.
    #[inline]
    pub fn from_angle_z(radians: f32) -> Self {
        let (s, c) = (radians * 0.5).sin_cos();
        Self { x: 0.0, y: 0.0, z: s, w: c }
    }

    /// Squared magnitude — should be 1.0 for a valid rotation quaternion.
    #[inline]
    pub fn norm_sq(self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }

    /// Normalises to unit length. Returns `IDENTITY` if the quaternion is
    /// degenerate (zero or near-zero magnitude) rather than propagating NaN.
    #[inline]
    pub fn normalised(self) -> Self {
        let n = self.norm_sq().sqrt();
        if n < f32::EPSILON {
            return Self::IDENTITY;
        }
        let inv = 1.0 / n;
        Self {
            x: self.x * inv,
            y: self.y * inv,
            z: self.z * inv,
            w: self.w * inv,
        }
    }

    /// Returns `true` if this quaternion is within `tolerance` of unit
    /// length. Useful in tests and assertions.
    #[inline]
    pub fn is_unit(&self, tolerance: f32) -> bool {
        (self.norm_sq() - 1.0).abs() < tolerance
    }
}

impl Default for Quaternion {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Body-rig kinematic transform frame — joint quaternions for the skeletal
/// joints that ARKit-52's face-only blendshape frame cannot carry.
///
/// This is the companion to [`BlendshapeFrame`], travelling in lockstep
/// through the transport layer. Together they carry the complete per-frame
/// face + body state that EVE's renderer needs.
///
/// # Layout (WO-4 defined — do not add fields without updating the IPC ring)
///
/// ```text
/// offset  0 : timestamp_us : u64           →   8 bytes
/// offset  8 : joints       : [Quaternion; 4]  → 64 bytes  (4 × 16)
/// offset 72 : head_pitch_deg : f32          →   4 bytes
/// offset 76 : clavicle_rise  : f32          →   4 bytes
/// offset 80 : _reserved    : [u8; 8]        →   8 bytes  (explicit pad)
/// ─────────────────────────────────────────────────────
/// total                                        88 bytes
/// ```
///
/// The `_reserved` field is zero on write and must be ignored on read.
/// It provides expansion room (e.g. for a spine joint, shoulder elevation
/// scalar, or protocol version byte) without breaking the mmap layout or
/// existing consumers.
///
/// # The WO-3 gap this closes
///
/// `miranda-nodes/src/breath.rs` computes `head_pitch_deg` (head nodding
/// with the respiratory cycle) and `clavicle_rise` (chest rising on
/// inhalation). Both are physiologically correct values, but `BlendshapeFrame`
/// — ARKit-52 face weights only — has nowhere to put them. Rather than
/// silently smuggling them into an unrelated face channel, WO-3 documented
/// the gap and wrote a test asserting no smuggling occurred. This struct is
/// the promised fix: `BreathGenerator::build_transform_frame()` (WO-3
/// `breath.rs`) can now populate the right fields.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct KinematicTransformFrame {
    /// Microseconds since harness start — matches [`BlendshapeFrame::timestamp_us`]
    /// so the renderer can pair the two frames by timestamp.
    pub timestamp_us: u64,
    /// Local-space rotation quaternions for each joint, in
    /// [`kinematic_joints`] index order. Identity quaternion (`w=1`) means
    /// no deviation from the bind pose.
    pub joints: [Quaternion; KINEMATIC_JOINT_COUNT],
    /// Head nodding amplitude from the respiratory cycle, in degrees.
    /// Positive = forward nod (chin toward chest). Signed — the head nods
    /// slightly backward on the exhale.
    ///
    /// This is a convenience scalar: the full rotation is also encoded in
    /// `joints[HEAD]`, but consumers that only need the scalar can read
    /// this field without decomposing a quaternion.
    pub head_pitch_deg: f32,
    /// Fractional clavicle rise from inhalation, in `[0, 1]`. Zero is the
    /// resting position; 1.0 would be full elevation (never reached in
    /// practice — normal tidal breathing drives this to roughly 0.3).
    ///
    /// Also a convenience scalar mirroring `joints[SHOULDER_LEFT/RIGHT]`'s
    /// vertical component.
    pub clavicle_rise: f32,
    /// Reserved for future fields. Always zero on write.
    pub _reserved: [u8; 8],
}

impl KinematicTransformFrame {
    /// Rest-pose frame — all joints at identity, scalar outputs zero.
    pub const REST: Self = Self {
        timestamp_us: 0,
        joints: [Quaternion::IDENTITY; KINEMATIC_JOINT_COUNT],
        head_pitch_deg: 0.0,
        clavicle_rise: 0.0,
        _reserved: [0; 8],
    };

    /// Builds a frame from the outputs of `BreathGenerator::tick()`.
    ///
    /// Converts `head_pitch_deg` into a proper X-axis rotation quaternion
    /// for the head joint and a matching smaller rotation for the neck
    /// joint (neck typically participates at ≈40% of head travel). Shoulders
    /// rise symmetrically by `clavicle_rise`.
    pub fn from_breath(
        timestamp_us: u64,
        head_pitch_deg: f32,
        clavicle_rise: f32,
    ) -> Self {
        let head_rad = head_pitch_deg.to_radians();
        // Neck participates at 40% of head pitch — anatomically, head motion
        // distributes across cervical vertebrae, not just the atlanto-occipital
        // joint. A 60/40 split between head and neck joints is the standard
        // authoring convention used by major rigs (Metahuman, UE5 mannequin).
        let neck_rad = head_rad * 0.4;
        // Shoulder elevation: clavicle_rise in [0,1] maps to a Z-axis
        // rotation of up to 4° per shoulder (typical quiet breathing range).
        let shoulder_elevation_rad = clavicle_rise * 4.0f32.to_radians();

        let mut joints = [Quaternion::IDENTITY; KINEMATIC_JOINT_COUNT];
        joints[kinematic_joints::HEAD] = Quaternion::from_angle_x(head_rad).normalised();
        joints[kinematic_joints::NECK] = Quaternion::from_angle_x(neck_rad).normalised();
        // Shoulders: left rotates positive Z, right negative Z (symmetric
        // elevation). The sign convention matches the Y-up, right-hand
        // coordinate system used by Three.js / GLB rigs.
        joints[kinematic_joints::SHOULDER_LEFT] =
            Quaternion::from_angle_z(shoulder_elevation_rad).normalised();
        joints[kinematic_joints::SHOULDER_RIGHT] =
            Quaternion::from_angle_z(-shoulder_elevation_rad).normalised();

        Self {
            timestamp_us,
            joints,
            head_pitch_deg,
            clavicle_rise,
            _reserved: [0; 8],
        }
    }
}

impl Default for KinematicTransformFrame {
    fn default() -> Self {
        Self::REST
    }
}

#[cfg(test)]
mod kinematic_tests {
    use super::*;

    #[test]
    fn struct_size_is_exactly_88_bytes() {
        assert_eq!(std::mem::size_of::<Quaternion>(), 16);
        assert_eq!(
            std::mem::size_of::<KinematicTransformFrame>(),
            88,
            "KinematicTransformFrame layout changed — update the IPC ring \
             capacity and the transport framing"
        );
    }

    #[test]
    fn rest_frame_is_all_identity_and_zero() {
        let f = KinematicTransformFrame::REST;
        assert_eq!(f.timestamp_us, 0);
        assert_eq!(f.head_pitch_deg, 0.0);
        assert_eq!(f.clavicle_rise, 0.0);
        assert_eq!(f._reserved, [0u8; 8]);
        for j in f.joints.iter() {
            assert_eq!(*j, Quaternion::IDENTITY);
        }
    }

    #[test]
    fn identity_quaternion_is_unit_length() {
        assert!(Quaternion::IDENTITY.is_unit(1e-6));
    }

    #[test]
    fn quaternion_from_angle_x_is_unit() {
        for deg in [-90.0f32, -45.0, -1.0, 0.0, 1.0, 15.0, 45.0, 90.0] {
            let q = Quaternion::from_angle_x(deg.to_radians());
            assert!(
                q.is_unit(1e-5),
                "from_angle_x({deg}) norm² = {} (not 1)",
                q.norm_sq()
            );
        }
    }

    #[test]
    fn from_breath_produces_valid_quaternions() {
        let f = KinematicTransformFrame::from_breath(1_000_000, 0.6, 0.3);
        for (i, j) in f.joints.iter().enumerate() {
            assert!(
                j.is_unit(1e-5),
                "joint {} quaternion is not unit: {:?}", i, j
            );
        }
        assert_eq!(f.head_pitch_deg, 0.6);
        assert_eq!(f.clavicle_rise, 0.3);
        assert_eq!(f._reserved, [0u8; 8]);
    }

    #[test]
    fn head_leads_neck() {
        // Head joint must rotate more than neck — neck participates at 40%.
        let f = KinematicTransformFrame::from_breath(0, 5.0, 0.0);
        // The X component of a quaternion is sin(θ/2); larger pitch ⇒ larger |x|.
        let head_x = f.joints[kinematic_joints::HEAD].x.abs();
        let neck_x = f.joints[kinematic_joints::NECK].x.abs();
        assert!(
            head_x > neck_x,
            "head x={head_x} should lead neck x={neck_x}"
        );
        assert!(
            (head_x / neck_x - 1.0 / 0.4).abs() < 0.01,
            "neck should be ~40% of head pitch, got ratio {}",
            head_x / neck_x
        );
    }

    #[test]
    fn shoulders_are_symmetric_and_scale_with_clavicle_rise() {
        let f_half = KinematicTransformFrame::from_breath(0, 0.0, 0.5);
        let f_full = KinematicTransformFrame::from_breath(0, 0.0, 1.0);
        let sl = f_half.joints[kinematic_joints::SHOULDER_LEFT];
        let sr = f_half.joints[kinematic_joints::SHOULDER_RIGHT];
        // Symmetric: Z components should be equal in magnitude, opposite in sign.
        assert!(
            (sl.z + sr.z).abs() < 1e-6,
            "shoulders are not symmetric: L.z={} R.z={}", sl.z, sr.z
        );
        // Scale: full rise should rotate more than half rise.
        let sl_full = f_full.joints[kinematic_joints::SHOULDER_LEFT];
        assert!(
            sl_full.z.abs() > sl.z.abs(),
            "shoulder at rise=1.0 should exceed rise=0.5"
        );
    }

    #[test]
    fn zero_breath_produces_rest_pose_joints() {
        let f = KinematicTransformFrame::from_breath(0, 0.0, 0.0);
        for (i, j) in f.joints.iter().enumerate() {
            assert!(
                j.is_unit(1e-6) && (j.x.abs() < 1e-7) && (j.y.abs() < 1e-7) && (j.z.abs() < 1e-7),
                "joint {i} should be identity at zero breath: {j:?}"
            );
        }
    }

    #[test]
    fn bytemuck_pod_round_trip() {
        // Proves the Pod derive is sound: casting to bytes and back must
        // give an identical value, with no padding bytes that could be
        // non-deterministic.
        let original = KinematicTransformFrame::from_breath(42_000, 0.8, 0.25);
        let bytes: &[u8] = bytemuck::bytes_of(&original);
        assert_eq!(bytes.len(), 88);
        let recovered: KinematicTransformFrame = *bytemuck::from_bytes(bytes);
        // Compare field-by-field since f32 doesn't impl Eq.
        assert_eq!(recovered.timestamp_us, original.timestamp_us);
        assert_eq!(recovered.head_pitch_deg, original.head_pitch_deg);
        assert_eq!(recovered.clavicle_rise, original.clavicle_rise);
        for i in 0..KINEMATIC_JOINT_COUNT {
            assert_eq!(recovered.joints[i], original.joints[i]);
        }
    }

    /// Joint index names must agree with `JOINT_NAMES`.
    #[test]
    fn joint_constants_match_names_array() {
        use kinematic_joints::*;
        assert_eq!(JOINT_NAMES[HEAD], "head");
        assert_eq!(JOINT_NAMES[NECK], "neck");
        assert_eq!(JOINT_NAMES[SHOULDER_LEFT], "shoulder_left");
        assert_eq!(JOINT_NAMES[SHOULDER_RIGHT], "shoulder_right");
    }

    /// Normalise must not panic or produce NaN on a zero quaternion.
    #[test]
    fn normalise_handles_degenerate_input() {
        let q = Quaternion { x: 0.0, y: 0.0, z: 0.0, w: 0.0 };
        let n = q.normalised();
        assert_eq!(n, Quaternion::IDENTITY, "degenerate quaternion must return IDENTITY");
    }
}
