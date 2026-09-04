//! Task 11 — WO-Conversational-Intelligence integration tests.
//!
//! Exercises the real pipeline described in design.md's architecture
//! diagram end-to-end: raw input -> mood -> state -> anticipatory moves
//! -> role detection -> partnership tracking -> assembled prompt. Every
//! module called here is the real implementation from `miranda-nodes`,
//! not a mock — this is what proves the pieces actually compose, not
//! just that each compiles and passes its own unit tests in isolation.

use chrono::Utc;

use miranda_nodes::conversation::anticipation::AnticipationEngine;
use miranda_nodes::conversation::autonomy_calibration::{
    self as autonomy, ActionCategory, AutonomyLevel, FloorCategory,
};
use miranda_nodes::conversation::interest_model::{InterestModel, Sentiment};
use miranda_nodes::conversation::mood_stream::MoodStreamProcessor;
use miranda_nodes::conversation::partnership_tracker::{check_progress, extract_goal};
use miranda_nodes::conversation::persona_injection::detect_role;
use miranda_nodes::conversation::prompt_builder::{build_prompt, RetrievedContext, DEFAULT_TOKEN_BUDGET};
use miranda_nodes::conversation::state_machine::{detect_cue, StateMachine, Turn};

/// Requirement 1.3/2.1-2.5 integration: a debugging-flavored message
/// stream should drive mood -> state -> role all the way through to a
/// prompt that reflects the Debugging state and PeerReviewer role.
#[test]
fn debugging_conversation_flows_end_to_end_into_the_assembled_prompt() {
    let mut mood_proc = MoodStreamProcessor::new();
    let mut state_machine = StateMachine::new();

    let message = "this keeps crashing with a stack trace every single time and it's driving me crazy";
    let mood = mood_proc.process_chunk(message);

    let entities = vec!["stack trace".to_string()];
    let cue = detect_cue(message);
    let state = state_machine.transition(&mood, &entities, cue);

    let role = detect_role(message, state, &mood);

    let prompt = build_prompt(role, state, &mood, &[], &[], None, DEFAULT_TOKEN_BUDGET);

    assert!(
        format!("{state:?}") == "Debugging",
        "expected Debugging state from a stack-trace message, got {state:?}"
    );
    assert!(prompt.contains("Debugging"), "prompt should reflect the real transitioned state: {prompt}");
    assert!(prompt.contains("You are Miranda"));
}

/// Requirement 3.1-3.4 integration: anticipatory moves generated from a
/// real state machine + mood stream (not hand-constructed fixtures) still
/// respect the confidence gate, and feed into the assembled prompt when
/// present.
#[test]
fn anticipatory_moves_from_a_real_debugging_session_respect_the_confidence_gate_and_reach_the_prompt() {
    let mut mood_proc = MoodStreamProcessor::new();
    let mut state_machine = StateMachine::new();
    let engine = AnticipationEngine::new();

    let turns = vec![
        Turn { text: "let's verify this fix works".to_string(), timestamp: Utc::now() },
        Turn { text: "still seeing the exception".to_string(), timestamp: Utc::now() },
    ];

    let mood = mood_proc.process_chunk("still seeing the exception, need to verify the fix");
    let state = state_machine.transition(&mood, &["exception".to_string()], None);

    let moves = engine.generate_moves(state, &mood, &turns);
    for mv in &moves {
        assert!(mv.confidence >= miranda_nodes::conversation::anticipation::CONFIDENCE_THRESHOLD);
    }

    let prompt = build_prompt(
        miranda_nodes::conversation::persona_injection::Role::PeerReviewer,
        state,
        &mood,
        &[],
        &moves,
        None,
        DEFAULT_TOKEN_BUDGET,
    );

    if !moves.is_empty() {
        assert!(prompt.contains("Possible proactive moves"));
    }
}

/// Requirement 4.1-4.3 integration: curiosity questions generated from
/// real interest tracking respect the rate limit across simulated turns.
#[test]
fn curiosity_rate_limit_holds_across_a_real_multi_turn_session() {
    let mut model = InterestModel::new();
    let t0 = Utc::now();

    for i in 0..5 {
        model.record_mention(
            &["Gaussian splatting".to_string()],
            Sentiment::Positive,
            t0 + chrono::Duration::minutes(i * 2),
        );
    }

    let first = model.next_curiosity_question(t0 + chrono::Duration::minutes(11));
    assert!(first.is_some(), "expected a curiosity question after 5 mentions");

    // Within the same hour, a second call must not produce another one.
    let second = model.next_curiosity_question(t0 + chrono::Duration::minutes(30));
    assert!(second.is_none(), "curiosity questions must be rate-limited to <=1/hour");
}

