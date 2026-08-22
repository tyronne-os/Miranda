//! WO-3 T4 — fixational gaze drift and micro-saccade generator.
//!
//! The second autonomic oscillator. Where blinking is episodic (multi-second
//! gaps of exactly zero contribution), gaze micro-movement is *continuous* —
//! which makes this module the primary guarantor of the Instant Presence
//! Standard's hard requirement that "zero motion for more than one frame
//! interval is a defect."
//!
//! # The physiology being reproduced
//!
//! A fixating human eye is never still. Fixational eye movement has three
//! components; this module models the first two, which are the ones visible
//! at blend-shape resolution:
//!
//! - **Slow drift** — the eye wanders off-target over hundreds of ms.
//! - **Micro-saccades** — small fast corrective jumps, roughly 1–2 per
//!   second.
//! - *(Tremor — ~90 Hz, sub-arcminute. Far below what a 60 FPS blend shape
//!   stream can represent, so deliberately not modelled: at 60 FPS it would
//!   alias into visible noise rather than reproduce anything real.)*
//!
//! Multi-octave Perlin noise gives drift and micro-saccades from one
//! construct: the low octave *is* the drift, and the higher octaves *are*
//! the faster corrective motion superimposed on it.
//!
//! # In/out is mirrored between the eyes — the easy bug to get wrong
//!
//! ARKit's `eyeLookIn`/`eyeLookOut` channels are named relative to the
//! **nose**, not to world space. So a single conjugate gaze shift decomposes
//! *asymmetrically*:
//!
//! - Gaze to the subject's right: the **left** eye rotates *toward* the nose
//!   (`eyeLookInLeft`), the **right** eye rotates *away* from it
//!   (`eyeLookOutRight`).
//! - Gaze to the subject's left: the mirror — `eyeLookOutLeft` and
//!   `eyeLookInRight`.
//!
//! Driving `eyeLookIn` on both eyes simultaneously would converge them
//! (cross-eyed); driving `eyeLookOut` on both would diverge them
//! (wall-eyed). Both compile, both pass any range check, and both are
//! immediately wrong on screen. Vertical is *not* mirrored — up is up for
//! both eyes.

use noise::{NoiseFn, Perlin};

use miranda_core::{arkit, BLENDSHAPE_COUNT};

/// Smallest peak angular deflection, degrees (WO-3 directive: ±1.5°).
pub const MIN_AMPLITUDE_DEG: f32 = 1.5;
/// Largest peak angular deflection, degrees (WO-3 directive: ±3.5°).
pub const MAX_AMPLITUDE_DEG: f32 = 3.5;

/// Eye rotation, in degrees, that a blend shape weight of 1.0 represents.
///
/// **This is a rig calibration assumption, not a measured value.** ARKit
/// does not define `eyeLook*` in absolute angular units — a weight of 1.0
/// means "fully deflected" for whatever rig is consuming it, and different
/// rigs are authored to different maxima. 25° is a common authoring
/// convention for full gaze deflection and is used here so the directive's
/// degree-denominated amplitudes have a concrete meaning.
///
/// Consequence worth being explicit about: if EVE's actual rig turns out to
/// be authored to a different maximum, micro-saccade amplitude will be
/// proportionally off (though still small and still non-repeating). That is
/// a calibration correction against real rendered frames — exactly the kind
/// of claim the project's verification discipline says must be checked
/// against painted pixels rather than asserted here.
pub const FULL_DEFLECTION_DEG: f32 = 25.0;

/// Number of noise octaves summed. 4 spans slow drift through
/// micro-saccade-rate motion without reaching frequencies that would alias
/// at 60 FPS.
const OCTAVES: u32 = 4;
/// Base (lowest-octave) frequency in Hz — the slow drift component.
const BASE_FREQ_HZ: f64 = 0.3;
/// Frequency multiplier per octave.
const LACUNARITY: f64 = 2.0;
/// Amplitude multiplier per octave.
const PERSISTENCE: f64 = 0.5;
/// Frequency of the slow channel that modulates overall amplitude, Hz.
/// Deliberately far below `BASE_FREQ_HZ` so amplitude wanders over tens of
/// seconds rather than fighting the drift it is scaling.
const AMPLITUDE_MOD_FREQ_HZ: f64 = 0.05;

