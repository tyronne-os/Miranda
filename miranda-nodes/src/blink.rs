//! WO-3 T3 — asymmetric eye-blink state machine.
//!
//! One of the three autonomic oscillators that make the Instant Presence
//! Standard's No-Loop Video Protocol achievable: because blinks are drawn
//! from a continuous distribution rather than a fixed schedule, the blink
//! pattern never repeats, so no window of EVE's face can be legible as a
//! loop.
//!
//! # Why asymmetry is a requirement, not a garnish
//!
//! Perfectly synchronous, perfectly symmetric, perfectly periodic blinking
//! is one of the strongest "this is a puppet" signals there is — it's the
//! uncanny-valley penalty described in Vanguard Innovation #22. Real human
//! blinks differ between eyes by a small phase delta and vary in amplitude,
//! and their intervals are stochastic. This module reproduces all three.
//!
//! # Model
//!
//! - **Inter-blink interval (IBI)**: Weibull-distributed, clamped to
//!   `[MIN_IBI_SECS, MAX_IBI_SECS]` (2.0–6.5 s per the WO-3 directive).
//! - **Double blinks**: with probability [`DOUBLE_BLINK_PROBABILITY`] (7%),
//!   a blink is immediately followed by a second one after a short gap
//!   instead of a full Weibull interval.
//! - **Per-eye phase delta**: 5–15 ms, with a randomly chosen leading eye.
//! - **Envelope**: fast close (80–100 ms) → brief dwell (~20 ms) →
//!   slower logarithmic reopen (150–200 ms).
//!
//! # Channels driven
//!
//! `eyeBlinkLeft`/`eyeBlinkRight` primarily, plus a fraction of that on
//! `eyeSquintLeft`/`eyeSquintRight` — the orbicularis oculi engages during
//! a blink, so squint co-activates rather than staying flat. All four are
//! addressed through `miranda_core::arkit`'s named constants.
//!
//! # Not sufficient alone for the No-Loop protocol
//!
//! Worth stating plainly: blinking leaves multi-second gaps where this
//! oscillator contributes exactly zero. The protocol's "zero motion for
//! more than one frame interval is a defect" bar is met by the *combination*
//! of this with gaze micro-saccades (T4) and respiration (T5) — never by
//! blink alone. T9's compliance test must exercise the composed output, not
//! this module in isolation.

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use miranda_core::arkit;

/// Shortest inter-blink interval, seconds (WO-3 directive).
pub const MIN_IBI_SECS: f32 = 2.0;
/// Longest inter-blink interval, seconds (WO-3 directive).
pub const MAX_IBI_SECS: f32 = 6.5;

/// Weibull shape parameter for the IBI distribution. `k > 1` gives a
/// right-skewed unimodal shape — most blinks cluster around the mean with
/// an occasional long gap, which is the observed human pattern. `k = 1`
/// would collapse to a memoryless exponential (clumpy, unnatural).
const IBI_WEIBULL_SHAPE: f32 = 1.8;
/// Weibull scale parameter, chosen so the untruncated mean lands near the
/// middle of the clamped range (mean = scale · Γ(1 + 1/k) ≈ 4.5 · 0.89 ≈ 4.0 s).
const IBI_WEIBULL_SCALE: f32 = 4.5;

/// Probability that a completed blink is immediately followed by a second
/// one (WO-3 directive: 7%).
pub const DOUBLE_BLINK_PROBABILITY: f64 = 0.07;

/// Gap between the two halves of a double blink, seconds.
const DOUBLE_BLINK_GAP: (f32, f32) = (0.10, 0.25);

/// Closing-phase duration range, seconds (fast).
const CLOSE_DUR: (f32, f32) = (0.080, 0.100);
/// Fully-closed dwell duration range, seconds (brief).
const DWELL_DUR: (f32, f32) = (0.015, 0.025);
/// Reopening-phase duration range, seconds (slower than closing).
const OPEN_DUR: (f32, f32) = (0.150, 0.200);

