//! Task 11 — System prompt injection (LLM integration).
//!
//! Formats a `MemoryRetriever`'s ranked `RetrievedContext` list into a
//! natural-language system-prompt block per Requirement 5.3, so whatever
//! calls the LLM (Bedrock Converse API, per the `pipeline-1-aws-native`
//! rule) can prepend real prior-context grounding without knowing
//! anything about Neo4j/DuckDB internals.
//!
//! Requirement 5.5 is honored at the call site, not here: if the
//! retriever returns an empty list, `format_context_injection` returns
//! `None` so the caller proceeds to LLM inference without injection
//! rather than sending an empty/useless system message.

use super::retriever::RetrievedContext;

/// Builds the natural-language system-prompt injection text from a set of
/// already-ranked retrieved contexts (highest relevance first — the order
/// `MemoryRetriever::retrieve_context` already returns them in).
///
/// Returns `None` when there is nothing to inject (Requirement 5.5).
pub fn format_context_injection(contexts: &[RetrievedContext]) -> Option<String> {
    if contexts.is_empty() {
        return None;
    }

    let mut out = String::from(
        "Relevant memory from past conversations (most relevant first). \
         Use this context naturally if helpful; do not quote it verbatim \
         unless the user asks about it directly:\n",
    );

    for (i, ctx) in contexts.iter().enumerate() {
        out.push_str(&format!(
            "{}. [{}] {} (relevance: {:.2})\n",
            i + 1,
            ctx.mood_state.as_str(),
            ctx.summary,
            ctx.relevance_score
        ));
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::mood_classifier::MoodState;

    fn sample_contexts() -> Vec<RetrievedContext> {
        vec![
            RetrievedContext {
                conversation_id: "conv-1".to_string(),
                relevance_score: 0.92,
                summary: "Discussed Neo4j graph modeling for entity relationships".to_string(),
                mood_state: MoodState::Research,
            },
            RetrievedContext {
                conversation_id: "conv-2".to_string(),
                relevance_score: 0.61,
                summary: "Talked about weekend plans casually".to_string(),
                mood_state: MoodState::Casual,
            },
        ]
    }

    #[test]
    fn empty_contexts_produce_no_injection() {
        assert_eq!(format_context_injection(&[]), None);
    }

    #[test]
    fn formats_numbered_ranked_list_with_mood_and_score() {
        let injection = format_context_injection(&sample_contexts()).expect("should inject");
        assert!(injection.contains("1. [research]"));
        assert!(injection.contains("Neo4j graph modeling"));
        assert!(injection.contains("2. [casual]"));
        assert!(injection.contains("0.92"));
        assert!(injection.contains("0.61"));
    }

    #[test]
    fn preserves_input_order_as_relevance_order() {
        let contexts = sample_contexts();
        let injection = format_context_injection(&contexts).unwrap();
        let pos_1 = injection.find("Neo4j graph modeling").unwrap();
        let pos_2 = injection.find("weekend plans").unwrap();
        assert!(pos_1 < pos_2, "higher-relevance context should appear first");
    }

    /// 10 sample conversations verifying the injected context deterministically
    /// appears in the formatted output (Requirement 5.3/5.4 — this is the
    /// formatting unit test; total pipeline latency is covered in
    /// `memory_integration_tests.rs` against live backends).
    #[test]
    fn ten_sample_conversations_all_produce_findable_injected_text() {
        for i in 0..10 {
            let contexts = vec![RetrievedContext {
                conversation_id: format!("conv-{i}"),
                relevance_score: 0.5 + (i as f32 * 0.01),
                summary: format!("Sample past conversation number {i}"),
                mood_state: MoodState::Curiosity,
            }];
            let injection = format_context_injection(&contexts).expect("should inject");
            assert!(injection.contains(&format!("Sample past conversation number {i}")));
        }
    }

    /// Formatting itself must be fast (<500ms budget is for the full
    /// retrieval+injection pipeline per Task 11's description; formatting
    /// alone is pure string building and should be orders of magnitude
    /// under that).
    #[test]
    fn formatting_latency_is_negligible() {
        let contexts = sample_contexts();
        let start = std::time::Instant::now();
        for _ in 0..1000 {
            let _ = format_context_injection(&contexts);
        }
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() < 500, "1000 formats took {:?}", elapsed);
    }
}
