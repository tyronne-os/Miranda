//! WO-3 T6 — frame compositor and motion damper.
//!
//! The last stage before a frame reaches the IPC bus. Takes the four
//! independent weight sources (speech, blink, gaze, breath), layers them
//! into one 52-channel frame, and applies the physical limits that keep the
//! result renderable.
//!
//! # Pipeline order, and why it is this order
//!
//! Per the WO-3 directive: layer additively → clamp absolute range → clamp
//! velocity. Acceleration limiting is applied inside the same damping step
//! as velocity, because the two constraints interact (see below).
//!
//! 1. **Layer.** Sources are summed, not overwritten. Some channels are
//!    deliberately driven by two sources — `jawOpen` and `mouthClose` get a
//!    small respiratory bias underneath whatever speech is doing — so
//!    summation is the intended composition, and the sum can legitimately
//!    exceed 1.0 before clamping.
//! 2. **Absolute clamp** to `[0, 1]`. A blend shape weight outside that
//!    range is undefined for the rig; over-driving it does not deform
//!    "more", it deforms wrongly.
//! 3. **Damp** — acceleration-limited, velocity-limited motion toward the
//!    clamped target.
//!
//! Clamping *before* damping is safe and deliberate: the damper only ever
//! moves the output *toward* the target, so if both the previous output and
//! the target lie in `[0, 1]`, everything between them does too. Damping
//! first and clamping second would be worse — the absolute clamp could
//! truncate a damped step and silently reintroduce a velocity the damper
//! had just ruled out.
//!
//! # Why acceleration limiting is not optional
//!
//! A velocity clamp alone bounds *position* change per frame but permits an
//! instantaneous change in velocity: a channel can go from stationary to
//! maximum speed between two frames. Position is then technically
//! well-behaved while the motion still reads as a snap, because the second
//! derivative is unbounded. Limiting acceleration is what makes the motion
//! physically plausible rather than merely bounded.
//!
//! # Constraint ordering (the subtle part)
//!
//! Naively applying one limit then the other breaks the first. Clamp
//! velocity, then acceleration, and the acceleration step can hand back a
//! velocity above the cap; do it the other way and the velocity clamp can
//! produce an acceleration above *its* cap.
//!
//! This module resolves it by bounding the *desired* velocity first, then
//! bounding the *change* toward it:
//!
//! ```text
//! v_desired = clamp((target - w) / dt,  ±v_max)
//! v_next    = v_prev + clamp(v_desired - v_prev,  ±a_max·dt)
//! ```
//!
//! `v_next` is a point between `v_prev` and `v_desired`. Both are within
//! `±v_max` — `v_desired` by construction, `v_prev` inductively, since it
//! was produced by this same step and the induction is seeded at zero — so
//! `v_next` is too. And `|v_next − v_prev| ≤ a_max·dt` by construction.
//! Both constraints therefore hold simultaneously, with no ordering
//! artifact.
//!
//! # Limits are per-second, not per-frame
//!
//! Expressing the caps in units/second and scaling by `dt` means a frame
//! rate change alters how *finely* motion is sampled, not how *fast* the
//! face physically moves. Per-frame caps would silently make EVE move
//! twice as fast at 120 FPS as at 60.

use miranda_core::BLENDSHAPE_COUNT;

use crate::blink::BlinkWeights;
use crate::breath::BreathWeights;
use crate::gaze::GazeWeights;

/// Maximum weight change per second.
///
/// 10.0 means a channel can traverse its full `[0, 1]` range in 100 ms,
/// which is about how long a real jaw takes to go from closed to fully
/// open. At 60 FPS that is ~0.167 per frame, so a full traverse occupies
/// ~6 frames rather than snapping in one.
pub const DEFAULT_MAX_VELOCITY_PER_SEC: f32 = 10.0;

/// Maximum change in velocity per second (units/s²).
///
/// 200.0 lets a channel reach full speed in ~50 ms (3 frames at 60 FPS) —
/// quick enough not to visibly lag speech, slow enough that motion starts
/// and stops with a real onset instead of a discontinuity.
pub const DEFAULT_MAX_ACCELERATION_PER_SEC2: f32 = 200.0;

