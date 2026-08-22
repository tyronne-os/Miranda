//! WO-3 T5 — respiratory prior modulator.
//!
//! The third autonomic oscillator. Breathing is the slowest and most
//! constant of the three: blinking is episodic, gaze drifts continuously at
//! a few Hz, and respiration provides a steady sub-Hz baseline that is
//! *never* zero. Together they satisfy the Instant Presence Standard's
//! requirement that no frame interval passes without motion.
//!
//! # ARKit-52 has no neck, clavicle, or head channels — and that matters here
//!
//! The WO-3 directive asks this oscillator to drive "head/clavicle pitch
//! offsets" alongside the jaw. It cannot do so through
//! [`miranda_core::BlendshapeFrame`], because **ARKit's 52 blend shapes are
//! face-only**. The full set is 14 eye + 4 jaw + 23 mouth + 5 brow + 3 cheek
//! + 2 nose + 1 tongue — there is no `neckPitch`, no `clavicleRise`, no head
//! rotation. In ARKit itself, head pose is carried by a separate transform
//! (`ARFaceAnchor.transform`), not by a blend shape weight.
//!
//! Rather than silently drop the requirement or abuse an unrelated face
//! channel to smuggle it through, this module:
//!
//! 1. Drives the parts that genuinely are ARKit-52 channels — `jawOpen`
//!    baseline and its `mouthClose` counterbalance — via
//!    [`BreathWeights::add_into`].
//! 2. Computes `head_pitch_deg` and `clavicle_rise` as real values on
//!    [`BreathWeights`], so the information exists and is correct.
//! 3. Leaves those two **unrouted**, because no transport for them exists
//!    yet. Carrying them needs either a body-rig frame type alongside
//!    `BlendshapeFrame` or an extra transform channel in the WO-4/WO-5
//!    transport. That is a real, deliberate gap, recorded here rather than
//!    papered over.
//!
//! # Waveform: a true sine, and why the asymmetry is skipped
//!
//! Real respiration is not sinusoidal — inhalation is shorter than
//! exhalation, roughly a 1:1.5 to 1:2 I:E ratio at rest. The directive
//! specifies a sine, and a sine is what this implements. That is a defensible
//! match rather than a shortcut: the jaw contribution peaks at 0.04 (4% of
//! full jaw travel), and the difference between a sine and an I:E-asymmetric
//! curve at that amplitude is far below perceptual threshold. Modelling the
//! asymmetry would add a parameter nobody can verify by looking at EVE, which
//! is the kind of unfalsifiable complexity this project's verification
//! discipline exists to avoid. If chest motion is ever routed and rendered at
//! a visible amplitude, revisit it *then*, against real frames.

use noise::{NoiseFn, Perlin};

use miranda_core::{arkit, BLENDSHAPE_COUNT};

/// Slowest resting respiratory rate, Hz (12 breaths/min).
pub const MIN_RATE_HZ: f32 = 0.2;
/// Fastest resting respiratory rate, Hz (20 breaths/min).
pub const MAX_RATE_HZ: f32 = 0.333;

/// Peak `jawOpen` contribution (WO-3 directive: baseline 0.0–0.04).
///
/// The jaw contribution is strictly non-negative: it oscillates within
/// `[0.0, JAW_OPEN_MAX]`, not `±JAW_OPEN_MAX`. A negative jaw weight is
/// meaningless — the jaw cannot open by a negative amount — and would be
/// clamped away downstream, quietly halving the intended motion.
pub const JAW_OPEN_MAX: f32 = 0.04;

/// How strongly `mouthClose` counterbalances the breathing jaw drop.
///
/// At rest the lips stay together while the jaw makes small respiratory
/// excursions, so `mouthClose` tracks `jawOpen` to hold the seal. Kept below
/// 1.0 so the lips are held but not pressed — a full counterbalance would
/// read as a deliberate clamp of the mouth rather than passive rest.
const MOUTH_CLOSE_COUNTERBALANCE: f32 = 0.6;

