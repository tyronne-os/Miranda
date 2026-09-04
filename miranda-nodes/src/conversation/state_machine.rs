//! Task 2 — Conversation state machine.
//!
//! Hierarchical state (design.md): five top-level `State`s, each paired
//! with a `MicroState` describing what Miranda is doing moment-to-moment
//! within that state. Transition logic is driven by the continuous mood
//! vector (Task 1), entity signals extracted from the turn, and optional
//! explicit user cues ("let's debug this", "just chatting").
//!
//! `Turn` lives here (not in a separate types module) because the state
//! machine is the first component in the pipeline that needs a plain
//! "one thing the user or Miranda said, with a timestamp" record, and
//! design.md's later components (Partnership Tracker, Anticipatory Move
//! Generator) both take `&[Turn]` — this is the shared minimal shape,
//! kept deliberately smaller than WO-Memory's full `Event` so this crate
//! doesn't need a compile-time dependency in that direction.

use chrono::{DateTime, Utc};

use crate::conversation::mood_stream::MoodVector;

#[derive(Debug, Clone)]
pub struct Turn {
    pub text: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Opening,
    DeepWork,
    Debugging,
    Reflection,
    Casual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicroState {
    Listening,
    Thinking,
    Talking,
    Probing,
    Leading,
}

/// Explicit user cues that short-circuit mood/entity-driven inference —
/// design.md's `transition` interface takes `explicit_cue: Option<Cue>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    StartDebugging,
    StartDeepWork,
    WrapUp,
    JustChatting,
}

/// Lexical cues a caller can derive from raw text before calling
/// `transition` — kept separate from `Cue` parsing itself so tests can
/// exercise `transition` directly against known cues without depending
/// on this heuristic, and so the heuristic can be swapped independently.
pub fn detect_cue(message: &str) -> Option<Cue> {
    let lower = message.to_lowercase();
    if lower.contains("let's debug") || lower.contains("lets debug") || lower.contains("this is broken") {
        Some(Cue::StartDebugging)
    } else if lower.contains("let's build") || lower.contains("lets build") || lower.contains("let's dive in") {
        Some(Cue::StartDeepWork)
    } else if lower.contains("that's a wrap") || lower.contains("thats a wrap") || lower.contains("let's wrap up") {
        Some(Cue::WrapUp)
    } else if lower.contains("just chatting") || lower.contains("just talking") || lower.contains("random question") {
        Some(Cue::JustChatting)
    } else {
        None
    }
}

/// Requirement 2.1-2.5. Entity signals are keyword-level (error/bug
/// terms vs. topic-continuity terms) rather than a full NER pass — the
/// heavier entity extraction from WO-Memory runs elsewhere in the
/// pipeline; the state machine only needs enough signal to bias
/// transitions, per design.md's interface shape (`entities: &[String]`,
/// already-extracted strings, not raw text).
pub struct StateMachine {
    pub state: State,
    pub micro_state: MicroState,
}

impl Default for StateMachine {
    fn default() -> Self {
        Self {
            state: State::Opening,
            micro_state: MicroState::Listening,
        }
    }
}

const DEBUG_ENTITY_MARKERS: &[&str] = &["error", "bug", "exception", "panic", "crash", "traceback", "stack trace"];