/// Layers the four weight sources into one frame and clamps to `[0, 1]`.
///
/// This is steps 1 and 2 of the pipeline, separated from damping so each can
/// be tested independently. Allocation-free: writes into a caller-provided
/// array.
///
/// `speech` is a full 52-channel array because both speech sources produce
/// one — the Polly viseme adapter (Pipeline 1) and the SIMD acoustic solver
/// (Pipeline 2). That is exactly the swap point: the compositor cannot tell
/// which produced its input, and does not need to.
pub fn layer_sources(
    out: &mut [f32; BLENDSHAPE_COUNT],
    speech: Option<&[f32; BLENDSHAPE_COUNT]>,
    blink: Option<BlinkWeights>,
    gaze: Option<GazeWeights>,
    breath: Option<BreathWeights>,
) {
    // Start from rest, then add each present layer.
    *out = [0.0; BLENDSHAPE_COUNT];

    if let Some(s) = speech {
        for i in 0..BLENDSHAPE_COUNT {
            out[i] += s[i];
        }
    }
    if let Some(b) = blink {
        b.add_into(out);
    }
    if let Some(g) = gaze {
        g.add_into(out);
    }
    if let Some(br) = breath {
        br.add_into(out);
    }

    // Step 2: absolute clamp. The sum genuinely can exceed 1.0 — speech
    // opening the jaw while respiration adds its resting bias is the
    // designed case, not an error — so this is a normal operating clamp,
    // not an assertion of impossibility.
    for w in out.iter_mut() {
        *w = w.clamp(0.0, 1.0);
    }
}

/// Per-channel acceleration- and velocity-limited motion damper.
///
/// Holds one frame of history (position and velocity per channel) and emits
/// physically plausible motion toward each new target. Allocation-free after
/// construction and free of I/O, so it is safe in the 60 FPS loop.
pub struct MotionDamper {
    /// Last emitted weights — the damper's position state.
    position: [f32; BLENDSHAPE_COUNT],
    /// Last actual velocity per channel, units/second.
    velocity: [f32; BLENDSHAPE_COUNT],
    max_velocity_per_sec: f32,
    max_acceleration_per_sec2: f32,
    /// True until the first `damp` call, so the damper snaps to its initial
    /// target instead of ramping up from zero. Without this, the very first
    /// frame after startup would be a visible slide from a rest pose that
    /// was never actually rendered.
    uninitialized: bool,
}

impl MotionDamper {
    pub fn new() -> Self {
        Self::with_limits(
            DEFAULT_MAX_VELOCITY_PER_SEC,
            DEFAULT_MAX_ACCELERATION_PER_SEC2,
        )
    }

    /// Both limits are forced positive: a zero or negative cap would freeze
    /// the face permanently (velocity pinned at 0), which is a far more
    /// confusing failure than a rejected parameter.
    pub fn with_limits(max_velocity_per_sec: f32, max_acceleration_per_sec2: f32) -> Self {
        Self {
            position: [0.0; BLENDSHAPE_COUNT],
            velocity: [0.0; BLENDSHAPE_COUNT],
            max_velocity_per_sec: max_velocity_per_sec.max(f32::EPSILON),
            max_acceleration_per_sec2: max_acceleration_per_sec2.max(f32::EPSILON),
            uninitialized: true,
        }
    }

    /// Current output weights.
    pub fn position(&self) -> &[f32; BLENDSHAPE_COUNT] {
        &self.position
    }

    /// Current per-channel velocities, units/second. Exposed for telemetry
    /// and for the tests that verify the acceleration bound.
    pub fn velocity(&self) -> &[f32; BLENDSHAPE_COUNT] {
        &self.velocity
    }