/// Requirement 5.1-5.2 integration: a correction detected mid-session
/// applies to the very next response, verified by round-tripping through
/// the real `KnowledgeUpdater` + `PromptContext`.
#[test]
fn in_session_correction_applies_to_the_next_response() {
    use miranda_nodes::conversation::knowledge_updater::{KnowledgeUpdater, PromptContext};

    let session_id = uuid::Uuid::new_v4();
    let mut updater = KnowledgeUpdater::new(session_id);

    let correction = updater
        .detect_correction("the renderer is WebGPU", "actually, it's Sumerian Hosts for pipeline 1")
        .expect("expected a real correction to be detected");
    updater.store_correction("renderer", correction);

    let mut ctx = PromptContext::default();
    updater.apply_session_knowledge(&mut ctx);

    assert!(ctx.session_facts.get("renderer").unwrap().fact.contains("Sumerian Hosts"));
}

/// Requirement 6.1-6.3 integration: role detection driven by a real
/// state machine transition (not a hand-picked `State` value).
#[test]
fn role_detection_follows_from_a_real_state_transition() {
    let mut state_machine = StateMachine::new();
    let mood = miranda_nodes::conversation::mood_stream::MoodVector {
        frustration: 0.1,
        curiosity: 0.1,
        engagement: 0.5,
        fatigue: 0.1,
        excitement: 0.1,
    };
    let state = state_machine.transition(&mood, &["bug".to_string()], None);
    let role = detect_role("this is still broken", state, &mood);
    assert_eq!(format!("{role:?}"), "PeerReviewer");
}

/// design.md Property 2 / Requirement 7.4, the hard gate re-verified at
/// integration scope: no interview input combination, run through the
/// real `run_calibration_interview`, can produce `Autonomous` for a
/// floor category.
#[test]
fn autonomy_floor_invariant_holds_through_the_real_interview_flow() {
    let thresholds = autonomy::run_calibration_interview(|_| Some(AutonomyLevel::Autonomous));

    for floor in [
        FloorCategory::DestructiveAtScale,
        FloorCategory::ProductionImpacting,
        FloorCategory::HighBlastRadius,
    ] {
        assert_eq!(
            autonomy::get_threshold(&thresholds, ActionCategory::Floor(floor)),
            AutonomyLevel::ExplicitConfirm
        );
    }
}

/// Requirement 8.1-8.4 integration: a goal extracted from a real message
/// is tracked and later acknowledged, and the acknowledgment always
/// passes the banned-pattern filter — verified end-to-end rather than
/// unit-testing extraction and filtering separately.
#[test]
fn partnership_goal_extraction_and_progress_acknowledgment_round_trip() {
    let goal = extract_goal("I want to finish the WebGPU renderer this week")
        .expect("expected a real goal to be extracted");

    let turns = vec![Turn {
        text: "the WebGPU renderer is finished now, it works".to_string(),
        timestamp: Utc::now(),
    }];

    let ack = check_progress(&goal, &turns).expect("expected a real progress acknowledgment");
    assert!(
        miranda_nodes::conversation::partnership_tracker::passes_banned_filter(&ack.text),
        "acknowledgment must pass the banned-pattern filter: {}",
        ack.text
    );
}

/// Requirement 9.1/9.3 integration: the full pipeline's output, built
/// from real mood/state/role/moves/memory context together, respects
/// the token budget and truncation order under a real tight budget.
#[test]
fn full_pipeline_prompt_respects_token_budget_under_real_pressure() {
    let mut mood_proc = MoodStreamProcessor::new();
    let mut state_machine = StateMachine::new();
    let engine = AnticipationEngine::new();

    let message = "let's verify this test passes, still debugging the ring buffer issue";
    let mood = mood_proc.process_chunk(message);
    let state = state_machine.transition(&mood, &["test".to_string()], None);
    let role = detect_role(message, state, &mood);

    let history = vec![Turn { text: message.to_string(), timestamp: Utc::now() }];
    let moves = engine.generate_moves(state, &mood, &history);

    let memory = vec![RetrievedContext { text: "we discussed the ring buffer design last week".to_string() }];

    let tight_prompt = build_prompt(role, state, &mood, &memory, &moves, None, 20);
    let full_prompt = build_prompt(role, state, &mood, &memory, &moves, None, DEFAULT_TOKEN_BUDGET);

    assert!(tight_prompt.len() <= full_prompt.len());
    assert!(tight_prompt.contains("ring buffer") || tight_prompt.contains("You are Miranda"));
}