/// Peak head pitch excursion, degrees. Small — resting breathing nods the
/// head by well under a degree.
const HEAD_PITCH_MAX_DEG: f32 = 0.6;

/// Frequency of the slow field that drifts the respiratory rate, Hz.
/// Far below `MIN_RATE_HZ` so the rate wanders across many breaths rather
/// than varying within a single one.
const RATE_DRIFT_FREQ_HZ: f64 = 0.02;

/// Frequency of the slow field that drifts breath depth, Hz.
const DEPTH_DRIFT_FREQ_HZ: f64 = 0.017;

/// Minimum fraction of full depth a breath can be scaled to, so depth
/// variation never flattens respiration into stillness (which would break
/// the No-Loop guarantee this oscillator underwrites).
const MIN_DEPTH_SCALE: f32 = 0.55;

/// One frame of respiratory contribution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreathWeights {
    /// ARKit `jawOpen` contribution, in `[0.0, JAW_OPEN_MAX]`.
    pub jaw_open: f32,
    /// ARKit `mouthClose` counterbalance holding the lip seal.
    pub mouth_close: f32,
    /// Head pitch in degrees. **Not an ARKit-52 channel** — computed and
    /// exposed but not routed; see the module docs.
    pub head_pitch_deg: f32,
    /// Normalized clavicle/chest rise in `[0.0, 1.0]`. **Not an ARKit-52
    /// channel** — same caveat as `head_pitch_deg`.
    pub clavicle_rise: f32,
}

impl BreathWeights {
    /// Adds the ARKit-representable portion into a 52-channel frame.
    ///
    /// Only `jawOpen` and `mouthClose` are written. `head_pitch_deg` and
    /// `clavicle_rise` are deliberately *not* written anywhere: there is no
    /// correct ARKit-52 channel for them, and choosing an incorrect one to
    /// avoid "losing" them would produce a wrong face rather than a missing
    /// nod.
    pub fn add_into(&self, weights: &mut [f32; BLENDSHAPE_COUNT]) {
        weights[arkit::JAW_OPEN] += self.jaw_open;
        weights[arkit::MOUTH_CLOSE] += self.mouth_close;
    }
}

/// Continuous respiratory oscillator.
///
/// Advance once per rendered frame with [`BreathGenerator::tick`]. Holds no
/// heap state and does no I/O after construction, so it is safe in the 60 FPS
/// loop, and it is `Send` so T8's dispatcher can move it to its own thread.
pub struct BreathGenerator {
    /// Accumulated phase in radians. **Integrated, never recomputed** — see
    /// `tick` for why this matters.
    phase: f32,
    /// Slowly-varying respiratory rate field.
    rate_noise: Perlin,
    /// Slowly-varying breath depth field.
    depth_noise: Perlin,
    /// Seconds since construction, for sampling the slow drift fields.
    t: f64,
    /// Last computed rate, exposed for diagnostics and tests.
    last_rate_hz: f32,
}

impl BreathGenerator {
    pub fn from_seed(seed: u32) -> Self {
        Self {
            phase: 0.0,
            rate_noise: Perlin::new(seed),
            depth_noise: Perlin::new(seed.wrapping_add(0xC2B2_AE35)),
            t: 0.0,
            last_rate_hz: (MIN_RATE_HZ + MAX_RATE_HZ) * 0.5,
        }
    }