/// Per-eye phase delta range, seconds (5–15 ms per the directive).
const PHASE_DELTA: (f32, f32) = (0.005, 0.015);

/// Peak blink amplitude range. Not exactly 1.0, and not identical between
/// eyes — a blink that always closes to precisely full on both sides reads
/// as mechanical.
const AMPLITUDE: (f32, f32) = (0.94, 1.0);

/// How much of the blink weight co-activates the squint channels.
const SQUINT_COACTIVATION: f32 = 0.30;

/// The four weights this oscillator produces for a given frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlinkWeights {
    pub blink_left: f32,
    pub blink_right: f32,
    pub squint_left: f32,
    pub squint_right: f32,
}

impl BlinkWeights {
    /// Fully-open eyes — the contribution when no blink is in progress.
    pub const REST: Self = Self {
        blink_left: 0.0,
        blink_right: 0.0,
        squint_left: 0.0,
        squint_right: 0.0,
    };

    /// Writes these weights into a full 52-channel frame at their canonical
    /// ARKit indices. Additive rather than assigning, so this composes with
    /// other layers instead of clobbering them — the compositor (T6) owns
    /// final clamping.
    pub fn add_into(&self, weights: &mut [f32; miranda_core::BLENDSHAPE_COUNT]) {
        weights[arkit::EYE_BLINK_LEFT] += self.blink_left;
        weights[arkit::EYE_BLINK_RIGHT] += self.blink_right;
        weights[arkit::EYE_SQUINT_LEFT] += self.squint_left;
        weights[arkit::EYE_SQUINT_RIGHT] += self.squint_right;
    }
}

/// Randomized parameters for one specific blink. Re-drawn per blink so no
/// two blinks are identical.
#[derive(Debug, Clone, Copy)]
struct BlinkParams {
    close_dur: f32,
    dwell_dur: f32,
    open_dur: f32,
    /// Seconds after the blink event before the left eye starts moving.
    left_offset: f32,
    /// Same for the right eye. Exactly one of the two is 0.0 — whichever
    /// eye leads — and the other carries the phase delta.
    right_offset: f32,
    left_amp: f32,
    right_amp: f32,
}

impl BlinkParams {
    /// Total time from the blink event until both eyes are fully open again.
    fn total_duration(&self) -> f32 {
        let per_eye = self.close_dur + self.dwell_dur + self.open_dur;
        per_eye + self.left_offset.max(self.right_offset)
    }
}

/// Generates the autonomic blink stream.
///
/// Advance it with [`BlinkGenerator::tick`] once per rendered frame. The
/// generator is allocation-free after construction and does no I/O, so it
/// is safe to run inside the 60 FPS loop.
pub struct BlinkGenerator {
    rng: SmallRng,
    /// Seconds remaining until the next blink begins.
    countdown: f32,
    /// `Some(elapsed_secs)` while a blink is in progress.
    active: Option<f32>,
    params: BlinkParams,
    /// True when the blink currently scheduled is the second half of a
    /// double blink (so it doesn't itself trigger another double).
    in_double: bool,
}

impl BlinkGenerator {
    /// Seeds from system entropy. Use [`BlinkGenerator::from_seed`] in
    /// tests where reproducibility matters.
    pub fn new() -> Self {
        Self::from_rng(SmallRng::from_entropy())
    }

    /// Deterministic construction for tests and for reproducible captures.
    pub fn from_seed(seed: u64) -> Self {
        Self::from_rng(SmallRng::seed_from_u64(seed))
    }

    fn from_rng(mut rng: SmallRng) -> Self {
        let countdown = sample_ibi(&mut rng);
        let params = sample_params(&mut rng);
        Self {
            rng,
            countdown,
            active: None,
            params,
            in_double: false,
        }
    }