impl StateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// design.md: `transition(current, mood, entities, explicit_cue) -> State`.
    /// Implemented as `&mut self` (holding `current` as `self.state`)
    /// rather than free-standing, since the micro-state also needs to
    /// carry forward and a free function would force callers to thread
    /// two pieces of state through by hand.
    pub fn transition(
        &mut self,
        mood: &MoodVector,
        entities: &[String],
        explicit_cue: Option<Cue>,
    ) -> State {
        // Explicit cues take precedence over inferred signals — a user
        // who says "let's debug this" means it regardless of what the
        // mood vector or entity list would otherwise suggest.
        if let Some(cue) = explicit_cue {
            self.state = match cue {
                Cue::StartDebugging => State::Debugging,
                Cue::StartDeepWork => State::DeepWork,
                Cue::WrapUp => State::Reflection,
                Cue::JustChatting => State::Casual,
            };
            self.micro_state = MicroState::Leading;
            return self.state;
        }

        let has_debug_marker = entities
            .iter()
            .any(|e| DEBUG_ENTITY_MARKERS.iter().any(|m| e.to_lowercase().contains(m)));

        self.state = match self.state {
            State::Opening => {
                if has_debug_marker {
                    State::Debugging
                } else if mood.engagement > 0.6 {
                    State::DeepWork
                } else {
                    State::Casual
                }
            }
            State::DeepWork => {
                if has_debug_marker {
                    State::Debugging
                } else if mood.fatigue > 0.7 || mood.frustration > 0.75 {
                    State::Reflection
                } else {
                    State::DeepWork
                }
            }
            State::Debugging => {
                if mood.frustration > 0.85 {
                    // Sustained high frustration during debugging is the
                    // one path design.md's response-tuning module cares
                    // about most (Requirement 2.3/2.4) — surfacing it as
                    // a state transition to Reflection gives the tuning
                    // layer somewhere to react to a step early.
                    State::Reflection
                } else if !has_debug_marker && mood.engagement > 0.5 {
                    State::DeepWork
                } else {
                    State::Debugging
                }
            }
            State::Reflection => {
                if mood.engagement > 0.5 && mood.fatigue < 0.4 {
                    State::DeepWork
                } else if mood.engagement < 0.3 {
                    State::Casual
                } else {
                    State::Reflection
                }
            }
            State::Casual => {
                if has_debug_marker {
                    State::Debugging
                } else if mood.engagement > 0.65 {
                    State::DeepWork
                } else {
                    State::Casual
                }
            }
        };

        self.micro_state = self.infer_micro_state(mood, has_debug_marker);
        self.state
    }

    fn infer_micro_state(&self, mood: &MoodVector, has_debug_marker: bool) -> MicroState {
        if mood.frustration > 0.7 {
            MicroState::Probing
        } else if has_debug_marker {
            MicroState::Thinking
        } else if mood.curiosity > 0.6 {
            MicroState::Probing
        } else if mood.engagement > 0.6 {
            MicroState::Talking
        } else {
            MicroState::Listening
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mood(frustration: f32, curiosity: f32, engagement: f32, fatigue: f32, excitement: f32) -> MoodVector {
        MoodVector {
            frustration,
            curiosity,
            engagement,
            fatigue,
            excitement,
        }
    }

    #[test]
    fn starts_in_opening_state() {
        let sm = StateMachine::new();
        assert_eq!(sm.state, State::Opening);
    }

    #[test]
    fn opening_with_debug_marker_transitions_to_debugging() {
        let mut sm = StateMachine::new();
        let state = sm.transition(&mood(0.2, 0.2, 0.5, 0.1, 0.1), &["stack trace".to_string()], None);
        assert_eq!(state, State::Debugging);
    }

    #[test]
    fn opening_with_high_engagement_transitions_to_deep_work() {
        let mut sm = StateMachine::new();
        let state = sm.transition(&mood(0.1, 0.3, 0.8, 0.1, 0.3), &[], None);
        assert_eq!(state, State::DeepWork);
    }

    #[test]
    fn opening_with_low_engagement_transitions_to_casual() {
        let mut sm = StateMachine::new();
        let state = sm.transition(&mood(0.1, 0.1, 0.2, 0.2, 0.1), &[], None);
        assert_eq!(state, State::Casual);
    }

    #[test]
    fn explicit_cue_overrides_inferred_signals() {
        let mut sm = StateMachine::new();
        // Mood/entities would suggest Casual, but the explicit cue wins.
        let state = sm.transition(&mood(0.0, 0.0, 0.1, 0.0, 0.0), &[], Some(Cue::StartDebugging));
        assert_eq!(state, State::Debugging);
        assert_eq!(sm.micro_state, MicroState::Leading);
    }

    #[test]
    fn sustained_frustration_in_debugging_moves_to_reflection() {
        let mut sm = StateMachine::new();
        sm.transition(&mood(0.2, 0.2, 0.5, 0.1, 0.1), &["bug".to_string()], None);
        assert_eq!(sm.state, State::Debugging);
        let state = sm.transition(&mood(0.9, 0.1, 0.4, 0.5, 0.0), &["bug".to_string()], None);
        assert_eq!(state, State::Reflection);
    }

    #[test]
    fn deep_work_with_high_fatigue_moves_to_reflection() {
        let mut sm = StateMachine::new();
        sm.transition(&mood(0.1, 0.3, 0.8, 0.1, 0.3), &[], None);
        assert_eq!(sm.state, State::DeepWork);
        let state = sm.transition(&mood(0.2, 0.2, 0.6, 0.8, 0.1), &[], None);
        assert_eq!(state, State::Reflection);
    }

    #[test]
    fn reflection_with_renewed_engagement_returns_to_deep_work() {
        let mut sm = StateMachine::new();
        sm.transition(&mood(0.9, 0.1, 0.4, 0.5, 0.0), &["bug".to_string()], None);
        // force into reflection via cue for a clean starting point
        sm.transition(&mood(0.0, 0.0, 0.0, 0.0, 0.0), &[], Some(Cue::WrapUp));
        assert_eq!(sm.state, State::Reflection);
        let state = sm.transition(&mood(0.1, 0.3, 0.7, 0.2, 0.4), &[], None);
        assert_eq!(state, State::DeepWork);
    }

    #[test]
    fn casual_with_debug_marker_moves_to_debugging() {
        let mut sm = StateMachine::new();
        sm.transition(&mood(0.0, 0.0, 0.0, 0.0, 0.0), &[], Some(Cue::JustChatting));
        assert_eq!(sm.state, State::Casual);
        let state = sm.transition(&mood(0.3, 0.2, 0.4, 0.1, 0.1), &["exception".to_string()], None);
        assert_eq!(state, State::Debugging);
    }

    #[test]
    fn micro_state_reflects_high_frustration_as_probing() {
        let mut sm = StateMachine::new();
        sm.transition(&mood(0.8, 0.1, 0.5, 0.2, 0.0), &[], None);
        assert_eq!(sm.micro_state, MicroState::Probing);
    }

    #[test]
    fn detect_cue_recognizes_debugging_phrases() {
        assert_eq!(detect_cue("let's debug this together"), Some(Cue::StartDebugging));
        assert_eq!(detect_cue("this is broken again"), Some(Cue::StartDebugging));
    }

    #[test]
    fn detect_cue_recognizes_deep_work_phrases() {
        assert_eq!(detect_cue("let's build the renderer"), Some(Cue::StartDeepWork));
    }

    #[test]
    fn detect_cue_recognizes_wrap_up_and_casual_phrases() {
        assert_eq!(detect_cue("that's a wrap for today"), Some(Cue::WrapUp));
        assert_eq!(detect_cue("just chatting, random question"), Some(Cue::JustChatting));
    }

    #[test]
    fn detect_cue_returns_none_for_unrelated_text() {
        assert_eq!(detect_cue("what's the weather like"), None);
    }
}