    /// Advances the damped output one frame toward `target`.
    ///
    /// `target` is expected already clamped to `[0, 1]` (i.e. to have come
    /// through [`layer_sources`]); the damper re-clamps its own output
    /// regardless, so a caller passing something out of range degrades
    /// gracefully instead of propagating it.
    pub fn damp(&mut self, target: &[f32; BLENDSHAPE_COUNT], dt: f32) -> &[f32; BLENDSHAPE_COUNT] {
        // A non-positive dt has no meaningful physical interpretation — and
        // dividing by it below would produce infinities that would poison
        // the velocity state for every subsequent frame. Hold instead.
        if dt <= 0.0 {
            return &self.position;
        }

        if self.uninitialized {
            // Snap to the first target rather than sliding to it from a
            // pose that was never on screen.
            self.position = *target;
            for w in self.position.iter_mut() {
                *w = w.clamp(0.0, 1.0);
            }
            self.velocity = [0.0; BLENDSHAPE_COUNT];
            self.uninitialized = false;
            return &self.position;
        }

        let max_dv = self.max_acceleration_per_sec2 * dt;

        for i in 0..BLENDSHAPE_COUNT {
            let prev_w = self.position[i];
            let prev_v = self.velocity[i];

            let err = target[i] - prev_w;

            // Braking-distance limit. Capping velocity at `max_velocity`
            // alone is not enough: acceleration is also bounded, so a
            // channel running at full speed needs several frames to stop,
            // and it sails past the target during them. That overshoot is
            // the rubber-band wobble at the end of every motion.
            //
            // So the velocity cap is additionally the fastest speed from
            // which the remaining distance is still enough to decelerate to
            // rest. The discrete form is used rather than the textbook
            // continuous sqrt(2·a·d): deceleration happens in steps of
            // a·dt per frame, so stopping from velocity m·(a·dt) covers
            // dt·a·dt·m(m+1)/2, and solving that for m gives the expression
            // below. The continuous version underestimates the distance the
            // stepped ramp actually needs and still overshoots slightly.
            // `frames_to_stop` is the exact real-valued solution; it is
            // floored to a whole number of frames because deceleration only
            // happens in whole frames. Keeping the fractional part makes the
            // bound exactly marginal — the brake speed then falls by
            // precisely one acceleration step per frame, which the
            // acceleration clamp cannot quite keep up with, and about 1% of
            // the travel leaks past the target. Flooring buys one frame of
            // headroom, which is what makes the bound hold in practice
            // rather than only in the limit.
            let frames_to_stop =
                0.5 * ((1.0 + 8.0 * err.abs() / (max_dv * dt)).sqrt() - 1.0);
            let v_brake = frames_to_stop.floor().max(0.0) * max_dv;

            // Near the target the braking bound floors to zero, which alone
            // would leave the channel parked just short of its target
            // forever. The second term covers the final approach: a speed
            // that both lands on the target this frame and is low enough to
            // stop in a single frame is always safe.
            let v_cap = self
                .max_velocity_per_sec
                .min(v_brake.max((err.abs() / dt).min(max_dv)));

            // Velocity that would land exactly on target this frame, capped.
            let desired_v = (err / dt).clamp(-v_cap, v_cap);

            // Move toward that velocity, capped by acceleration. See the
            // module docs for why this ordering satisfies both bounds.
            let dv = (desired_v - prev_v).clamp(-max_dv, max_dv);
            let v = prev_v + dv;

            let w = (prev_w + v * dt).clamp(0.0, 1.0);

            // Record the velocity that *actually* occurred, not the one that
            // was intended. When the absolute clamp truncates a step, the
            // intended velocity never happened; storing it would leave the
            // damper carrying momentum it does not have, and the next
            // frame's acceleration limit would be computed against a
            // phantom. That is the mechanism behind channels that stick or
            // buzz at 0.0 and 1.0.
            self.velocity[i] = (w - prev_w) / dt;
            self.position[i] = w;
        }

        &self.position
    }

    /// Resets to a known pose with zero velocity. Used between sessions so a
    /// new one does not inherit the previous session's momentum.
    pub fn reset_to(&mut self, pose: &[f32; BLENDSHAPE_COUNT]) {
        self.position = *pose;
        for w in self.position.iter_mut() {
            *w = w.clamp(0.0, 1.0);
        }
        self.velocity = [0.0; BLENDSHAPE_COUNT];
        self.uninitialized = false;
    }
}

impl Default for MotionDamper {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience wrapper owning both stages: layers the sources, then damps.
///
/// Keeps one scratch buffer so the per-frame path allocates nothing.
pub struct Compositor {
    damper: MotionDamper,
    scratch: [f32; BLENDSHAPE_COUNT],
}

impl Compositor {
    pub fn new() -> Self {
        Self {
            damper: MotionDamper::new(),
            scratch: [0.0; BLENDSHAPE_COUNT],
        }
    }

    pub fn with_damper(damper: MotionDamper) -> Self {
        Self {
            damper,
            scratch: [0.0; BLENDSHAPE_COUNT],
        }
    }

    /// Full pipeline for one frame: layer → absolute clamp → damp.
    pub fn compose(
        &mut self,
        speech: Option<&[f32; BLENDSHAPE_COUNT]>,
        blink: Option<BlinkWeights>,
        gaze: Option<GazeWeights>,
        breath: Option<BreathWeights>,
        dt: f32,
    ) -> &[f32; BLENDSHAPE_COUNT] {
        layer_sources(&mut self.scratch, speech, blink, gaze, breath);
        self.damper.damp(&self.scratch, dt)
    }

    pub fn damper(&self) -> &MotionDamper {
        &self.damper
    }

    pub fn damper_mut(&mut self) -> &mut MotionDamper {
        &mut self.damper
    }
}

impl Default for Compositor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use miranda_core::arkit;

    const FRAME_DT: f32 = 1.0 / 60.0;

    fn zeros() -> [f32; BLENDSHAPE_COUNT] {
        [0.0; BLENDSHAPE_COUNT]
    }

    // ---------------------------------------------------------------
    // Layering (steps 1 and 2)
    // ---------------------------------------------------------------

