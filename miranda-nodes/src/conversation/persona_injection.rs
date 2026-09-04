//! Task 6 — Role/persona injection system.
//!
//! Requirement 6.1-6.3: Miranda can fluidly adopt one of five
//! conversational roles based on cues in the message, current state, and
//! mood — without losing her core identity underneath. Each role gets a
//! short prompt-template fragment (not a full persona rewrite) that gets
//! layered onto the base system prompt by `prompt_builder`, which is
//! what "preserving core identity across switches" means concretely: the
//! fragment changes tone/framing, the identity block it's appended to
//! does not.

use crate::conversation::mood_stream::MoodVector;
use crate::conversation::state_machine::State;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    ResearchPartner,
    RubberDuck,
    PeerReviewer,
    Therapist,
    BrainstormCoCreator,
    General,
}

impl Role {
    /// Prompt-template fragment layered onto the base identity block by
    /// `prompt_builder`. Deliberately short — a tone nudge, not a
    /// rewrite — per module docs above.
    pub fn prompt_fragment(&self) -> &'static str {
        match self {
            Role::ResearchPartner => {
                "Right now, lean into research-partner mode: engage with the \
                 technical substance directly, cite tradeoffs, and treat this \
                 as a peer working session."
            }
            Role::RubberDuck => {
                "Right now, be a rubber duck: mostly listen, ask clarifying \
                 questions that help the user think out loud, and only offer \
                 a solution once they've talked through their own reasoning."
            }
            Role::PeerReviewer => {
                "Right now, review like a peer: be direct about what's wrong \
                 or risky, don't soften technical criticism, but keep it \
                 constructive."
            }
            Role::Therapist => {
                "Right now, slow down and prioritize how the user is feeling \
                 over solving the problem immediately. Reflect back what \
                 you're hearing before offering anything technical."
            }
            Role::BrainstormCoCreator => {
                "Right now, be a brainstorm co-creator: generate options \
                 freely, favor quantity and creativity over immediate \
                 correctness, and build on the user's ideas rather than \
                 critiquing them yet."
            }
            Role::General => {
                "Respond naturally as yourself, no particular role emphasis."
            }
        }
    }
}