/// The eight gaze weights produced for a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GazeWeights {
    pub look_in_left: f32,
    pub look_out_left: f32,
    pub look_up_left: f32,
    pub look_down_left: f32,
    pub look_in_right: f32,
    pub look_out_right: f32,
    pub look_up_right: f32,
    pub look_down_right: f32,
}

impl GazeWeights {
    /// Eyes centred.
    pub const REST: Self = Self {
        look_in_left: 0.0,
        look_out_left: 0.0,
        look_up_left: 0.0,
        look_down_left: 0.0,
        look_in_right: 0.0,
        look_out_right: 0.0,
        look_up_right: 0.0,
        look_down_right: 0.0,
    };

    /// Decomposes a conjugate gaze direction into ARKit's opposing-pair
    /// channels.
    ///
    /// `yaw` is positive toward the subject's right, `pitch` positive
    /// upward, both normalized to `[-1, 1]` of full deflection. Only one of
    /// each opposing pair is ever non-zero, because a single eye cannot
    /// simultaneously look in *and* out.
    fn from_normalized(yaw: f32, pitch: f32) -> Self {
        let (in_left, out_left, in_right, out_right) = if yaw >= 0.0 {
            // Rightward: left eye toward the nose, right eye away from it.
            (yaw, 0.0, 0.0, yaw)
        } else {
            // Leftward: the mirror.
            (0.0, -yaw, -yaw, 0.0)
        };

        let (up, down) = if pitch >= 0.0 {
            (pitch, 0.0)
        } else {
            (0.0, -pitch)
        };

        Self {
            look_in_left: in_left,
            look_out_left: out_left,
            // Vertical is NOT mirrored — both eyes look up together.
            look_up_left: up,
            look_down_left: down,
            look_in_right: in_right,
            look_out_right: out_right,
            look_up_right: up,
            look_down_right: down,
        }
    }

    /// Adds these weights into a full 52-channel frame at their canonical
    /// indices. Additive so it layers with the other oscillators; the
    /// compositor (T6) owns final clamping.
    pub fn add_into(&self, weights: &mut [f32; BLENDSHAPE_COUNT]) {
        weights[arkit::EYE_LOOK_IN_LEFT] += self.look_in_left;
        weights[arkit::EYE_LOOK_OUT_LEFT] += self.look_out_left;
        weights[arkit::EYE_LOOK_UP_LEFT] += self.look_up_left;
        weights[arkit::EYE_LOOK_DOWN_LEFT] += self.look_down_left;
        weights[arkit::EYE_LOOK_IN_RIGHT] += self.look_in_right;
        weights[arkit::EYE_LOOK_OUT_RIGHT] += self.look_out_right;
        weights[arkit::EYE_LOOK_UP_RIGHT] += self.look_up_right;
        weights[arkit::EYE_LOOK_DOWN_RIGHT] += self.look_down_right;
    }

    /// Largest single weight in this set — used by tests and telemetry to
    /// check the oscillator is actually producing motion.
    pub fn max_component(&self) -> f32 {
        [
            self.look_in_left,
            self.look_out_left,
            self.look_up_left,
            self.look_down_left,
            self.look_in_right,
            self.look_out_right,
            self.look_up_right,
            self.look_down_right,
        ]
        .into_iter()
        .fold(0.0f32, f32::max)
    }
}