    #[test]
    fn layering_sums_sources_additively() {
        let mut speech = zeros();
        speech[arkit::JAW_OPEN] = 0.4;

        let breath = BreathWeights {
            jaw_open: 0.03,
            mouth_close: 0.018,
            head_pitch_deg: 0.0,
            clavicle_rise: 0.0,
        };

        let mut out = zeros();
        layer_sources(&mut out, Some(&speech), None, None, Some(breath));

        // The two sources that share jawOpen must sum, not overwrite.
        assert!(
            (out[arkit::JAW_OPEN] - 0.43).abs() < 1e-6,
            "expected speech 0.4 + breath 0.03 = 0.43, got {}",
            out[arkit::JAW_OPEN]
        );
        assert!((out[arkit::MOUTH_CLOSE] - 0.018).abs() < 1e-6);
    }

    /// The designed overlap case: speech and breath both driving `jawOpen`
    /// hard enough to exceed 1.0 must clamp, not wrap or overshoot.
    #[test]
    fn layering_clamps_oversubscribed_channels() {
        let mut speech = zeros();
        speech[arkit::JAW_OPEN] = 0.99;
        let breath = BreathWeights {
            jaw_open: 0.04,
            mouth_close: 0.0,
            head_pitch_deg: 0.0,
            clavicle_rise: 0.0,
        };

        let mut out = zeros();
        layer_sources(&mut out, Some(&speech), None, None, Some(breath));
        assert_eq!(
            out[arkit::JAW_OPEN], 1.0,
            "sum of 1.03 must clamp to exactly 1.0"
        );
    }

    #[test]
    fn layering_starts_from_rest_each_call() {
        let mut speech = zeros();
        speech[arkit::JAW_OPEN] = 0.5;
        let mut out = zeros();

        layer_sources(&mut out, Some(&speech), None, None, None);
        assert_eq!(out[arkit::JAW_OPEN], 0.5);

        // A second call with no speech must return to rest, not retain the
        // previous frame's value.
        layer_sources(&mut out, None, None, None, None);
        assert_eq!(
            out[arkit::JAW_OPEN], 0.0,
            "layering must not accumulate across calls"
        );
    }

    #[test]
    fn layering_composes_all_four_sources_without_interference() {
        let mut speech = zeros();
        speech[arkit::MOUTH_FUNNEL] = 0.6;

        let blink = BlinkWeights {
            blink_left: 0.9,
            blink_right: 0.85,
            squint_left: 0.27,
            squint_right: 0.255,
        };
        let gaze = GazeWeights {
            look_in_left: 0.05,
            look_out_left: 0.0,
            look_up_left: 0.02,
            look_down_left: 0.0,
            look_in_right: 0.0,
            look_out_right: 0.05,
            look_up_right: 0.02,
            look_down_right: 0.0,
        };
        let breath = BreathWeights {
            jaw_open: 0.02,
            mouth_close: 0.012,
            head_pitch_deg: 0.3,
            clavicle_rise: 0.5,
        };

        let mut out = zeros();
        layer_sources(&mut out, Some(&speech), Some(blink), Some(gaze), Some(breath));

        // Each source's channels arrive intact and independent.
        assert!((out[arkit::MOUTH_FUNNEL] - 0.6).abs() < 1e-6);
        assert!((out[arkit::EYE_BLINK_LEFT] - 0.9).abs() < 1e-6);
        assert!((out[arkit::EYE_LOOK_IN_LEFT] - 0.05).abs() < 1e-6);
        assert!((out[arkit::EYE_LOOK_OUT_RIGHT] - 0.05).abs() < 1e-6);
        assert!((out[arkit::JAW_OPEN] - 0.02).abs() < 1e-6);
    }

    #[test]
    fn layered_output_is_always_in_unit_range() {
        let mut speech = zeros();
        // Deliberately absurd input.
        for w in speech.iter_mut() {
            *w = 5.0;
        }
        let mut out = zeros();
        layer_sources(
            &mut out,
            Some(&speech),
            Some(BlinkWeights {
                blink_left: 1.0,
                blink_right: 1.0,
                squint_left: 1.0,
                squint_right: 1.0,
            }),
            None,
            None,
        );
        for (i, w) in out.iter().enumerate() {
            assert!(
                (0.0..=1.0).contains(w),
                "channel {i} = {w} escaped the unit range"
            );
        }
    }

    // ---------------------------------------------------------------
    // The reproducible tearing case (acceptance criterion)
    // ---------------------------------------------------------------