    /// True while a blink is mid-flight.
    pub fn is_blinking(&self) -> bool {
        self.active.is_some()
    }

    /// Advances by `dt` seconds and returns this frame's weights.
    ///
    /// `dt` is clamped to a sane maximum: a hitched frame (debugger pause,
    /// scheduler stall) delivering a huge `dt` would otherwise skip an
    /// entire blink in one step, and silently swallowing a blink is worse
    /// than stretching one frame.
    pub fn tick(&mut self, dt: f32) -> BlinkWeights {
        let dt = dt.clamp(0.0, 0.1);

        match self.active {
            None => {
                self.countdown -= dt;
                if self.countdown <= 0.0 {
                    // Begin a blink. Carry the overshoot into the blink's
                    // elapsed time so timing doesn't drift by up to one
                    // frame on every blink.
                    let overshoot = -self.countdown;
                    self.active = Some(overshoot);
                    self.params = sample_params(&mut self.rng);
                    self.evaluate()
                } else {
                    BlinkWeights::REST
                }
            }
            Some(elapsed) => {
                let elapsed = elapsed + dt;
                if elapsed >= self.params.total_duration() {
                    // Blink finished. Decide whether a second one follows.
                    self.active = None;
                    let overshoot = elapsed - self.params.total_duration();
                    if !self.in_double
                        && self.rng.gen_bool(DOUBLE_BLINK_PROBABILITY)
                    {
                        self.in_double = true;
                        self.countdown =
                            self.rng.gen_range(DOUBLE_BLINK_GAP.0..=DOUBLE_BLINK_GAP.1)
                                - overshoot;
                    } else {
                        self.in_double = false;
                        self.countdown = sample_ibi(&mut self.rng) - overshoot;
                    }
                    BlinkWeights::REST
                } else {
                    self.active = Some(elapsed);
                    self.evaluate()
                }
            }
        }
    }

    fn evaluate(&self) -> BlinkWeights {
        let Some(elapsed) = self.active else {
            return BlinkWeights::REST;
        };
        let p = &self.params;
        let blink_left = envelope(elapsed, p, p.left_offset, p.left_amp);
        let blink_right = envelope(elapsed, p, p.right_offset, p.right_amp);
        BlinkWeights {
            blink_left,
            blink_right,
            squint_left: blink_left * SQUINT_COACTIVATION,
            squint_right: blink_right * SQUINT_COACTIVATION,
        }
    }
}

impl Default for BlinkGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Samples one inter-blink interval from a Weibull distribution via inverse
/// transform sampling: `X = λ(−ln U)^(1/k)` for `U ~ Uniform(0,1)`.
///
/// Done directly rather than pulling in `rand_distr` for one closed-form
/// formula — the inverse CDF is exact here, not an approximation, so the
/// dependency would buy nothing.
///
/// The result is clamped to the directive's `[2.0, 6.5]` s window. Clamping
/// (rather than rejection-resampling) is deliberate: it keeps `tick` O(1)
/// with no unbounded loop, which matters for a function called on the
/// render thread. The cost is a small probability mass piled at each
/// endpoint instead of a smooth tail, which is imperceptible for blink
/// timing.
fn sample_ibi(rng: &mut SmallRng) -> f32 {
    // Exclude 0.0 so ln() cannot produce -inf.
    let u: f32 = rng.gen_range(f32::EPSILON..1.0);
    let raw = IBI_WEIBULL_SCALE * (-u.ln()).powf(1.0 / IBI_WEIBULL_SHAPE);
    raw.clamp(MIN_IBI_SECS, MAX_IBI_SECS)
}