/// Continuous fixational gaze generator.
///
/// Advance once per rendered frame with [`GazeGenerator::tick`]. Holds no
/// heap state and performs no I/O after construction, so it is safe inside
/// the 60 FPS loop. It is `Send`, so the dispatcher (T8) can move it onto
/// its own thread as the WO-3 design's thread model calls for.
///
/// # Why this does not spawn its own thread
///
/// `design.md` specifies each oscillator on its own isolated thread. That
/// threading is deliberately implemented in the T8 dispatcher rather than
/// here: an oscillator that spawns its own thread and publishes through
/// shared mutable state cannot be unit-tested deterministically (you can
/// only observe it through a race), and it would force a synchronization
/// design before the compositor that consumes it even exists. Keeping the
/// oscillator a pure `tick(dt) -> weights` function makes its output
/// exactly reproducible for a given seed — which is what T9's No-Loop
/// verification needs — while leaving it trivially movable onto a dedicated
/// thread by whoever owns the frame clock.
pub struct GazeGenerator {
    /// Separate noise fields per axis. Two seeds rather than one field
    /// sampled at two offsets, because offsetting within a single field
    /// leaves the axes subtly correlated — which would make the gaze track
    /// a diagonal line instead of wandering a 2D area.
    yaw_noise: Perlin,
    pitch_noise: Perlin,
    /// Slow field modulating peak amplitude within the directive's range,
    /// so amplitude itself is not a constant.
    amp_noise: Perlin,
    /// Seconds since construction; the noise-field coordinate.
    t: f64,
    /// Last computed angles, in degrees, for diagnostics.
    last_yaw_deg: f32,
    last_pitch_deg: f32,
}

impl GazeGenerator {
    /// Seeds the three noise fields from one master seed. Offsets are
    /// arbitrary but fixed, so a given master seed always reproduces the
    /// same gaze stream.
    pub fn from_seed(seed: u32) -> Self {
        Self {
            yaw_noise: Perlin::new(seed),
            pitch_noise: Perlin::new(seed.wrapping_add(0x9E37_79B9)),
            amp_noise: Perlin::new(seed.wrapping_add(0x85EB_CA6B)),
            t: 0.0,
            last_yaw_deg: 0.0,
            last_pitch_deg: 0.0,
        }
    }

    pub fn new() -> Self {
        // Derive a seed from the clock. Gaze needs unpredictability across
        // sessions, not cryptographic quality.
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        Self::from_seed(seed)
    }

    /// Current angular offsets in degrees, as `(yaw, pitch)`. Exposed so
    /// tests can assert against the directive's degree-denominated bounds
    /// directly rather than inferring them back out of blend shape weights.
    pub fn current_angles_deg(&self) -> (f32, f32) {
        (self.last_yaw_deg, self.last_pitch_deg)
    }

    /// Advances by `dt` seconds and returns this frame's gaze weights.
    pub fn tick(&mut self, dt: f32) -> GazeWeights {
        // Clamp for the same reason the blink generator does: a stalled
        // frame delivering a huge dt would teleport the gaze across the
        // noise field, producing a jump that reads as a flick rather than
        // fixational drift.
        self.t += dt.clamp(0.0, 0.1) as f64;

        // Peak amplitude wanders slowly within the directive's window. The
        // spec says amplitudes are "constrained between ±1.5° and ±3.5°" —
        // i.e. a range, not a single value — so a fixed amplitude would be
        // a narrower reading of it than intended, and would also make the
        // motion's envelope constant.
        let amp_raw = self.amp_noise.get([self.t * AMPLITUDE_MOD_FREQ_HZ, 0.0]) as f32;
        let amp_unit = (amp_raw + 1.0) * 0.5; // [-1,1] -> [0,1]
        let amplitude_deg = MIN_AMPLITUDE_DEG
            + amp_unit.clamp(0.0, 1.0) * (MAX_AMPLITUDE_DEG - MIN_AMPLITUDE_DEG);

        let yaw_unit = self.fbm(&self.yaw_noise, 0.0);
        let pitch_unit = self.fbm(&self.pitch_noise, 100.0);

        // Scale to degrees, then hard-clamp. The clamp is what actually
        // guarantees the directive's bound: Perlin's practical output range
        // for 2D is narrower than [-1,1] (roughly ±0.7), and normalizing by
        // the theoretical octave-amplitude sum is therefore an estimate
        // rather than a proof. Clamping makes the upper bound certain
        // regardless; the accompanying test then checks we are not
        // systematically *under*-driving, which is the failure the clamp
        // alone would hide.
        let yaw_deg = (yaw_unit * amplitude_deg).clamp(-MAX_AMPLITUDE_DEG, MAX_AMPLITUDE_DEG);
        let pitch_deg = (pitch_unit * amplitude_deg).clamp(-MAX_AMPLITUDE_DEG, MAX_AMPLITUDE_DEG);

        self.last_yaw_deg = yaw_deg;
        self.last_pitch_deg = pitch_deg;

        GazeWeights::from_normalized(
            yaw_deg / FULL_DEFLECTION_DEG,
            pitch_deg / FULL_DEFLECTION_DEG,
        )
    }