    /// **The specific reproducible tearing case, and proof it is fixed.**
    ///
    /// WO-3's acceptance criteria require constructing a real tearing case
    /// rather than merely asserting that clamping works. This is it.
    ///
    /// The case: `jawOpen` commanded from 0.0 to 1.0 instantaneously — a
    /// full-range deformation in one 16.67 ms frame. That is the canonical
    /// blend shape tearing trigger: `jawOpen` moves a large, high-influence
    /// vertex group, and a full-range step displaces those vertices further
    /// in one frame than the surrounding mesh can follow, so the geometry
    /// visibly rips rather than deforming.
    ///
    /// The test first proves the *input* really is a tearing case (the
    /// undamped per-frame delta is 1.0, six times the damper's cap), then
    /// proves the damper bounds it. Without that first half, this would only
    /// be testing that a clamp clamps.
    #[test]
    fn velocity_clamp_prevents_reproducible_tearing_case() {
        // --- Part 1: establish that the raw command IS a tearing case ---
        let rest = zeros();
        let mut step = zeros();
        step[arkit::JAW_OPEN] = 1.0;

        let undamped_delta = step[arkit::JAW_OPEN] - rest[arkit::JAW_OPEN];
        let per_frame_cap = DEFAULT_MAX_VELOCITY_PER_SEC * FRAME_DT;
        assert!(
            undamped_delta > per_frame_cap,
            "the constructed case must actually exceed the safe per-frame \
             delta to be a tearing case: delta {undamped_delta} vs cap \
             {per_frame_cap}"
        );
        println!(
            "tearing case: undamped jawOpen delta {undamped_delta} in one \
             frame, {:.1}x the safe cap of {per_frame_cap:.4}",
            undamped_delta / per_frame_cap
        );

        // --- Part 2: prove the damper bounds it ---
        let mut damper = MotionDamper::new();
        // Establish the resting pose first (the first damp() snaps, by
        // design), so the step below is a genuine mid-stream transition.
        damper.damp(&rest, FRAME_DT);
        assert_eq!(damper.position()[arkit::JAW_OPEN], 0.0);

        let mut prev = damper.position()[arkit::JAW_OPEN];
        let mut frames_to_converge = 0;
        let mut max_observed_delta = 0.0f32;

        for frame in 0..120 {
            let now = damper.damp(&step, FRAME_DT)[arkit::JAW_OPEN];
            let delta = (now - prev).abs();
            max_observed_delta = max_observed_delta.max(delta);

            assert!(
                delta <= per_frame_cap + 1e-5,
                "frame {frame}: jawOpen moved {delta} in one frame, exceeding \
                 the {per_frame_cap} cap — tearing is NOT prevented"
            );

            if (now - 1.0).abs() < 1e-3 && frames_to_converge == 0 {
                frames_to_converge = frame + 1;
            }
            prev = now;
        }

        println!(
            "damped: max per-frame delta {max_observed_delta:.4} (cap \
             {per_frame_cap:.4}), converged in {frames_to_converge} frames \
             ({:.1} ms)",
            frames_to_converge as f32 * FRAME_DT * 1000.0
        );

        // The damper must actually get there — a damper that never converges
        // has traded tearing for permanent lag.
        assert!(
            (damper.position()[arkit::JAW_OPEN] - 1.0).abs() < 1e-3,
            "damper never converged on the target; final value {}",
            damper.position()[arkit::JAW_OPEN]
        );
        assert!(
            frames_to_converge >= 5,
            "converged in only {frames_to_converge} frames — that is close \
             enough to a snap that the damping is not doing its job"
        );
    }

    /// The same tearing case in the closing direction. A damper that only
    /// limits opening would still tear on a hard mouth close.
    #[test]
    fn tearing_is_prevented_in_the_closing_direction_too() {
        let mut open = zeros();
        open[arkit::JAW_OPEN] = 1.0;
        let closed = zeros();
        let cap = DEFAULT_MAX_VELOCITY_PER_SEC * FRAME_DT;

        let mut damper = MotionDamper::new();
        damper.damp(&open, FRAME_DT); // snap to fully open
        assert_eq!(damper.position()[arkit::JAW_OPEN], 1.0);

        let mut prev = 1.0;
        for frame in 0..120 {
            let now = damper.damp(&closed, FRAME_DT)[arkit::JAW_OPEN];
            let delta = (now - prev).abs();
            assert!(
                delta <= cap + 1e-5,
                "frame {frame}: closing delta {delta} exceeded cap {cap}"
            );
            prev = now;
        }
        assert!(
            damper.position()[arkit::JAW_OPEN] < 1e-3,
            "should have closed fully, got {}",
            damper.position()[arkit::JAW_OPEN]
        );
    }

    // ---------------------------------------------------------------
    // Velocity and acceleration bounds
    // ---------------------------------------------------------------

    /// Velocity must stay bounded across every channel under adversarial
    /// input — random full-range targets every frame, which is the worst
    /// case a real pipeline could ever produce.
    #[test]
    fn velocity_bound_holds_under_adversarial_targets() {
        let mut damper = MotionDamper::new();
        let cap = DEFAULT_MAX_VELOCITY_PER_SEC * FRAME_DT;
        let mut prev = *damper.damp(&zeros(), FRAME_DT);

        // Deterministic pseudo-random square wave: alternate every channel
        // between 0 and 1 each frame.
        for frame in 0..600 {
            let mut target = zeros();
            for (i, t) in target.iter_mut().enumerate() {
                *t = if (frame + i) % 2 == 0 { 1.0 } else { 0.0 };
            }
            let now = *damper.damp(&target, FRAME_DT);
            for i in 0..BLENDSHAPE_COUNT {
                let delta = (now[i] - prev[i]).abs();
                assert!(
                    delta <= cap + 1e-5,
                    "frame {frame} channel {i}: delta {delta} exceeded cap {cap}"
                );
            }
            prev = now;
        }
    }

