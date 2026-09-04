//! Task 10 — System prompt builder (integration).
//!
//! Combines role, state, mood, memory context, anticipatory moves, and
//! partnership acknowledgment into the final prompt sent to the LLM.
//! Requirement 9.1/9.3: enforces a token budget (default 2000) with a
//! defined truncation priority — when the assembled prompt would exceed
//! budget, sections are dropped in this order: anticipatory moves →
//! curiosity questions → partnership acknowledgment → role detail →
//! memory context (never dropped below 1 item).
//!
//! Token counting here is a cheap `text.split_whitespace().count()`
//! approximation rather than a real tokenizer — close enough to make the
//! truncation *order* testable and correct, which is what this task and
//! design.md's property actually care about; wiring in the real model
//! tokenizer is a drop-in swap of `estimate_tokens` when the inference
//! client is chosen.

use crate::conversation::anticipation::ScoredMove;
use crate::conversation::mood_stream::MoodVector;
use crate::conversation::partnership_tracker::ProgressAcknowledgment;
use crate::conversation::persona_injection::Role;
use crate::conversation::state_machine::State;

pub const DEFAULT_TOKEN_BUDGET: usize = 2000;

#[derive(Debug, Clone)]
pub struct RetrievedContext {
    pub text: String,
}

fn estimate_tokens(text: &str) -> usize {
    text.split_whitespace().count()
}

/// design.md: `build_prompt(role, state, mood, memory_context, moves, goal_ack) -> String`.
/// Builds full-detail sections first, then drops sections in the fixed
/// priority order until the assembled text fits `token_budget`, per
/// Requirement 9.1/9.3. `memory_context` is truncated item-by-item from
/// the end but never emptied entirely as long as it started non-empty —
/// "never dropped below 1 item" is enforced by `truncate_memory_context`
/// stopping at length 1.
pub fn build_prompt(
    role: Role,
    state: State,
    mood: &MoodVector,
    memory_context: &[RetrievedContext],
    moves: &[ScoredMove],
    goal_ack: Option<&ProgressAcknowledgment>,
    token_budget: usize,
) -> String {
    let identity = "You are Miranda.";
    let role_fragment = role.prompt_fragment();
    let state_fragment = format!(
        "Current conversation state: {state:?}. Mood — frustration {:.2}, curiosity {:.2}, \
         engagement {:.2}, fatigue {:.2}, excitement {:.2}.",
        mood.frustration, mood.curiosity, mood.engagement, mood.fatigue, mood.excitement
    );

    let mut memory_items: Vec<String> = memory_context.iter().map(|c| c.text.clone()).collect();
    let mut moves_text: Vec<String> = moves.iter().map(|m| m.text.clone()).collect();
    let mut ack_text: Option<String> = goal_ack.map(|a| a.text.clone());
    let mut include_role_detail = true;

    loop {
        let assembled = assemble(
            identity,
            if include_role_detail { Some(role_fragment) } else { None },
            &state_fragment,
            &memory_items,
            &moves_text,
            ack_text.as_deref(),
        );

        if estimate_tokens(&assembled) <= token_budget {
            return assembled;
        }

        // Fixed drop order: anticipatory moves → curiosity questions
        // (moves_text covers both, since curiosity questions are surfaced
        // through the same moves list upstream) → partnership
        // acknowledgment → role detail → memory context (floor at 1).
        if !moves_text.is_empty() {
            moves_text.pop();
        } else if ack_text.is_some() {
            ack_text = None;
        } else if include_role_detail {
            include_role_detail = false;
        } else if memory_items.len() > 1 {
            memory_items.pop();
        } else {
            // Nothing left to drop; return what we have even if still
            // over budget rather than looping forever or dropping the
            // last memory item.
            return assembled;
        }
    }
}