    /// Sums `OCTAVES` octaves of Perlin noise, normalized to about
    /// `[-1, 1]`.
    ///
    /// Hand-rolled rather than using `noise::Fbm` so the exact frequency
    /// and amplitude of each octave is explicit here — this module needs to
    /// reason about which octave corresponds to drift versus micro-saccades
    /// in order to justify the parameters, and a builder-configured Fbm
    /// hides that behind defaults.
    fn fbm(&self, field: &Perlin, axis_offset: f64) -> f32 {
        let mut sum = 0.0;
        let mut amplitude = 1.0;
        let mut frequency = BASE_FREQ_HZ;
        let mut norm = 0.0;

        for _ in 0..OCTAVES {
            sum += field.get([self.t * frequency, axis_offset]) * amplitude;
            norm += amplitude;
            amplitude *= PERSISTENCE;
            frequency *= LACUNARITY;
        }

        (sum / norm) as f32
    }
}

impl Default for GazeGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME_DT: f32 = 1.0 / 60.0;

    fn run(gen: &mut GazeGenerator, frames: usize) -> Vec<GazeWeights> {
        (0..frames).map(|_| gen.tick(FRAME_DT)).collect()
    }

    /// The mirroring rule, stated as a test because it is the single
    /// easiest thing to get backwards in this module and the consequence
    /// (cross-eyed or wall-eyed EVE) is invisible to any range check.
    #[test]
    fn rightward_gaze_drives_left_in_and_right_out() {
        let w = GazeWeights::from_normalized(0.5, 0.0);
        assert_eq!(w.look_in_left, 0.5, "left eye should rotate toward the nose");
        assert_eq!(w.look_out_right, 0.5, "right eye should rotate away from the nose");
        assert_eq!(w.look_out_left, 0.0);
        assert_eq!(w.look_in_right, 0.0);
    }

    #[test]
    fn leftward_gaze_is_the_exact_mirror() {
        let w = GazeWeights::from_normalized(-0.5, 0.0);
        assert_eq!(w.look_out_left, 0.5);
        assert_eq!(w.look_in_right, 0.5);
        assert_eq!(w.look_in_left, 0.0);
        assert_eq!(w.look_out_right, 0.0);
    }

    /// Vertical must NOT be mirrored — both eyes look up together. If this
    /// were mirrored like the horizontal axis, the eyes would diverge
    /// vertically, which no real eye pair does.
    #[test]
    fn vertical_gaze_is_not_mirrored() {
        let up = GazeWeights::from_normalized(0.0, 0.8);
        assert_eq!(up.look_up_left, 0.8);
        assert_eq!(up.look_up_right, 0.8, "both eyes must look up together");
        assert_eq!(up.look_down_left, 0.0);
        assert_eq!(up.look_down_right, 0.0);

        let down = GazeWeights::from_normalized(0.0, -0.8);
        assert_eq!(down.look_down_left, 0.8);
        assert_eq!(down.look_down_right, 0.8);
    }

    /// Opposing channels are mutually exclusive: one eye cannot look in and
    /// out at once. If both were ever non-zero the rig would receive
    /// contradictory input and the result would depend on its internal
    /// blending order.
    #[test]
    fn opposing_channels_are_never_both_active() {
        let mut gen = GazeGenerator::from_seed(7);
        for w in run(&mut gen, 60 * 120) {
            assert!(
                w.look_in_left == 0.0 || w.look_out_left == 0.0,
                "left eye driven in ({}) and out ({}) simultaneously",
                w.look_in_left,
                w.look_out_left
            );
            assert!(
                w.look_in_right == 0.0 || w.look_out_right == 0.0,
                "right eye driven in and out simultaneously"
            );
            assert!(
                w.look_up_left == 0.0 || w.look_down_left == 0.0,
                "left eye driven up and down simultaneously"
            );
            assert!(
                w.look_up_right == 0.0 || w.look_down_right == 0.0,
                "right eye driven up and down simultaneously"
            );
        }
    }

    /// Angular output must respect the directive's ±3.5° ceiling at all
    /// times.
    #[test]
    fn angles_never_exceed_directive_maximum() {
        let mut gen = GazeGenerator::from_seed(99);
        for _ in 0..60 * 300 {
            gen.tick(FRAME_DT);
            let (yaw, pitch) = gen.current_angles_deg();
            assert!(
                yaw.abs() <= MAX_AMPLITUDE_DEG + 1e-4,
                "yaw {yaw}° exceeds ±{MAX_AMPLITUDE_DEG}°"
            );
            assert!(
                pitch.abs() <= MAX_AMPLITUDE_DEG + 1e-4,
                "pitch {pitch}° exceeds ±{MAX_AMPLITUDE_DEG}°"
            );
        }
    }

    /// The counterpart to the ceiling test: motion must actually reach a
    /// meaningful fraction of the allowed amplitude. A clamp guarantees the
    /// upper bound, but a normalization error could leave the gaze drifting
    /// by a hundredth of a degree — technically "in range" and visually
    /// dead. This is the test that catches that.
    #[test]
    fn motion_actually_reaches_meaningful_amplitude() {
        let mut gen = GazeGenerator::from_seed(2024);
        let mut peak_yaw = 0.0f32;
        let mut peak_pitch = 0.0f32;
        for _ in 0..60 * 300 {
            gen.tick(FRAME_DT);
            let (yaw, pitch) = gen.current_angles_deg();
            peak_yaw = peak_yaw.max(yaw.abs());
            peak_pitch = peak_pitch.max(pitch.abs());
        }
        // Over 5 minutes the gaze should at least exceed the minimum
        // specified amplitude on both axes, or the oscillator is
        // under-driving.
        assert!(
            peak_yaw >= MIN_AMPLITUDE_DEG,
            "peak yaw {peak_yaw}° never reached the minimum specified \
             amplitude {MIN_AMPLITUDE_DEG}° — gaze is under-driven"
        );
        assert!(
            peak_pitch >= MIN_AMPLITUDE_DEG,
            "peak pitch {peak_pitch}° never reached {MIN_AMPLITUDE_DEG}°"
        );
    }

    /// The No-Loop Video Protocol's core requirement, tested at this
    /// module's level: gaze must never be perfectly static for more than one
    /// frame interval. This is the property that makes gaze the continuous
    /// backbone of the autonomic layer while blink is episodic.
    #[test]
    fn gaze_is_never_static_across_consecutive_frames() {
        let mut gen = GazeGenerator::from_seed(31337);
        let frames = run(&mut gen, 60 * 180);

        let mut identical_pairs = 0;
        for pair in frames.windows(2) {
            if pair[0] == pair[1] {
                identical_pairs += 1;
            }
        }
        assert_eq!(
            identical_pairs, 0,
            "found {identical_pairs} consecutive identical gaze frames — \
             violates the No-Loop Video Protocol's one-frame-interval rule"
        );
    }

    /// Frame-to-frame change must stay small. Fixational drift that jumped
    /// a large fraction of full deflection between frames would read as a
    /// darting eye, not fixation — and would be the kind of motion the T6
    /// velocity clamp exists to catch, so it should not be produced here in
    /// the first place.
    #[test]
    fn per_frame_change_is_small_enough_to_read_as_fixation() {
        let mut gen = GazeGenerator::from_seed(4242);
        let mut prev = gen.current_angles_deg();
        for _ in 0..60 * 180 {
            gen.tick(FRAME_DT);
            let now = gen.current_angles_deg();
            let d_yaw = (now.0 - prev.0).abs();
            let d_pitch = (now.1 - prev.1).abs();
            // A full traverse of the allowed range in one frame would be
            // 7°; anything approaching that is a flick, not drift.
            assert!(
                d_yaw < 1.0,
                "yaw jumped {d_yaw}° in one frame — too fast for fixation"
            );
            assert!(d_pitch < 1.0, "pitch jumped {d_pitch}° in one frame");
            prev = now;
        }
    }

    /// Blend shape weights must stay within `[0, 1]`. Since the gaze
    /// amplitude is a small fraction of full deflection, they should in fact
    /// stay well below 1.0 — a value near 1.0 would mean the
    /// degrees-to-weight conversion is off by an order of magnitude.
    #[test]
    fn weights_stay_in_range_and_remain_small() {
        let mut gen = GazeGenerator::from_seed(555);
        let expected_ceiling = MAX_AMPLITUDE_DEG / FULL_DEFLECTION_DEG;
        for w in run(&mut gen, 60 * 180) {
            let m = w.max_component();
            assert!((0.0..=1.0).contains(&m), "weight {m} out of unit range");
            assert!(
                m <= expected_ceiling + 1e-4,
                "weight {m} exceeds the ceiling implied by \
                 {MAX_AMPLITUDE_DEG}°/{FULL_DEFLECTION_DEG}° = \
                 {expected_ceiling} — the angle-to-weight conversion looks wrong"
            );
        }
    }

    /// Yaw and pitch must be genuinely independent. If they were correlated
    /// the gaze would slide along a diagonal instead of wandering a 2D
    /// area — which is exactly what sampling one noise field at two offsets
    /// would produce, and why this module uses two separately-seeded fields.
    #[test]
    fn yaw_and_pitch_are_uncorrelated() {
        let mut gen = GazeGenerator::from_seed(808);
        let mut ys = Vec::new();
        let mut ps = Vec::new();
        for _ in 0..60 * 300 {
            gen.tick(FRAME_DT);
            let (y, p) = gen.current_angles_deg();
            ys.push(y);
            ps.push(p);
        }

        let n = ys.len() as f32;
        let my = ys.iter().sum::<f32>() / n;
        let mp = ps.iter().sum::<f32>() / n;
        let cov: f32 = ys
            .iter()
            .zip(&ps)
            .map(|(y, p)| (y - my) * (p - mp))
            .sum::<f32>()
            / n;
        let sy = (ys.iter().map(|y| (y - my).powi(2)).sum::<f32>() / n).sqrt();
        let sp = (ps.iter().map(|p| (p - mp).powi(2)).sum::<f32>() / n).sqrt();
        let corr = cov / (sy * sp);

        assert!(
            corr.abs() < 0.5,
            "yaw/pitch correlation {corr} is too high — the axes are not \
             independent, so gaze will track a line rather than an area"
        );
    }

    /// Gaze must explore both directions on both axes over time, not park
    /// on one side. A persistent bias would read as a fixed squint or a
    /// sideways stare.
    #[test]
    fn gaze_explores_all_four_directions() {
        let mut gen = GazeGenerator::from_seed(1234);
        let mut saw = (false, false, false, false); // in_l, out_l, up, down
        for w in run(&mut gen, 60 * 300) {
            if w.look_in_left > 0.0 {
                saw.0 = true;
            }
            if w.look_out_left > 0.0 {
                saw.1 = true;
            }
            if w.look_up_left > 0.0 {
                saw.2 = true;
            }
            if w.look_down_left > 0.0 {
                saw.3 = true;
            }
        }
        assert!(saw.0 && saw.1, "gaze never explored both horizontal directions");
        assert!(saw.2 && saw.3, "gaze never explored both vertical directions");
    }

    #[test]
    fn add_into_writes_only_the_eight_gaze_channels() {
        let w = GazeWeights::from_normalized(0.1, -0.05);
        let mut frame = [0.0f32; BLENDSHAPE_COUNT];
        w.add_into(&mut frame);

        let gaze_channels = [
            arkit::EYE_LOOK_IN_LEFT,
            arkit::EYE_LOOK_OUT_LEFT,
            arkit::EYE_LOOK_UP_LEFT,
            arkit::EYE_LOOK_DOWN_LEFT,
            arkit::EYE_LOOK_IN_RIGHT,
            arkit::EYE_LOOK_OUT_RIGHT,
            arkit::EYE_LOOK_UP_RIGHT,
            arkit::EYE_LOOK_DOWN_RIGHT,
        ];
        for (i, v) in frame.iter().enumerate() {
            if !gaze_channels.contains(&i) {
                assert_eq!(
                    *v, 0.0,
                    "channel {i} ({}) should be untouched by gaze",
                    arkit::CHANNEL_NAMES[i]
                );
            }
        }
    }

    /// Gaze must not write to blink or squint channels — those belong to
    /// the blink oscillator. Overlapping ownership would make motion
    /// impossible to attribute when debugging.
    #[test]
    fn gaze_never_touches_blink_channels() {
        let w = GazeWeights::from_normalized(0.2, 0.2);
        let mut frame = [0.0f32; BLENDSHAPE_COUNT];
        w.add_into(&mut frame);
        assert_eq!(frame[arkit::EYE_BLINK_LEFT], 0.0);
        assert_eq!(frame[arkit::EYE_BLINK_RIGHT], 0.0);
        assert_eq!(frame[arkit::EYE_SQUINT_LEFT], 0.0);
        assert_eq!(frame[arkit::EYE_SQUINT_RIGHT], 0.0);
    }

    #[test]
    fn add_into_is_additive_not_assigning() {
        let mut frame = [0.0f32; BLENDSHAPE_COUNT];
        frame[arkit::EYE_LOOK_UP_LEFT] = 0.05;
        GazeWeights::from_normalized(0.0, 0.1).add_into(&mut frame);
        assert!((frame[arkit::EYE_LOOK_UP_LEFT] - 0.15).abs() < 1e-6);
    }

    #[test]
    fn seeded_generation_is_reproducible() {
        let a = run(&mut GazeGenerator::from_seed(77), 60 * 30);
        let b = run(&mut GazeGenerator::from_seed(77), 60 * 30);
        assert_eq!(a, b, "same seed must reproduce the same gaze stream");
    }

    #[test]
    fn different_seeds_diverge() {
        let a = run(&mut GazeGenerator::from_seed(1), 60 * 30);
        let b = run(&mut GazeGenerator::from_seed(2), 60 * 30);
        assert_ne!(a, b, "different seeds must produce different gaze streams");
    }

    /// A pathological `dt` must not teleport the gaze across the noise
    /// field, which would produce a visible flick.
    #[test]
    fn huge_dt_is_clamped() {
        let mut gen = GazeGenerator::from_seed(11);
        gen.tick(FRAME_DT);
        let before = gen.current_angles_deg();
        gen.tick(100.0);
        let after = gen.current_angles_deg();
        // With dt clamped to 0.1 s the noise coordinate advances by at most
        // 0.1, which at the base frequency cannot swing the full range.
        assert!(
            (after.0 - before.0).abs() <= 2.0 * MAX_AMPLITUDE_DEG,
            "gaze teleported from {before:?} to {after:?} on a huge dt"
        );
    }

    /// The generator must be `Send` so the T8 dispatcher can move it onto a
    /// dedicated thread, as the WO-3 thread model requires.
    #[test]
    fn generator_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<GazeGenerator>();
    }
}