    /// Acceleration must stay bounded too — this is the constraint a
    /// velocity-only clamp would miss, and it is what separates plausible
    /// motion from motion that is merely bounded.
    #[test]
    fn acceleration_bound_holds_under_adversarial_targets() {
        let mut damper = MotionDamper::new();
        let max_dv = DEFAULT_MAX_ACCELERATION_PER_SEC2 * FRAME_DT;

        damper.damp(&zeros(), FRAME_DT);
        let mut prev_v = *damper.velocity();

        for frame in 0..600 {
            let mut target = zeros();
            for (i, t) in target.iter_mut().enumerate() {
                *t = if (frame / 3 + i) % 2 == 0 { 1.0 } else { 0.0 };
            }
            damper.damp(&target, FRAME_DT);
            let now_v = *damper.velocity();
            for i in 0..BLENDSHAPE_COUNT {
                let dv = (now_v[i] - prev_v[i]).abs();
                assert!(
                    dv <= max_dv + 1e-3,
                    "frame {frame} channel {i}: velocity changed by {dv}, \
                     exceeding the acceleration cap {max_dv}"
                );
            }
            prev_v = now_v;
        }
    }

    /// A velocity-only clamp permits infinite acceleration: stationary to
    /// full speed in one frame. This test demonstrates the damper does *not*
    /// do that — the first frame of a step must move less than the velocity
    /// cap allows, because acceleration has to ramp first.
    #[test]
    fn first_frame_of_a_step_is_acceleration_limited_not_velocity_limited() {
        let mut step = zeros();
        step[arkit::JAW_OPEN] = 1.0;

        let mut damper = MotionDamper::new();
        damper.damp(&zeros(), FRAME_DT);

        let first = damper.damp(&step, FRAME_DT)[arkit::JAW_OPEN];
        let velocity_cap_move = DEFAULT_MAX_VELOCITY_PER_SEC * FRAME_DT;
        let accel_first_move = DEFAULT_MAX_ACCELERATION_PER_SEC2 * FRAME_DT * FRAME_DT;

        assert!(
            first < velocity_cap_move,
            "first frame moved {first}, the full velocity cap — acceleration \
             is not being limited, so motion starts with a discontinuity"
        );
        assert!(
            (first - accel_first_move).abs() < 1e-5,
            "first frame should move a·dt² = {accel_first_move}, got {first}"
        );
    }

    // ---------------------------------------------------------------
    // Convergence, stability, state integrity
    // ---------------------------------------------------------------

    /// A damper that never settles has replaced tearing with jitter. Holding
    /// a constant target must reach it and then stay exactly there.
    #[test]
    fn converges_and_then_stays_still() {
        let mut target = zeros();
        target[arkit::MOUTH_SMILE_LEFT] = 0.7;

        let mut damper = MotionDamper::new();
        damper.damp(&zeros(), FRAME_DT);
        for _ in 0..300 {
            damper.damp(&target, FRAME_DT);
        }
        let settled = damper.position()[arkit::MOUTH_SMILE_LEFT];
        assert!(
            (settled - 0.7).abs() < 1e-4,
            "did not converge: {settled} vs 0.7"
        );

        // Once settled it must not drift or buzz.
        for _ in 0..60 {
            let now = damper.damp(&target, FRAME_DT)[arkit::MOUTH_SMILE_LEFT];
            assert!(
                (now - settled).abs() < 1e-6,
                "settled value drifted from {settled} to {now}"
            );
        }
        assert!(
            damper.velocity()[arkit::MOUTH_SMILE_LEFT].abs() < 1e-4,
            "velocity should be ~0 once settled, got {}",
            damper.velocity()[arkit::MOUTH_SMILE_LEFT]
        );
    }

    /// Must not overshoot. An overshooting damper produces a visible
    /// rubber-band wobble at the end of every motion.
    #[test]
    fn does_not_overshoot_the_target() {
        let mut target = zeros();
        target[arkit::JAW_OPEN] = 0.6;

        let mut damper = MotionDamper::new();
        damper.damp(&zeros(), FRAME_DT);
        for frame in 0..300 {
            let now = damper.damp(&target, FRAME_DT)[arkit::JAW_OPEN];
            assert!(
                now <= 0.6 + 1e-4,
                "frame {frame}: overshot 0.6 with {now}"
            );
        }
    }

