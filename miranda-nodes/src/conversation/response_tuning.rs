//! Task 7 — Response latency & depth tuning.
//!
//! Requirement 2.3/2.4: the target latency and depth of Miranda's
//! response should adapt to the user's mood and conversation state —
//! a frustrated user debugging a crash wants a fast, short answer they
//! can act on immediately; a curious user in deep reflection can be
//! given a slower, more thorough response. This module computes that
//! target *before* inference starts, so the LLM call and any streaming
//! decision downstream have a concrete number to aim for rather than a
//! fixed one-size-fits-all budget.

use crate::conversation::mood_stream::MoodVector;
use crate::conversation::state_machine::State;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResponseTarget {
    /// Target time-to-first-token, in milliseconds.
    pub target_latency_ms: u32,
    /// Relative depth on a 0.0 (terse) - 1.0 (thorough) scale.
    pub depth: f32,
    /// Whether partial/streamed output should be preferred over waiting
    /// for a complete response — set under high frustration so the user
    /// sees *something* moving quickly rather than a silent wait.
    pub prefer_streaming: bool,
}


/// Requirement 2.3/2.4. Pure function of mood + state — no I/O, no
/// hidden state — so it's trivially testable against the 10 mood/state
/// combinations the task calls for, and cheap enough to call on every
/// turn without its own latency budget line.
pub fn compute_target(mood: &MoodVector, state: State) -> ResponseTarget {
    // Frustration is the dominant driver: high frustration always wants
    // fast + terse + streamed, regardless of state, because "I'm annoyed
    // and stuck" is a worse experience to leave waiting than any other
    // combination this function can encounter.
    let frustration_latency_cut = (mood.frustration * 500.0) as u32;
    let fatigue_latency_cut = (mood.fatigue * 200.0) as u32;

    let state_base_latency: u32 = match state {
        State::Debugging => 500,
        State::Opening => 700,
        State::Casual => 700,
        State::DeepWork => 1000,
        State::Reflection => 1200,
    };

    let target_latency_ms = state_base_latency
        .saturating_sub(frustration_latency_cut)
        .saturating_sub(fatigue_latency_cut)
        .max(200); // never target below 200ms — an unrealistically fast
                   // target would just be silently missed every time.

    let state_base_depth: f32 = match state {
        State::Debugging => 0.35,
        State::Opening => 0.4,
        State::Casual => 0.3,
        State::DeepWork => 0.75,
        State::Reflection => 0.85,
    };

    // Curiosity nudges depth up (the user wants more), frustration nudges
    // it down (the user wants the fix, not the essay).
    let depth = (state_base_depth + mood.curiosity * 0.2 - mood.frustration * 0.3).clamp(0.0, 1.0);

    let prefer_streaming = mood.frustration > 0.6 || state == State::Debugging;

    ResponseTarget {
        target_latency_ms,
        depth,
        prefer_streaming,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mood(frustration: f32, curiosity: f32, engagement: f32, fatigue: f32, excitement: f32) -> MoodVector {
        MoodVector { frustration, curiosity, engagement, fatigue, excitement }
    }

    #[test]
    fn calm_deep_work_targets_slower_deeper_response() {
        let target = compute_target(&mood(0.0, 0.3, 0.7, 0.1, 0.2), State::DeepWork);
        assert!(target.target_latency_ms >= 800);
        assert!(target.depth > 0.6);
        assert!(!target.prefer_streaming);
    }

    #[test]
    fn frustrated_debugging_targets_fast_terse_streamed_response() {
        let target = compute_target(&mood(0.9, 0.1, 0.5, 0.3, 0.0), State::Debugging);
        assert!(target.target_latency_ms <= 400);
        assert!(target.depth < 0.3);
        assert!(target.prefer_streaming);
    }

    #[test]
    fn reflection_with_curiosity_targets_thorough_response() {
        let target = compute_target(&mood(0.0, 0.8, 0.5, 0.1, 0.1), State::Reflection);
        assert!(target.depth > 0.8);
    }

    #[test]
    fn casual_state_targets_short_light_response() {
        let target = compute_target(&mood(0.0, 0.1, 0.4, 0.0, 0.1), State::Casual);
        assert!(target.depth < 0.5);
    }

    #[test]
    fn high_fatigue_reduces_target_latency_regardless_of_state() {
        let low_fatigue = compute_target(&mood(0.0, 0.2, 0.5, 0.0, 0.1), State::DeepWork);
        let high_fatigue = compute_target(&mood(0.0, 0.2, 0.5, 0.9, 0.1), State::DeepWork);
        assert!(high_fatigue.target_latency_ms < low_fatigue.target_latency_ms);
    }

    #[test]
    fn target_latency_never_drops_below_the_200ms_floor() {
        let target = compute_target(&mood(1.0, 0.0, 0.0, 1.0, 0.0), State::Debugging);
        assert!(target.target_latency_ms >= 200);
    }

    #[test]
    fn depth_is_always_within_unit_range() {
        for f in [0.0, 0.5, 1.0] {
            for c in [0.0, 0.5, 1.0] {
                let target = compute_target(&mood(f, c, 0.5, 0.5, 0.5), State::Reflection);
                assert!((0.0..=1.0).contains(&target.depth));
            }
        }
    }

    #[test]
    fn moderate_frustration_prefers_streaming_even_outside_debugging() {
        let target = compute_target(&mood(0.7, 0.1, 0.5, 0.1, 0.0), State::DeepWork);
        assert!(target.prefer_streaming);
    }

    #[test]
    fn low_frustration_deep_work_does_not_prefer_streaming() {
        let target = compute_target(&mood(0.1, 0.3, 0.6, 0.1, 0.2), State::DeepWork);
        assert!(!target.prefer_streaming);
    }

    #[test]
    fn opening_state_has_moderate_default_targets() {
        let target = compute_target(&mood(0.0, 0.0, 0.3, 0.0, 0.0), State::Opening);
        assert!(target.target_latency_ms >= 600 && target.target_latency_ms <= 800);
        assert!(target.depth > 0.2 && target.depth < 0.6);
    }
}