    pub fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        Self::from_seed(seed)
    }

    /// Current respiratory rate in Hz.
    pub fn rate_hz(&self) -> f32 {
        self.last_rate_hz
    }

    /// Current respiratory rate in breaths per minute — the unit the
    /// physiological range is normally quoted in, so tests and telemetry can
    /// check it directly without re-deriving the conversion.
    pub fn breaths_per_minute(&self) -> f32 {
        self.last_rate_hz * 60.0
    }

    /// Current phase in radians, wrapped to `[0, 2π)`.
    pub fn phase(&self) -> f32 {
        self.phase
    }

    /// Advances by `dt` seconds and returns this frame's contribution.
    pub fn tick(&mut self, dt: f32) -> BreathWeights {
        let dt = dt.clamp(0.0, 0.1);
        self.t += dt as f64;

        // Drift the rate within the physiological window.
        let rate_raw = self.rate_noise.get([self.t * RATE_DRIFT_FREQ_HZ, 0.0]) as f32;
        let rate_unit = ((rate_raw + 1.0) * 0.5).clamp(0.0, 1.0);
        let rate_hz = MIN_RATE_HZ + rate_unit * (MAX_RATE_HZ - MIN_RATE_HZ);
        self.last_rate_hz = rate_hz;

        // Integrate the phase rather than computing `2π·f·t` directly.
        //
        // This is the load-bearing detail of a frequency-modulated
        // oscillator. With `phase = 2π·f·t`, changing `f` retroactively
        // rescales *all* elapsed time, so every rate adjustment teleports
        // the phase — producing a visible hitch in the breath at exactly the
        // moments the rate drifts. Integrating `dφ = 2π·f·dt` keeps the
        // waveform continuous across rate changes, which is the only way a
        // drifting rate stays imperceptible.
        self.phase += std::f32::consts::TAU * rate_hz * dt;
        if self.phase >= std::f32::consts::TAU {
            self.phase -= std::f32::consts::TAU;
        }

        // Drift breath depth too, so successive breaths differ in size. Held
        // above MIN_DEPTH_SCALE so depth variation can never flatten the
        // oscillator to stillness.
        let depth_raw = self.depth_noise.get([self.t * DEPTH_DRIFT_FREQ_HZ, 0.0]) as f32;
        let depth_unit = ((depth_raw + 1.0) * 0.5).clamp(0.0, 1.0);
        let depth = MIN_DEPTH_SCALE + depth_unit * (1.0 - MIN_DEPTH_SCALE);

        // Map the sine into [0, 1] before scaling: the jaw opens by a
        // non-negative amount only.
        let cycle = (self.phase.sin() + 1.0) * 0.5;

        let jaw_open = cycle * JAW_OPEN_MAX * depth;
        let mouth_close = jaw_open * MOUTH_CLOSE_COUNTERBALANCE;

        // Head pitch is signed — the head nods slightly up and down through
        // the cycle rather than only dropping — so it uses the raw sine.
        let head_pitch_deg = self.phase.sin() * HEAD_PITCH_MAX_DEG * depth;

        BreathWeights {
            jaw_open,
            mouth_close,
            head_pitch_deg,
            clavicle_rise: cycle * depth,
        }
    }
}