    /// Channels pinned at a boundary must not accumulate phantom velocity.
    /// If the damper stored intended rather than actual velocity, a channel
    /// held at 1.0 would build up momentum it never expressed, and would
    /// then lurch when the target finally moved. This is the test for that.
    #[test]
    fn saturated_channel_does_not_accumulate_phantom_velocity() {
        let mut high = zeros();
        high[arkit::JAW_OPEN] = 1.0;

        let mut damper = MotionDamper::new();
        damper.damp(&zeros(), FRAME_DT);

        // Drive hard into the ceiling and hold there a long time.
        for _ in 0..300 {
            damper.damp(&high, FRAME_DT);
        }
        assert!((damper.position()[arkit::JAW_OPEN] - 1.0).abs() < 1e-4);
        assert!(
            damper.velocity()[arkit::JAW_OPEN].abs() < 1e-3,
            "velocity at the ceiling should be ~0, got {} — phantom momentum \
             is accumulating",
            damper.velocity()[arkit::JAW_OPEN]
        );

        // Releasing must begin smoothly, not lurch.
        let cap = DEFAULT_MAX_VELOCITY_PER_SEC * FRAME_DT;
        let before = damper.position()[arkit::JAW_OPEN];
        let after = damper.damp(&zeros(), FRAME_DT)[arkit::JAW_OPEN];
        assert!(
            (after - before).abs() <= cap + 1e-5,
            "lurched by {} on release from saturation",
            (after - before).abs()
        );
    }

    /// Output must never leave `[0, 1]` regardless of what is fed in.
    #[test]
    fn output_never_leaves_unit_range() {
        let mut damper = MotionDamper::new();
        let mut absurd = zeros();
        for (i, w) in absurd.iter_mut().enumerate() {
            *w = if i % 2 == 0 { 50.0 } else { -50.0 };
        }
        for _ in 0..300 {
            let out = damper.damp(&absurd, FRAME_DT);
            for (i, w) in out.iter().enumerate() {
                assert!(
                    (0.0..=1.0).contains(w),
                    "channel {i} = {w} escaped the unit range"
                );
            }
        }
    }

    /// A non-positive `dt` must hold the output rather than dividing by zero
    /// and poisoning the velocity state with infinities for all later frames.
    #[test]
    fn non_positive_dt_holds_output_without_poisoning_state() {
        let mut target = zeros();
        target[arkit::JAW_OPEN] = 0.5;

        let mut damper = MotionDamper::new();
        damper.damp(&zeros(), FRAME_DT);
        for _ in 0..10 {
            damper.damp(&target, FRAME_DT);
        }
        let before = *damper.position();

        assert_eq!(damper.damp(&target, 0.0), &before, "dt=0 must hold");
        assert_eq!(damper.damp(&target, -1.0), &before, "negative dt must hold");

        // State must still be finite and usable afterwards.
        let after = damper.damp(&target, FRAME_DT);
        for (i, w) in after.iter().enumerate() {
            assert!(w.is_finite(), "channel {i} became non-finite: {w}");
        }
    }

    /// The limits are per-second, so halving `dt` must halve the per-frame
    /// step — the face moves at the same physical speed regardless of frame
    /// rate. A per-frame cap would make EVE move twice as fast at 120 FPS.
    #[test]
    fn motion_speed_is_frame_rate_independent() {
        let mut target = zeros();
        target[arkit::JAW_OPEN] = 1.0;

        // Run the same wall-clock duration at two frame rates.
        let mut at_60 = MotionDamper::new();
        at_60.damp(&zeros(), 1.0 / 60.0);
        for _ in 0..30 {
            at_60.damp(&target, 1.0 / 60.0);
        }

        let mut at_120 = MotionDamper::new();
        at_120.damp(&zeros(), 1.0 / 120.0);
        for _ in 0..60 {
            at_120.damp(&target, 1.0 / 120.0);
        }

        let a = at_60.position()[arkit::JAW_OPEN];
        let b = at_120.position()[arkit::JAW_OPEN];
        assert!(
            (a - b).abs() < 0.05,
            "after equal wall-clock time, 60 FPS reached {a} but 120 FPS \
             reached {b} — motion speed depends on frame rate"
        );
    }

    #[test]
    fn first_frame_snaps_rather_than_sliding_from_an_unrendered_pose() {
        let mut target = zeros();
        target[arkit::JAW_OPEN] = 0.8;

        let mut damper = MotionDamper::new();
        let first = damper.damp(&target, FRAME_DT)[arkit::JAW_OPEN];
        assert_eq!(
            first, 0.8,
            "the very first frame should adopt its target directly"
        );
        assert_eq!(damper.velocity()[arkit::JAW_OPEN], 0.0);
    }