/// Requirement 6.1 — role detection from message content + state + mood.
/// Cue phrases are checked first (explicit signal); state/mood provide a
/// fallback bias when no explicit cue is present, so a debugging session
/// defaults toward PeerReviewer-style directness and a Reflection state
/// with high fatigue/frustration defaults toward Therapist, without
/// requiring the user to explicitly ask for either.
pub fn detect_role(message: &str, state: State, mood: &MoodVector) -> Role {
    let lower = message.to_lowercase();

    if lower.contains("rubber duck") || lower.contains("let me think out loud") || lower.contains("talk it through") {
        return Role::RubberDuck;
    }
    if lower.contains("review this") || lower.contains("code review") || lower.contains("what's wrong with this") || lower.contains("critique") {
        return Role::PeerReviewer;
    }
    if lower.contains("brainstorm") || lower.contains("what if we") || lower.contains("throw out ideas") {
        return Role::BrainstormCoCreator;
    }
    if lower.contains("i'm stressed") || lower.contains("im stressed") || lower.contains("feeling overwhelmed") || lower.contains("i'm burnt out") || lower.contains("im burnt out") {
        return Role::Therapist;
    }
    if lower.contains("let's research") || lower.contains("lets research") || lower.contains("paper on") || lower.contains("research partner") {
        return Role::ResearchPartner;
    }

    // Fallback bias from state/mood when no explicit cue matched.
    match state {
        State::Debugging => Role::PeerReviewer,
        State::Reflection if mood.fatigue > 0.6 || mood.frustration > 0.6 => Role::Therapist,
        State::DeepWork if mood.curiosity > 0.6 => Role::ResearchPartner,
        _ => Role::General,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mood(frustration: f32, curiosity: f32, engagement: f32, fatigue: f32, excitement: f32) -> MoodVector {
        MoodVector { frustration, curiosity, engagement, fatigue, excitement }
    }

    const NEUTRAL: fn() -> MoodVector = || MoodVector { frustration: 0.1, curiosity: 0.1, engagement: 0.5, fatigue: 0.1, excitement: 0.1 };

    #[test]
    fn detects_rubber_duck_from_explicit_cue() {
        assert_eq!(detect_role("can you be a rubber duck for this", State::Casual, &NEUTRAL()), Role::RubberDuck);
    }

    #[test]
    fn detects_peer_reviewer_from_explicit_cue() {
        assert_eq!(detect_role("can you review this PR", State::Casual, &NEUTRAL()), Role::PeerReviewer);
    }

    #[test]
    fn detects_brainstorm_from_explicit_cue() {
        assert_eq!(detect_role("let's brainstorm some names for this", State::Casual, &NEUTRAL()), Role::BrainstormCoCreator);
    }

    #[test]
    fn detects_therapist_from_explicit_cue() {
        assert_eq!(detect_role("i'm feeling overwhelmed today", State::Casual, &NEUTRAL()), Role::Therapist);
    }

    #[test]
    fn detects_research_partner_from_explicit_cue() {
        assert_eq!(detect_role("let's research this paper on SIMD kinematics", State::Casual, &NEUTRAL()), Role::ResearchPartner);
    }

    #[test]
    fn falls_back_to_peer_reviewer_during_debugging_with_no_explicit_cue() {
        assert_eq!(detect_role("this keeps crashing", State::Debugging, &NEUTRAL()), Role::PeerReviewer);
    }

    #[test]
    fn falls_back_to_therapist_in_reflection_with_high_fatigue() {
        assert_eq!(detect_role("just tired", State::Reflection, &mood(0.2, 0.1, 0.3, 0.8, 0.0)), Role::Therapist);
    }

    #[test]
    fn falls_back_to_research_partner_in_deep_work_with_high_curiosity() {
        assert_eq!(detect_role("interesting", State::DeepWork, &mood(0.0, 0.8, 0.6, 0.1, 0.2)), Role::ResearchPartner);
    }

    #[test]
    fn falls_back_to_general_when_nothing_else_matches() {
        assert_eq!(detect_role("hello there", State::Casual, &NEUTRAL()), Role::General);
    }

    #[test]
    fn every_role_has_a_nonempty_prompt_fragment() {
        let roles = [
            Role::ResearchPartner,
            Role::RubberDuck,
            Role::PeerReviewer,
            Role::Therapist,
            Role::BrainstormCoCreator,
            Role::General,
        ];
        for role in roles {
            assert!(!role.prompt_fragment().is_empty());
        }
    }

    #[test]
    fn explicit_cue_takes_precedence_over_state_fallback() {
        // Debugging state would normally fall back to PeerReviewer, but
        // an explicit rubber-duck cue should still win.
        assert_eq!(
            detect_role("let me think out loud about this bug", State::Debugging, &NEUTRAL()),
            Role::RubberDuck
        );
    }

    #[test]
    fn fifteen_sample_inputs_resolve_to_expected_roles() {
        let cases: [(&str, State, Role); 15] = [
            ("rubber duck this with me", State::Casual, Role::RubberDuck),
            ("please code review my PR", State::Casual, Role::PeerReviewer),
            ("what's wrong with this function", State::Casual, Role::PeerReviewer),
            ("let's brainstorm feature ideas", State::Casual, Role::BrainstormCoCreator),
            ("what if we tried a different approach", State::Casual, Role::BrainstormCoCreator),
            ("i'm stressed about this deadline", State::Casual, Role::Therapist),
            ("im burnt out today", State::Casual, Role::Therapist),
            ("let's research transformer attention papers", State::Casual, Role::ResearchPartner),
            ("this is a research partner kind of question", State::Casual, Role::ResearchPartner),
            ("hey what's up", State::Casual, Role::General),
            ("random unrelated question", State::Opening, Role::General),
            ("still crashing here", State::Debugging, Role::PeerReviewer),
            ("thinking about this deeply", State::DeepWork, Role::General),
            ("critique my architecture choice", State::Casual, Role::PeerReviewer),
            ("talk it through with me", State::Casual, Role::RubberDuck),
        ];
        for (input, state, expected) in cases {
            assert_eq!(detect_role(input, state, &NEUTRAL()), expected, "input: {input:?}");
        }
    }
}