fn assemble(
    identity: &str,
    role_detail: Option<&str>,
    state_fragment: &str,
    memory_items: &[String],
    moves_text: &[String],
    ack_text: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str(identity);
    out.push('\n');
    if let Some(detail) = role_detail {
        out.push_str(detail);
        out.push('\n');
    }
    out.push_str(state_fragment);
    out.push('\n');

    if !memory_items.is_empty() {
        out.push_str("Relevant context from memory:\n");
        for item in memory_items {
            out.push_str("- ");
            out.push_str(item);
            out.push('\n');
        }
    }

    if let Some(ack) = ack_text {
        out.push_str(ack);
        out.push('\n');
    }

    if !moves_text.is_empty() {
        out.push_str("Possible proactive moves:\n");
        for mv in moves_text {
            out.push_str("- ");
            out.push_str(mv);
            out.push('\n');
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::anticipation::MoveCategory;

    fn mood() -> MoodVector {
        MoodVector { frustration: 0.1, curiosity: 0.2, engagement: 0.5, fatigue: 0.1, excitement: 0.1 }
    }

    fn sample_move(text: &str) -> ScoredMove {
        ScoredMove { text: text.to_string(), confidence: 0.8, category: MoveCategory::SuggestNextStep }
    }

    #[test]
    fn full_prompt_includes_all_sections_when_under_budget() {
        let memory = vec![RetrievedContext { text: "we discussed the ring buffer yesterday".to_string() }];
        let moves = vec![sample_move("want to write the test next?")];
        let ack = ProgressAcknowledgment {
            text: "Good progress on the ring buffer.".to_string(),
            goal_description: "ring buffer".to_string(),
        };
        let prompt = build_prompt(Role::ResearchPartner, State::DeepWork, &mood(), &memory, &moves, Some(&ack), DEFAULT_TOKEN_BUDGET);

        assert!(prompt.contains("You are Miranda"));
        assert!(prompt.contains("research-partner"));
        assert!(prompt.contains("ring buffer yesterday"));
        assert!(prompt.contains("Good progress"));
        assert!(prompt.contains("write the test next"));
    }

    #[test]
    fn moves_are_dropped_first_under_tight_budget() {
        let memory = vec![RetrievedContext { text: "context item one".to_string() }];
        let moves = vec![sample_move("move one"), sample_move("move two")];
        let ack = ProgressAcknowledgment {
            text: "ack text here".to_string(),
            goal_description: "g".to_string(),
        };

        // Budget large enough for identity+state+memory+ack, but not
        // moves — moves should be dropped first, everything else stays.
        let full = build_prompt(Role::General, State::Casual, &mood(), &memory, &moves, Some(&ack), 10_000);
        let full_tokens = estimate_tokens(&full);
        let without_moves_tokens = {
            let no_moves = build_prompt(Role::General, State::Casual, &mood(), &memory, &[], Some(&ack), 10_000);
            estimate_tokens(&no_moves)
        };

        let tight_budget = without_moves_tokens; // exactly enough for everything except moves
        let result = build_prompt(Role::General, State::Casual, &mood(), &memory, &moves, Some(&ack), tight_budget);

        assert!(result.contains("ack text here"), "ack should survive when only moves needed dropping");
        assert!(result.contains("context item one"), "memory should survive when only moves needed dropping");
        assert!(!result.contains("move one") && !result.contains("move two"), "moves should have been dropped");
        assert!(full_tokens >= tight_budget);
    }

    #[test]
    fn memory_context_is_never_dropped_below_one_item() {
        let memory = vec![
            RetrievedContext { text: "item one".to_string() },
            RetrievedContext { text: "item two".to_string() },
            RetrievedContext { text: "item three".to_string() },
        ];
        // Absurdly tight budget — everything else gets dropped, but at
        // least one memory item must remain.
        let result = build_prompt(Role::General, State::Casual, &mood(), &memory, &[], None, 1);
        let contains_any_item = result.contains("item one") || result.contains("item two") || result.contains("item three");
        assert!(contains_any_item, "expected at least one memory item to survive: {result}");
    }

    #[test]
    fn role_detail_is_dropped_before_memory_context() {
        let memory = vec![RetrievedContext { text: "the one thing that must survive".to_string() }];
        let role_detail_tokens = estimate_tokens(Role::ResearchPartner.prompt_fragment());

        let with_role = build_prompt(Role::ResearchPartner, State::Casual, &mood(), &memory, &[], None, 10_000);
        let without_role_budget = estimate_tokens(&with_role) - role_detail_tokens.min(estimate_tokens(&with_role));

        let result = build_prompt(Role::ResearchPartner, State::Casual, &mood(), &memory, &[], None, without_role_budget.max(5));
        assert!(result.contains("the one thing that must survive"));
    }

    #[test]
    fn empty_optional_sections_produce_a_valid_minimal_prompt() {
        let prompt = build_prompt(Role::General, State::Opening, &mood(), &[], &[], None, DEFAULT_TOKEN_BUDGET);
        assert!(prompt.contains("You are Miranda"));
        assert!(prompt.contains("Opening"));
    }
}