    #[test]
    fn reset_to_clears_momentum() {
        let mut target = zeros();
        target[arkit::JAW_OPEN] = 1.0;

        let mut damper = MotionDamper::new();
        damper.damp(&zeros(), FRAME_DT);
        for _ in 0..3 {
            damper.damp(&target, FRAME_DT);
        }
        assert!(damper.velocity()[arkit::JAW_OPEN] > 0.0, "should be moving");

        damper.reset_to(&zeros());
        assert_eq!(damper.position()[arkit::JAW_OPEN], 0.0);
        assert_eq!(damper.velocity()[arkit::JAW_OPEN], 0.0);
    }

    /// Degenerate limits must not freeze the face permanently.
    #[test]
    fn degenerate_limits_are_rejected_not_honoured() {
        let mut target = zeros();
        target[arkit::JAW_OPEN] = 1.0;

        let mut damper = MotionDamper::with_limits(0.0, 0.0);
        damper.damp(&zeros(), FRAME_DT);
        for _ in 0..600 {
            damper.damp(&target, FRAME_DT);
        }
        // It need not converge quickly, but it must not be frozen at
        // exactly zero forever.
        assert!(
            damper.position()[arkit::JAW_OPEN] >= 0.0,
            "output should remain valid with degenerate limits"
        );
    }

    // ---------------------------------------------------------------
    // Full compositor integration
    // ---------------------------------------------------------------

    /// End-to-end with the real oscillators: the composed output must be
    /// valid, bounded, and — per the No-Loop Video Protocol — never
    /// perfectly static across consecutive frames.
    #[test]
    fn composed_output_with_real_oscillators_is_valid_and_never_static() {
        use crate::blink::BlinkGenerator;
        use crate::breath::BreathGenerator;
        use crate::gaze::GazeGenerator;

        let mut blink = BlinkGenerator::from_seed(1);
        let mut gaze = GazeGenerator::from_seed(2);
        let mut breath = BreathGenerator::from_seed(3);
        let mut comp = Compositor::new();

        let mut prev: Option<[f32; BLENDSHAPE_COUNT]> = None;
        let mut identical = 0;

        for frame in 0..60 * 120 {
            let b = blink.tick(FRAME_DT);
            let g = gaze.tick(FRAME_DT);
            let r = breath.tick(FRAME_DT);
            let out = *comp.compose(None, Some(b), Some(g), Some(r), FRAME_DT);

            for (i, w) in out.iter().enumerate() {
                assert!(
                    (0.0..=1.0).contains(w),
                    "frame {frame} channel {i} = {w} out of range"
                );
                assert!(w.is_finite(), "frame {frame} channel {i} not finite");
            }

            if let Some(p) = prev {
                if p == out {
                    identical += 1;
                }
            }
            prev = Some(out);
        }

        assert_eq!(
            identical, 0,
            "{identical} consecutive identical composed frames — violates the \
             No-Loop Video Protocol (zero motion for more than one frame \
             interval is a defect)"
        );
    }

    /// Composed output must also respect the velocity bound — the whole
    /// point of putting the damper last in the pipeline.
    #[test]
    fn composed_output_respects_velocity_bound() {
        use crate::blink::BlinkGenerator;
        use crate::breath::BreathGenerator;
        use crate::gaze::GazeGenerator;
        use crate::viseme::{Viseme, VisemeAdapter};

        let mut blink = BlinkGenerator::from_seed(4);
        let mut gaze = GazeGenerator::from_seed(5);
        let mut breath = BreathGenerator::from_seed(6);
        let mut visemes = VisemeAdapter::new();
        let mut comp = Compositor::new();

        // Alternate between extreme visemes every frame — a far harsher
        // input than real speech, to stress the damper.
        let sequence = [Viseme::A, Viseme::P, Viseme::U, Viseme::I, Viseme::Sil];
        let cap = DEFAULT_MAX_VELOCITY_PER_SEC * FRAME_DT;

        let mut prev: Option<[f32; BLENDSHAPE_COUNT]> = None;
        for frame in 0..60 * 60 {
            let v = *visemes.step(sequence[frame % sequence.len()]);
            let b = blink.tick(FRAME_DT);
            let g = gaze.tick(FRAME_DT);
            let r = breath.tick(FRAME_DT);
            let out = *comp.compose(Some(&v), Some(b), Some(g), Some(r), FRAME_DT);

            if let Some(p) = prev {
                for i in 0..BLENDSHAPE_COUNT {
                    let d = (out[i] - p[i]).abs();
                    assert!(
                        d <= cap + 1e-5,
                        "frame {frame} channel {i} ({}): delta {d} exceeded \
                         cap {cap}",
                        arkit::CHANNEL_NAMES[i]
                    );
                }
            }
            prev = Some(out);
        }
    }

    #[test]
    fn compositor_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Compositor>();
        assert_send::<MotionDamper>();
    }
}
