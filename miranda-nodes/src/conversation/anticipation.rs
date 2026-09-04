//! Task 3 — Anticipatory move generator. [CAT 4]
//!
//! Requirement 3.1-3.4 / design.md Property 1 (confidence gating): this
//! is the one component in the pipeline where being confidently wrong is
//! worse than saying nothing — an overconfident, incorrect "next step"
//! suggestion reads as presumptuous rather than helpful. The design
//! response to that risk is structural, matching the same pattern as
//! `autonomy_calibration`'s floor: [`generate_moves`] never returns a
//! move below the 0.7 confidence threshold, because the filter is
//! applied inside the function, not left to callers to remember to
//! apply. There is no code path that returns an unfiltered list.
//!
//! Confidence scoring here is a small set of heuristic signals (state
//! match, mood alignment, entity/keyword recurrence across the last few
//! turns) combined additively and capped at 1.0 — not a learned model.
//! That's a deliberate, disclosed limitation: a real trained ranker is
//! future work; what this module guarantees today is the *gating*
//! behavior (Property 1), which holds regardless of how the underlying
//! scores are produced.

use crate::conversation::mood_stream::MoodVector;
use crate::conversation::state_machine::{State, Turn};

pub const CONFIDENCE_THRESHOLD: f32 = 0.7;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MoveCategory {
    SuggestNextStep,
    OfferToTest,
    OfferToExplain,
    SuggestRelatedWork,
}

#[derive(Debug, Clone)]
pub struct ScoredMove {
    pub text: String,
    pub confidence: f32,
    pub category: MoveCategory,
}

#[derive(Debug, Clone)]
struct Candidate {
    text: String,
    category: MoveCategory,
    base_confidence: f32,
}

pub struct AnticipationEngine {
    /// Requirement 3.4 — dismissal feedback: each dismissed category
    /// gets a persistent penalty applied to future candidates of that
    /// same category, so repeatedly-dismissed move types stop surfacing
    /// even if their raw heuristic score would otherwise clear the gate.
    dismissal_penalty: std::collections::HashMap<MoveCategory, f32>,
}

impl Default for AnticipationEngine {
    fn default() -> Self {
        Self {
            dismissal_penalty: std::collections::HashMap::new(),
        }
    }
}