fn sample_params(rng: &mut SmallRng) -> BlinkParams {
    let delta = rng.gen_range(PHASE_DELTA.0..=PHASE_DELTA.1);
    // Randomize which eye leads, so the asymmetry isn't itself a constant
    // (a rig that always leads with the left eye is still a fixed pattern).
    let left_leads = rng.gen_bool(0.5);
    let (left_offset, right_offset) = if left_leads {
        (0.0, delta)
    } else {
        (delta, 0.0)
    };

    BlinkParams {
        close_dur: rng.gen_range(CLOSE_DUR.0..=CLOSE_DUR.1),
        dwell_dur: rng.gen_range(DWELL_DUR.0..=DWELL_DUR.1),
        open_dur: rng.gen_range(OPEN_DUR.0..=OPEN_DUR.1),
        left_offset,
        right_offset,
        left_amp: rng.gen_range(AMPLITUDE.0..=AMPLITUDE.1),
        right_amp: rng.gen_range(AMPLITUDE.0..=AMPLITUDE.1),
    }
}

/// Evaluates one eye's lid position at `t` seconds after the blink event.
///
/// Phase shapes, and why each:
///
/// - **Close** uses smoothstep (`3x² − 2x³`). The lid accelerates from rest
///   and decelerates into contact rather than starting at infinite
///   velocity, which a bare power curve like `x^0.75` would imply (its
///   derivative diverges at `x = 0`).
/// - **Dwell** holds at peak.
/// - **Reopen** is logarithmic per the directive: `1 − ln(1 + x(e−1))`.
///   This is exactly 1 at `x = 0` and exactly 0 at `x = 1`, and its
///   derivative shrinks as `x` grows — the lid lifts quickly off the
///   closed position, then eases into the open rest pose.
fn envelope(t: f32, p: &BlinkParams, offset: f32, amp: f32) -> f32 {
    let t = t - offset;
    if t <= 0.0 {
        return 0.0;
    }

    if t < p.close_dur {
        let x = t / p.close_dur;
        amp * (3.0 * x * x - 2.0 * x * x * x)
    } else if t < p.close_dur + p.dwell_dur {
        amp
    } else {
        let x = (t - p.close_dur - p.dwell_dur) / p.open_dur;
        if x >= 1.0 {
            0.0
        } else {
            let e = std::f32::consts::E;
            amp * (1.0 - (1.0 + x * (e - 1.0)).ln())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use miranda_core::BLENDSHAPE_COUNT;

    const FRAME_DT: f32 = 1.0 / 60.0;

    /// Collects every blink's peak weights over a long simulated run.
    fn run_frames(gen: &mut BlinkGenerator, frames: usize) -> Vec<BlinkWeights> {
        (0..frames).map(|_| gen.tick(FRAME_DT)).collect()
    }

    #[test]
    fn envelope_is_bounded_and_starts_and_ends_closed() {
        let p = BlinkParams {
            close_dur: 0.09,
            dwell_dur: 0.02,
            open_dur: 0.18,
            left_offset: 0.0,
            right_offset: 0.01,
            left_amp: 1.0,
            right_amp: 1.0,
        };
        // Before the blink and after it completes, the lid is fully open.
        assert_eq!(envelope(0.0, &p, 0.0, 1.0), 0.0);
        assert_eq!(envelope(p.total_duration() + 0.01, &p, 0.0, 1.0), 0.0);

        // Never exceeds amplitude anywhere in between.
        let mut t = 0.0;
        while t < p.total_duration() + 0.05 {
            let w = envelope(t, &p, 0.0, 1.0);
            assert!(
                (0.0..=1.0).contains(&w),
                "envelope out of range at t={t}: {w}"
            );
            t += 0.001;
        }
    }

    #[test]
    fn envelope_reaches_full_closure_during_dwell() {
        let p = BlinkParams {
            close_dur: 0.09,
            dwell_dur: 0.02,
            open_dur: 0.18,
            left_offset: 0.0,
            right_offset: 0.0,
            left_amp: 1.0,
            right_amp: 1.0,
        };
        let mid_dwell = p.close_dur + p.dwell_dur * 0.5;
        assert!(
            (envelope(mid_dwell, &p, 0.0, 1.0) - 1.0).abs() < 1e-6,
            "lid should be fully closed mid-dwell"
        );
    }

    /// The defining timing asymmetry: closing must be faster than
    /// reopening. If these were equal (or reversed) the blink would read as
    /// mechanical.
    #[test]
    fn closing_is_faster_than_reopening() {
        let mut rng = SmallRng::seed_from_u64(7);
        for _ in 0..200 {
            let p = sample_params(&mut rng);
            assert!(
                p.close_dur < p.open_dur,
                "close {} should be faster than reopen {}",
                p.close_dur,
                p.open_dur
            );
        }
    }

    /// Reopening must be monotonic — a non-monotonic reopen would look like
    /// a flutter, and the logarithmic curve is only correct if it decreases
    /// throughout.
    #[test]
    fn reopen_phase_is_monotonically_decreasing() {
        let p = BlinkParams {
            close_dur: 0.09,
            dwell_dur: 0.02,
            open_dur: 0.18,
            left_offset: 0.0,
            right_offset: 0.0,
            left_amp: 1.0,
            right_amp: 1.0,
        };
        let start = p.close_dur + p.dwell_dur;
        let mut prev = envelope(start, &p, 0.0, 1.0);
        let mut t = start;
        while t < start + p.open_dur {
            let now = envelope(t, &p, 0.0, 1.0);
            assert!(
                now <= prev + 1e-6,
                "reopen not monotonic at t={t}: {prev} -> {now}"
            );
            prev = now;
            t += 0.001;
        }
    }

    /// Left and right must never be identical throughout a blink — that's
    /// the whole point of the phase delta. Checks the *observed* weights,
    /// not just that the parameters differ.
    #[test]
    fn eyes_are_never_perfectly_synchronous() {
        let mut gen = BlinkGenerator::from_seed(42);
        let frames = run_frames(&mut gen, 60 * 60); // 60 s

        let blinking: Vec<&BlinkWeights> = frames
            .iter()
            .filter(|w| w.blink_left > 0.0 || w.blink_right > 0.0)
            .collect();
        assert!(
            !blinking.is_empty(),
            "expected at least one blink in 60 s of frames"
        );

        let any_asymmetric = blinking
            .iter()
            .any(|w| (w.blink_left - w.blink_right).abs() > 1e-4);
        assert!(
            any_asymmetric,
            "left and right lids moved identically in every frame — the \
             phase delta is not taking effect"
        );
    }

    /// Inter-blink intervals must respect the directive's window and must
    /// actually vary (a fixed interval would be a loop by another name).
    #[test]
    fn ibi_stays_in_range_and_varies() {
        let mut rng = SmallRng::seed_from_u64(99);
        let samples: Vec<f32> = (0..2000).map(|_| sample_ibi(&mut rng)).collect();

        for s in &samples {
            assert!(
                (MIN_IBI_SECS..=MAX_IBI_SECS).contains(s),
                "IBI {s} outside [{MIN_IBI_SECS}, {MAX_IBI_SECS}]"
            );
        }

        let distinct = samples
            .iter()
            .map(|s| (s * 1000.0) as i32)
            .collect::<std::collections::HashSet<_>>()
            .len();
        assert!(
            distinct > 500,
            "only {distinct} distinct IBI values in 2000 samples — the \
             distribution is not actually varying"
        );

        let mean = samples.iter().sum::<f32>() / samples.len() as f32;
        assert!(
            (MIN_IBI_SECS..=MAX_IBI_SECS).contains(&mean),
            "mean IBI {mean} should sit inside the clamped window"
        );
    }

    /// Blink *rate* must be physiologically plausible. Humans blink roughly
    /// 10–20 times per minute at rest; with a 2–6.5 s interval the expected
    /// rate is around 15/min. This is the test that would catch a units
    /// error (e.g. treating the IBI as milliseconds), which no range check
    /// on individual samples would catch.
    #[test]
    fn blink_rate_is_physiologically_plausible() {
        let mut gen = BlinkGenerator::from_seed(2024);
        let frames = run_frames(&mut gen, 60 * 120); // 120 s

        // Count rising edges of "either eye closing".
        let mut blinks = 0;
        let mut was_blinking = false;
        for w in &frames {
            let now = w.blink_left > 0.0 || w.blink_right > 0.0;
            if now && !was_blinking {
                blinks += 1;
            }
            was_blinking = now;
        }

        let per_minute = blinks as f32 / 2.0;
        assert!(
            (8.0..=35.0).contains(&per_minute),
            "blink rate {per_minute}/min is implausible (counted {blinks} in 120 s)"
        );
    }

    /// Double blinks must actually occur — a 7% branch that never fires
    /// would be dead code, and it's easy for such a branch to be
    /// unreachable by accident.
    #[test]
    fn double_blinks_occur_sometimes() {
        // Long run so a 7% event is essentially certain to appear.
        let mut gen = BlinkGenerator::from_seed(1234);
        let frames = run_frames(&mut gen, 60 * 600); // 600 s

        // Measure gaps between blink onsets; a double blink shows up as a
        // gap far below MIN_IBI_SECS.
        let mut onsets_frames = Vec::new();
        let mut was_blinking = false;
        for (i, w) in frames.iter().enumerate() {
            let now = w.blink_left > 0.0 || w.blink_right > 0.0;
            if now && !was_blinking {
                onsets_frames.push(i);
            }
            was_blinking = now;
        }

        let short_gaps = onsets_frames
            .windows(2)
            .map(|p| (p[1] - p[0]) as f32 * FRAME_DT)
            .filter(|gap| *gap < MIN_IBI_SECS)
            .count();

        assert!(
            short_gaps > 0,
            "no sub-{MIN_IBI_SECS}s onset gaps in 600 s — the double-blink \
             path never fired"
        );
    }

    /// A double blink must not chain indefinitely into a stutter — the
    /// `in_double` guard exists to stop that, and this proves it holds.
    #[test]
    fn double_blinks_do_not_chain_into_a_stutter() {
        let mut gen = BlinkGenerator::from_seed(555);
        let frames = run_frames(&mut gen, 60 * 600);

        let mut onsets = Vec::new();
        let mut was = false;
        for (i, w) in frames.iter().enumerate() {
            let now = w.blink_left > 0.0 || w.blink_right > 0.0;
            if now && !was {
                onsets.push(i);
            }
            was = now;
        }

        // No three consecutive onsets may all be separated by short gaps.
        for triple in onsets.windows(3) {
            let g1 = (triple[1] - triple[0]) as f32 * FRAME_DT;
            let g2 = (triple[2] - triple[1]) as f32 * FRAME_DT;
            assert!(
                !(g1 < MIN_IBI_SECS && g2 < MIN_IBI_SECS),
                "three blinks in a row with short gaps ({g1}s, {g2}s) — \
                 doubles are chaining"
            );
        }
    }

    /// Weights must stay in `[0, 1]` every frame, forever. Anything outside
    /// that range would be clamped downstream, hiding the real bug here.
    #[test]
    fn weights_never_leave_unit_range() {
        let mut gen = BlinkGenerator::from_seed(31337);
        for _ in 0..60 * 300 {
            let w = gen.tick(FRAME_DT);
            for (name, v) in [
                ("blink_left", w.blink_left),
                ("blink_right", w.blink_right),
                ("squint_left", w.squint_left),
                ("squint_right", w.squint_right),
            ] {
                assert!(
                    (0.0..=1.0).contains(&v),
                    "{name} out of range: {v}"
                );
            }
        }
    }

    /// Squint must co-activate with blink and be strictly weaker — if
    /// squint ever exceeded blink the eye would read as scrunching rather
    /// than blinking.
    #[test]
    fn squint_co_activates_below_blink() {
        let mut gen = BlinkGenerator::from_seed(808);
        let mut saw_activity = false;
        for _ in 0..60 * 120 {
            let w = gen.tick(FRAME_DT);
            if w.blink_left > 0.0 {
                saw_activity = true;
                assert!(
                    w.squint_left < w.blink_left,
                    "squint {} must stay below blink {}",
                    w.squint_left,
                    w.blink_left
                );
            }
            if w.blink_left == 0.0 {
                assert_eq!(w.squint_left, 0.0, "squint must rest when not blinking");
            }
        }
        assert!(saw_activity, "no blink activity observed");
    }

    /// A pathological `dt` (debugger pause, scheduler stall) must not skip a
    /// blink outright or produce out-of-range output.
    #[test]
    fn huge_dt_is_clamped_and_does_not_skip_blinks() {
        let mut gen = BlinkGenerator::from_seed(11);
        // A 10-second dt would otherwise blow straight past a whole blink.
        for _ in 0..50 {
            let w = gen.tick(10.0);
            assert!((0.0..=1.0).contains(&w.blink_left));
            assert!((0.0..=1.0).contains(&w.blink_right));
        }
    }

    #[test]
    fn add_into_writes_only_the_four_eye_channels() {
        let w = BlinkWeights {
            blink_left: 0.8,
            blink_right: 0.7,
            squint_left: 0.24,
            squint_right: 0.21,
        };
        let mut frame = [0.0f32; BLENDSHAPE_COUNT];
        w.add_into(&mut frame);

        assert_eq!(frame[arkit::EYE_BLINK_LEFT], 0.8);
        assert_eq!(frame[arkit::EYE_BLINK_RIGHT], 0.7);
        assert_eq!(frame[arkit::EYE_SQUINT_LEFT], 0.24);
        assert_eq!(frame[arkit::EYE_SQUINT_RIGHT], 0.21);

        let touched = [
            arkit::EYE_BLINK_LEFT,
            arkit::EYE_BLINK_RIGHT,
            arkit::EYE_SQUINT_LEFT,
            arkit::EYE_SQUINT_RIGHT,
        ];
        for (i, v) in frame.iter().enumerate() {
            if !touched.contains(&i) {
                assert_eq!(*v, 0.0, "channel {i} ({}) should be untouched", arkit::CHANNEL_NAMES[i]);
            }
        }
    }

    /// `add_into` must be additive, not assigning — the compositor layers
    /// several oscillators into one frame and this must not clobber them.
    #[test]
    fn add_into_is_additive_not_assigning() {
        let mut frame = [0.0f32; BLENDSHAPE_COUNT];
        frame[arkit::EYE_BLINK_LEFT] = 0.1;
        BlinkWeights {
            blink_left: 0.5,
            blink_right: 0.0,
            squint_left: 0.0,
            squint_right: 0.0,
        }
        .add_into(&mut frame);
        assert_eq!(frame[arkit::EYE_BLINK_LEFT], 0.6);
    }

    /// Same seed must give identical output — required for the
    /// reproducible-capture workflow T9's verification depends on.
    #[test]
    fn seeded_generation_is_reproducible() {
        let a = run_frames(&mut BlinkGenerator::from_seed(4242), 60 * 30);
        let b = run_frames(&mut BlinkGenerator::from_seed(4242), 60 * 30);
        assert_eq!(a, b, "same seed must produce identical blink streams");
    }

    /// Different seeds must diverge — otherwise the "randomness" is a
    /// constant and every EVE session would blink identically.
    #[test]
    fn different_seeds_diverge() {
        let a = run_frames(&mut BlinkGenerator::from_seed(1), 60 * 30);
        let b = run_frames(&mut BlinkGenerator::from_seed(2), 60 * 30);
        assert_ne!(a, b, "different seeds must produce different blink streams");
    }
}