impl Default for BreathGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME_DT: f32 = 1.0 / 60.0;

    fn run(gen: &mut BreathGenerator, frames: usize) -> Vec<BreathWeights> {
        (0..frames).map(|_| gen.tick(FRAME_DT)).collect()
    }

    /// Rate must stay inside the physiological window at all times.
    #[test]
    fn rate_stays_within_physiological_window() {
        let mut gen = BreathGenerator::from_seed(1);
        for _ in 0..60 * 600 {
            gen.tick(FRAME_DT);
            let hz = gen.rate_hz();
            assert!(
                (MIN_RATE_HZ..=MAX_RATE_HZ).contains(&hz),
                "rate {hz} Hz outside [{MIN_RATE_HZ}, {MAX_RATE_HZ}]"
            );
            let bpm = gen.breaths_per_minute();
            assert!(
                (12.0..=20.0).contains(&bpm),
                "rate {bpm} breaths/min outside the resting 12–20 range"
            );
        }
    }

    /// Counts actual completed breath cycles over a known duration. This is
    /// the test that catches a units error — e.g. treating the rate as
    /// breaths-per-minute where Hz is expected would give a 60× wrong
    /// period while every per-sample range check still passed.
    #[test]
    fn observed_breath_rate_matches_the_configured_rate() {
        let mut gen = BreathGenerator::from_seed(2024);
        let seconds = 300.0;
        let frames = (seconds * 60.0) as usize;

        // Count rising zero-crossings of the sine, i.e. completed cycles.
        let mut cycles = 0;
        let mut prev_phase = gen.phase();
        for _ in 0..frames {
            gen.tick(FRAME_DT);
            let now = gen.phase();
            // Phase wrapped => one cycle completed.
            if now < prev_phase {
                cycles += 1;
            }
            prev_phase = now;
        }

        let observed_bpm = cycles as f32 / (seconds / 60.0);
        assert!(
            (11.0..=21.0).contains(&observed_bpm),
            "observed {observed_bpm} breaths/min over {seconds}s ({cycles} \
             cycles) is outside the plausible band — likely a Hz/bpm units error"
        );
    }

    /// `jawOpen` must stay within `[0, JAW_OPEN_MAX]` — never negative,
    /// never above the directive's ceiling.
    #[test]
    fn jaw_open_respects_directive_bounds() {
        let mut gen = BreathGenerator::from_seed(7);
        for w in run(&mut gen, 60 * 600) {
            assert!(
                (0.0..=JAW_OPEN_MAX + 1e-6).contains(&w.jaw_open),
                "jaw_open {} outside [0, {JAW_OPEN_MAX}]",
                w.jaw_open
            );
        }
    }

    /// `mouthClose` must counterbalance the jaw — present whenever the jaw
    /// is open, and never exceeding it (which would read as clamping the
    /// mouth shut rather than holding a resting seal).
    #[test]
    fn mouth_close_counterbalances_without_exceeding_jaw() {
        let mut gen = BreathGenerator::from_seed(99);
        let mut saw_open = false;
        for w in run(&mut gen, 60 * 300) {
            assert!(
                w.mouth_close <= w.jaw_open + 1e-6,
                "mouth_close {} exceeds jaw_open {}",
                w.mouth_close,
                w.jaw_open
            );
            if w.jaw_open > 1e-4 {
                saw_open = true;
                assert!(
                    w.mouth_close > 0.0,
                    "mouth_close must engage while the jaw is open"
                );
            }
        }
        assert!(saw_open, "jaw never opened over 300s — oscillator is dead");
    }

    /// The phase-integration correctness test.
    ///
    /// If the phase were computed as `2π·f·t` instead of integrated, a drift
    /// in `f` would retroactively rescale all elapsed time and teleport the
    /// phase, producing a jump far larger than one frame of smooth motion
    /// can account for. This bounds the per-frame change against what the
    /// maximum rate physically allows, so a discontinuity fails the test.
    #[test]
    fn phase_is_continuous_under_rate_drift() {
        let mut gen = BreathGenerator::from_seed(31337);

        // Maximum possible |d(jaw_open)/dt| is at the steepest point of the
        // sine: 2π·f·(A/2). Convert to a per-frame bound and allow generous
        // headroom for depth modulation.
        let max_per_frame =
            std::f32::consts::TAU * MAX_RATE_HZ * (JAW_OPEN_MAX / 2.0) * FRAME_DT * 4.0;

        let mut prev = gen.tick(FRAME_DT).jaw_open;
        for frame in 0..60 * 600 {
            let now = gen.tick(FRAME_DT).jaw_open;
            let delta = (now - prev).abs();
            assert!(
                delta <= max_per_frame,
                "frame {frame}: jaw_open jumped {delta} in one frame (bound \
                 {max_per_frame}) — the phase is discontinuous, which means \
                 it is being recomputed from elapsed time rather than integrated"
            );
            prev = now;
        }
    }

    /// Breathing must never be static across consecutive frames. This is the
    /// oscillator that has to hold the No-Loop guarantee during the
    /// multi-second gaps when blinking contributes exactly zero.
    #[test]
    fn breathing_is_never_static_across_consecutive_frames() {
        let mut gen = BreathGenerator::from_seed(4242);
        let frames = run(&mut gen, 60 * 300);
        let identical = frames.windows(2).filter(|p| p[0] == p[1]).count();
        assert_eq!(
            identical, 0,
            "{identical} consecutive identical breath frames — violates the \
             No-Loop Video Protocol's one-frame-interval rule"
        );
    }

    /// Successive breaths must differ in depth. Identical-amplitude breaths
    /// forever would be a loop even with a correct waveform.
    #[test]
    fn breath_depth_varies_between_breaths() {
        let mut gen = BreathGenerator::from_seed(555);
        // Collect the peak jaw_open of each cycle.
        let mut peaks = Vec::new();
        let mut current_peak = 0.0f32;
        let mut prev_phase = gen.phase();
        for _ in 0..60 * 900 {
            let w = gen.tick(FRAME_DT);
            let now = gen.phase();
            if now < prev_phase {
                peaks.push(current_peak);
                current_peak = 0.0;
            }
            current_peak = current_peak.max(w.jaw_open);
            prev_phase = now;
        }

        assert!(peaks.len() > 20, "expected many breaths, got {}", peaks.len());

        // Measure the *spread* of peak depths, not how many distinct
        // quantization buckets they land in.
        //
        // A bucket count is the wrong metric here: depth drifts at
        // DEPTH_DRIFT_FREQ_HZ (~59 s period) against a ~4 s breath period,
        // so ~16 consecutive breaths fall inside one depth cycle and are
        // legitimately near-identical in depth. That is physiologically
        // right — real breathing depth wanders slowly rather than jumping
        // per breath — so a distinctness threshold would fail the
        // implementation for behaving correctly. What actually matters is
        // that depth traverses a real portion of its available range over
        // time.
        let min_peak = peaks.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_peak = peaks.iter().cloned().fold(0.0f32, f32::max);
        let observed_span = max_peak - min_peak;
        let theoretical_span = JAW_OPEN_MAX * (1.0 - MIN_DEPTH_SCALE);

        assert!(
            observed_span > theoretical_span * 0.5,
            "peak depths spanned only {observed_span} of a possible \
             {theoretical_span} across {} breaths (min {min_peak}, max \
             {max_peak}) — depth is not actually varying",
            peaks.len()
        );
    }

    /// Depth variation must never flatten respiration into stillness — the
    /// `MIN_DEPTH_SCALE` floor exists for exactly this.
    #[test]
    fn depth_never_collapses_to_zero() {
        let mut gen = BreathGenerator::from_seed(808);
        let mut peaks = Vec::new();
        let mut current_peak = 0.0f32;
        let mut prev_phase = gen.phase();
        for _ in 0..60 * 900 {
            let w = gen.tick(FRAME_DT);
            let now = gen.phase();
            if now < prev_phase {
                peaks.push(current_peak);
                current_peak = 0.0;
            }
            current_peak = current_peak.max(w.jaw_open);
            prev_phase = now;
        }

        let floor = JAW_OPEN_MAX * MIN_DEPTH_SCALE * 0.9;
        for p in &peaks {
            assert!(
                *p >= floor,
                "a breath peaked at only {p} (floor {floor}) — depth \
                 modulation is flattening respiration toward stillness"
            );
        }
    }

    /// `add_into` must write exactly two channels and leave the other 50
    /// alone — in particular it must not invent a channel for head pitch or
    /// clavicle rise.
    #[test]
    fn add_into_writes_only_jaw_open_and_mouth_close() {
        let w = BreathWeights {
            jaw_open: 0.03,
            mouth_close: 0.018,
            head_pitch_deg: 0.5,
            clavicle_rise: 0.8,
        };
        let mut frame = [0.0f32; BLENDSHAPE_COUNT];
        w.add_into(&mut frame);

        assert_eq!(frame[arkit::JAW_OPEN], 0.03);
        assert_eq!(frame[arkit::MOUTH_CLOSE], 0.018);

        for (i, v) in frame.iter().enumerate() {
            if i != arkit::JAW_OPEN && i != arkit::MOUTH_CLOSE {
                assert_eq!(
                    *v, 0.0,
                    "channel {i} ({}) was written by breath — head pitch and \
                     clavicle rise must NOT be smuggled into a face channel",
                    arkit::CHANNEL_NAMES[i]
                );
            }
        }
    }

    /// Breath must not touch the eye channels the blink and gaze oscillators
    /// own — overlapping ownership makes motion impossible to attribute.
    #[test]
    fn breath_never_touches_eye_channels() {
        let mut frame = [0.0f32; BLENDSHAPE_COUNT];
        BreathWeights {
            jaw_open: 0.04,
            mouth_close: 0.024,
            head_pitch_deg: 0.6,
            clavicle_rise: 1.0,
        }
        .add_into(&mut frame);

        for i in arkit::EYE_BLINK_LEFT..=arkit::EYE_WIDE_RIGHT {
            assert_eq!(
                frame[i], 0.0,
                "breath wrote to eye channel {i} ({})",
                arkit::CHANNEL_NAMES[i]
            );
        }
    }

    #[test]
    fn add_into_is_additive_not_assigning() {
        let mut frame = [0.0f32; BLENDSHAPE_COUNT];
        frame[arkit::JAW_OPEN] = 0.5; // e.g. a speech viseme already applied
        BreathWeights {
            jaw_open: 0.03,
            mouth_close: 0.0,
            head_pitch_deg: 0.0,
            clavicle_rise: 0.0,
        }
        .add_into(&mut frame);
        assert!((frame[arkit::JAW_OPEN] - 0.53).abs() < 1e-6);
    }

    /// Head pitch must be signed (the head nods both ways) while clavicle
    /// rise must not be (the chest rises from rest, it does not invert).
    #[test]
    fn head_pitch_is_signed_and_clavicle_rise_is_not() {
        let mut gen = BreathGenerator::from_seed(1234);
        let mut saw_positive_pitch = false;
        let mut saw_negative_pitch = false;
        for w in run(&mut gen, 60 * 300) {
            if w.head_pitch_deg > 0.01 {
                saw_positive_pitch = true;
            }
            if w.head_pitch_deg < -0.01 {
                saw_negative_pitch = true;
            }
            assert!(
                (0.0..=1.0).contains(&w.clavicle_rise),
                "clavicle_rise {} outside [0,1]",
                w.clavicle_rise
            );
            assert!(
                w.head_pitch_deg.abs() <= HEAD_PITCH_MAX_DEG + 1e-6,
                "head pitch {} exceeds ±{HEAD_PITCH_MAX_DEG}°",
                w.head_pitch_deg
            );
        }
        assert!(
            saw_positive_pitch && saw_negative_pitch,
            "head pitch should nod both up and down across the breath cycle"
        );
    }

    #[test]
    fn seeded_generation_is_reproducible() {
        let a = run(&mut BreathGenerator::from_seed(77), 60 * 60);
        let b = run(&mut BreathGenerator::from_seed(77), 60 * 60);
        assert_eq!(a, b, "same seed must reproduce the same breath stream");
    }

    #[test]
    fn different_seeds_diverge() {
        let a = run(&mut BreathGenerator::from_seed(1), 60 * 60);
        let b = run(&mut BreathGenerator::from_seed(2), 60 * 60);
        assert_ne!(a, b, "different seeds must produce different breath streams");
    }

    /// A pathological `dt` must not skip most of a breath cycle or produce
    /// out-of-range output.
    #[test]
    fn huge_dt_is_clamped() {
        let mut gen = BreathGenerator::from_seed(11);
        for _ in 0..100 {
            let w = gen.tick(50.0);
            assert!((0.0..=JAW_OPEN_MAX + 1e-6).contains(&w.jaw_open));
            assert!((0.0..=1.0).contains(&w.clavicle_rise));
        }
    }

    #[test]
    fn generator_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<BreathGenerator>();
    }
}