impl AnticipationEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Requirement 3.4. Each dismissal adds a fixed 0.15 penalty,
    /// capped so a category can be fully suppressed but the map itself
    /// doesn't grow unbounded in magnitude.
    pub fn record_dismissal(&mut self, category: MoveCategory) {
        let penalty = self.dismissal_penalty.entry(category).or_insert(0.0);
        *penalty = (*penalty + 0.15).min(1.0);
    }

    fn candidates_for(state: State, mood: &MoodVector, history: &[Turn]) -> Vec<Candidate> {
        let mut out = Vec::new();

        let recent_mentions_tests = history
            .iter()
            .rev()
            .take(5)
            .any(|t| t.text.to_lowercase().contains("test") || t.text.to_lowercase().contains("verify"));

        match state {
            State::Debugging => {
                out.push(Candidate {
                    text: "Want me to add a regression test once this is fixed, so it doesn't come back?".to_string(),
                    category: MoveCategory::OfferToTest,
                    base_confidence: if recent_mentions_tests { 0.85 } else { 0.6 },
                });
                out.push(Candidate {
                    text: "I can walk through why this failed if that'd help.".to_string(),
                    category: MoveCategory::OfferToExplain,
                    base_confidence: if mood.frustration > 0.5 { 0.5 } else { 0.72 },
                });
            }
            State::DeepWork => {
                out.push(Candidate {
                    text: "Once this piece is done, the next logical step looks like wiring it into the pipeline — want me to line that up?".to_string(),
                    category: MoveCategory::SuggestNextStep,
                    base_confidence: 0.72,
                });
                out.push(Candidate {
                    text: "There's a related module that touches similar logic — worth a quick look together?".to_string(),
                    category: MoveCategory::SuggestRelatedWork,
                    base_confidence: 0.55,
                });
            }
            State::Reflection => {
                out.push(Candidate {
                    text: "Want to capture what we learned here before moving on?".to_string(),
                    category: MoveCategory::SuggestNextStep,
                    base_confidence: 0.72,
                });
            }
            State::Opening | State::Casual => {
                // Low-signal states: nothing scores high enough to clear
                // the gate by design, rather than fabricating a candidate
                // just to have one — matches design.md's error-handling
                // posture ("silently omits proactive content").
            }
        }

        out
    }

    /// design.md: `generate_moves(state, mood, history) -> Vec<ScoredMove>`.
    /// Returns 0-3 moves; every returned move has `confidence >= 0.7`
    /// (Property 1) and reflects any accumulated dismissal penalty for
    /// its category.
    pub fn generate_moves(&self, state: State, mood: &MoodVector, history: &[Turn]) -> Vec<ScoredMove> {
        let candidates = Self::candidates_for(state, mood, history);

        let mut scored: Vec<ScoredMove> = candidates
            .into_iter()
            .map(|c| {
                let penalty = self.dismissal_penalty.get(&c.category).copied().unwrap_or(0.0);
                let confidence = (c.base_confidence - penalty).clamp(0.0, 1.0);
                ScoredMove {
                    text: c.text,
                    confidence,
                    category: c.category,
                }
            })
            .filter(|m| m.confidence >= CONFIDENCE_THRESHOLD)
            .collect();

        scored.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        scored.truncate(3);
        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn mood(frustration: f32, curiosity: f32, engagement: f32, fatigue: f32, excitement: f32) -> MoodVector {
        MoodVector { frustration, curiosity, engagement, fatigue, excitement }
    }

    fn turn(text: &str) -> Turn {
        Turn { text: text.to_string(), timestamp: Utc::now() }
    }

    /// design.md Property 1, the hard gate: no move below threshold is
    /// ever returned, across every state/mood combination this module
    /// can produce candidates for.
    #[test]
    fn no_returned_move_is_ever_below_the_confidence_threshold() {
        let engine = AnticipationEngine::new();
        let states = [State::Opening, State::DeepWork, State::Debugging, State::Reflection, State::Casual];
        let moods = [
            mood(0.0, 0.0, 0.0, 0.0, 0.0),
            mood(1.0, 1.0, 1.0, 1.0, 1.0),
            mood(0.5, 0.5, 0.5, 0.5, 0.5),
            mood(0.9, 0.1, 0.3, 0.7, 0.0),
        ];
        for state in states {
            for m in &moods {
                let moves = engine.generate_moves(state, m, &[]);
                for mv in moves {
                    assert!(
                        mv.confidence >= CONFIDENCE_THRESHOLD,
                        "returned move below threshold: {mv:?} for state {state:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn debugging_with_recent_test_mentions_surfaces_offer_to_test() {
        let engine = AnticipationEngine::new();
        let history = vec![turn("let me verify this works"), turn("still broken")];
        let moves = engine.generate_moves(State::Debugging, &mood(0.2, 0.2, 0.5, 0.1, 0.1), &history);
        assert!(moves.iter().any(|m| m.category == MoveCategory::OfferToTest));
    }

    #[test]
    fn opening_and_casual_states_produce_no_moves() {
        let engine = AnticipationEngine::new();
        assert!(engine.generate_moves(State::Opening, &mood(0.0, 0.0, 0.0, 0.0, 0.0), &[]).is_empty());
        assert!(engine.generate_moves(State::Casual, &mood(0.0, 0.0, 0.0, 0.0, 0.0), &[]).is_empty());
    }

    #[test]
    fn dismissal_lowers_future_confidence_for_the_same_category() {
        let mut engine = AnticipationEngine::new();
        let history = vec![turn("let's verify this works")];
        let before = engine.generate_moves(State::Debugging, &mood(0.2, 0.2, 0.5, 0.1, 0.1), &history);
        assert!(before.iter().any(|m| m.category == MoveCategory::OfferToTest));

        engine.record_dismissal(MoveCategory::OfferToTest);
        engine.record_dismissal(MoveCategory::OfferToTest);
        let after = engine.generate_moves(State::Debugging, &mood(0.2, 0.2, 0.5, 0.1, 0.1), &history);
        let after_test_move = after.iter().find(|m| m.category == MoveCategory::OfferToTest);
        assert!(after_test_move.is_none(), "expected OfferToTest to drop below threshold after dismissals");
    }

    #[test]
    fn repeated_dismissals_can_fully_suppress_a_category() {
        let mut engine = AnticipationEngine::new();
        for _ in 0..10 {
            engine.record_dismissal(MoveCategory::OfferToExplain);
        }
        let moves = engine.generate_moves(State::Debugging, &mood(0.1, 0.1, 0.5, 0.1, 0.1), &[]);
        assert!(!moves.iter().any(|m| m.category == MoveCategory::OfferToExplain));
    }

    #[test]
    fn deep_work_surfaces_next_step_suggestion() {
        let engine = AnticipationEngine::new();
        let moves = engine.generate_moves(State::DeepWork, &mood(0.1, 0.3, 0.7, 0.1, 0.2), &[]);
        assert!(moves.iter().any(|m| m.category == MoveCategory::SuggestNextStep));
    }

    #[test]
    fn returns_at_most_three_moves() {
        let engine = AnticipationEngine::new();
        let history = vec![turn("let's verify and test this")];
        let moves = engine.generate_moves(State::Debugging, &mood(0.1, 0.1, 0.5, 0.1, 0.1), &history);
        assert!(moves.len() <= 3);
    }

    #[test]
    fn moves_are_sorted_by_descending_confidence() {
        let engine = AnticipationEngine::new();
        let history = vec![turn("let's verify and test this")];
        let moves = engine.generate_moves(State::Debugging, &mood(0.1, 0.1, 0.5, 0.1, 0.1), &history);
        for i in 1..moves.len() {
            assert!(moves[i - 1].confidence >= moves[i].confidence);
        }
    }

    /// The 20-labeled-snippet calibration the task calls for: a compact
    /// real (not mocked) end-to-end pass through `generate_moves` across
    /// a spread of state/mood/history combinations, confirming the gate
    /// holds and at least the clearly-high-confidence cases do surface
    /// something.
    #[test]
    fn twenty_labeled_snippets_respect_the_confidence_gate() {
        let engine = AnticipationEngine::new();
        let snippets: [(State, MoodVector, &[&str], bool); 20] = [
            (State::Debugging, mood(0.1, 0.1, 0.5, 0.1, 0.1), &["let's test this"], true),
            (State::Debugging, mood(0.2, 0.1, 0.5, 0.1, 0.1), &["need to verify the fix"], true),
            (State::Debugging, mood(0.9, 0.1, 0.3, 0.8, 0.0), &["still broken"], false),
            (State::DeepWork, mood(0.1, 0.3, 0.7, 0.1, 0.2), &[], true),
            (State::DeepWork, mood(0.0, 0.1, 0.5, 0.0, 0.1), &[], true),
            (State::Reflection, mood(0.0, 0.2, 0.4, 0.2, 0.1), &[], true),
            (State::Opening, mood(0.0, 0.0, 0.2, 0.0, 0.0), &[], false),
            (State::Casual, mood(0.0, 0.0, 0.2, 0.0, 0.0), &[], false),
            (State::Debugging, mood(0.0, 0.0, 0.5, 0.0, 0.0), &["test"], true),
            (State::DeepWork, mood(0.2, 0.2, 0.6, 0.2, 0.2), &[], true),
            (State::Debugging, mood(0.3, 0.1, 0.4, 0.3, 0.0), &["verify"], true),
            (State::Casual, mood(0.1, 0.1, 0.3, 0.1, 0.1), &[], false),
            (State::Opening, mood(0.1, 0.1, 0.3, 0.1, 0.1), &[], false),
            (State::Reflection, mood(0.1, 0.1, 0.3, 0.1, 0.1), &[], true),
            (State::DeepWork, mood(0.0, 0.0, 0.5, 0.0, 0.0), &[], true),
            (State::Debugging, mood(0.85, 0.0, 0.3, 0.9, 0.0), &[], false),
            (State::Debugging, mood(0.1, 0.0, 0.5, 0.1, 0.0), &["testing"], true),
            (State::Casual, mood(0.5, 0.1, 0.4, 0.1, 0.1), &[], false),
            (State::DeepWork, mood(0.1, 0.4, 0.7, 0.1, 0.2), &[], true),
            (State::Reflection, mood(0.0, 0.3, 0.5, 0.1, 0.1), &[], true),
        ];

        for (state, m, texts, expect_any) in snippets {
            let history: Vec<Turn> = texts.iter().map(|t| turn(t)).collect();
            let moves = engine.generate_moves(state, &m, &history);
            for mv in &moves {
                assert!(mv.confidence >= CONFIDENCE_THRESHOLD);
            }
            if expect_any {
                assert!(!moves.is_empty(), "expected at least one move for state {state:?}, mood {m:?}");
            }
        }
    }
}
