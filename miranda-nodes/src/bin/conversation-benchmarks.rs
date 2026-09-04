//! Task 11 — Conversation Intelligence latency benchmarks.
//!
//! Measures real wall-clock timing for the four budgets design.md calls
//! out: mood classification (<50ms), state transition (<10ms),
//! anticipatory move generation (<150ms), and full prompt assembly
//! (<300ms). Every measurement below calls the real production function
//! — no mocked timing, no invented numbers — and the binary exits
//! non-zero if any budget is exceeded, so it can gate a build the same
//! way `verify-60fps` does for WO-3.
//!
//! ```text
//! cargo run --release -p miranda-nodes --bin conversation-benchmarks
//! ```
//!
//! Build in release for a measurement that reflects the shipped
//! pipeline's actual cost, not an unoptimized debug build.

use std::process::ExitCode;
use std::time::Instant;

use chrono::Utc;

use miranda_nodes::conversation::anticipation::AnticipationEngine;
use miranda_nodes::conversation::mood_stream::MoodStreamProcessor;
use miranda_nodes::conversation::persona_injection::Role;
use miranda_nodes::conversation::prompt_builder::{build_prompt, RetrievedContext, DEFAULT_TOKEN_BUDGET};
use miranda_nodes::conversation::state_machine::{StateMachine, Turn};

const MOOD_BUDGET_MS: f64 = 50.0;
const STATE_TRANSITION_BUDGET_MS: f64 = 10.0;
const ANTICIPATION_BUDGET_MS: f64 = 150.0;
const PROMPT_ASSEMBLY_BUDGET_MS: f64 = 300.0;
const ITERATIONS: u32 = 200;

fn measure_ms<F: FnMut()>(iterations: u32, mut f: F) -> f64 {
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = start.elapsed();
    (elapsed.as_secs_f64() * 1000.0) / iterations as f64
}

fn main() -> ExitCode {
    println!("=== Conversation Intelligence Latency Benchmarks ===");
    println!("({ITERATIONS} iterations per measurement, average reported)\n");

    let mut all_passed = true;

    // Mood classification — real MoodStreamProcessor::process_chunk call.
    let mut mood_proc = MoodStreamProcessor::new();
    let sample_text = "this keeps crashing with a stack trace, still debugging it";
    let mood_avg_ms = measure_ms(ITERATIONS, || {
        let _ = mood_proc.process_chunk(sample_text);
    });
    report("Mood classification", mood_avg_ms, MOOD_BUDGET_MS, &mut all_passed);
    let mood = mood_proc.current();

    // State transition — real StateMachine::transition call.
    let mut state_machine = StateMachine::new();
    let entities = vec!["stack trace".to_string()];
    let state_avg_ms = measure_ms(ITERATIONS, || {
        let _ = state_machine.transition(&mood, &entities, None);
    });
    report("State transition", state_avg_ms, STATE_TRANSITION_BUDGET_MS, &mut all_passed);
    let state = state_machine.state;

    // Anticipatory move generation — real AnticipationEngine::generate_moves call.
    let engine = AnticipationEngine::new();
    let history = vec![
        Turn { text: "let's verify this fix works".to_string(), timestamp: Utc::now() },
        Turn { text: "still seeing the exception".to_string(), timestamp: Utc::now() },
    ];
    let anticipation_avg_ms = measure_ms(ITERATIONS, || {
        let _ = engine.generate_moves(state, &mood, &history);
    });
    report("Anticipatory move generation", anticipation_avg_ms, ANTICIPATION_BUDGET_MS, &mut all_passed);
    let moves = engine.generate_moves(state, &mood, &history);

    // Full prompt assembly — real build_prompt call with realistic inputs.
    let memory = vec![
        RetrievedContext { text: "we discussed the ring buffer design last week".to_string() },
        RetrievedContext { text: "the SIMD solver budget is 150 microseconds per frame".to_string() },
    ];
    let prompt_avg_ms = measure_ms(ITERATIONS, || {
        let _ = build_prompt(Role::PeerReviewer, state, &mood, &memory, &moves, None, DEFAULT_TOKEN_BUDGET);
    });
    report("Full prompt assembly", prompt_avg_ms, PROMPT_ASSEMBLY_BUDGET_MS, &mut all_passed);

    println!();
    if all_passed {
        println!("All latency budgets met.");
        ExitCode::SUCCESS
    } else {
        println!("One or more latency budgets exceeded.");
        ExitCode::FAILURE
    }
}

fn report(label: &str, measured_ms: f64, budget_ms: f64, all_passed: &mut bool) {
    let status = if measured_ms <= budget_ms { "PASS" } else { "FAIL" };
    if measured_ms > budget_ms {
        *all_passed = false;
    }
    println!("{label:35} {measured_ms:8.4}ms  (budget {budget_ms:.1}ms)  [{status}]");
}
