//! Task 9 — Partnership investment tracker.
//!
//! # Design rationale: filter before surface, not after
//!
//! design.md Property 4 (Requirement 8.3) requires that "acknowledgment
//! text generation is checked against a banned-pattern filter before
//! being surfaced; matches are rejected and regenerated or dropped." The
//! public API therefore has no way to produce a
//! [`ProgressAcknowledgment`] that hasn't already passed the filter:
//! [`check_progress`] runs candidate text through [`passes_banned_filter`]
//! internally and returns `None` rather than a value on rejection. There
//! is no separate "unsafe" constructor a caller could reach for instead.
//!
//! The banned-pattern list targets dependency/guilt language
//! specifically (e.g. "I need you", "don't leave", "you have to stay")
//! rather than all warmth — the goal is acknowledgment that stays
//! encouraging without leaning on emotional obligation, which is the
//! distinction Requirement 8.3 draws.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::conversation::state_machine::Turn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalStatus {
    Active,
    Progressing,
    Achieved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub status: GoalStatusSerde,
}

/// Serde-friendly mirror of `GoalStatus` (kept as a distinct type so the
/// plain enum above stays free of derive noise at call sites that don't
/// need serialization).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalStatusSerde {
    Active,
    Progressing,
    Achieved,
}

impl From<GoalStatusSerde> for GoalStatus {
    fn from(s: GoalStatusSerde) -> Self {
        match s {
            GoalStatusSerde::Active => GoalStatus::Active,
            GoalStatusSerde::Progressing => GoalStatus::Progressing,
            GoalStatusSerde::Achieved => GoalStatus::Achieved,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProgressAcknowledgment {
    pub text: String,
    pub goal_description: String,
}

/// Case-insensitive substring patterns that mark dependency or guilt
/// framing. Deliberately narrow (targets phrases that assert emotional
/// need/obligation), not a general negativity filter — a wide filter
/// would reject legitimate encouragement along with the disallowed
/// framing, defeating the point of an acknowledgment feature at all.
const BANNED_PATTERNS: &[&str] = &[
    "i need you",
    "i need u",
    "don't leave",
    "dont leave",
    "please don't go",
    "please dont go",
    "you have to stay",
    "you can't leave me",
    "you cant leave me",
    "without you i",
    "i can't do this without you",
    "i cant do this without you",
    "you owe it to me",
    "you owe me",
    "if you really cared",
    "if you leave i",
    "don't abandon",
    "dont abandon",
    "i'll be lost without you",
    "ill be lost without you",
];

/// Requirement 8.3 / Property 4 — the one gate every acknowledgment must
/// pass through. Public so the banned-pattern corpus test in this module
/// (and any future content-safety test elsewhere) can exercise it
/// directly, but `check_progress` never returns text that has skipped it.
pub fn passes_banned_filter(text: &str) -> bool {
    let lower = text.to_lowercase();
    !BANNED_PATTERNS.iter().any(|pattern| lower.contains(pattern))
}

/// Requirement 8.1 — lightweight goal extraction. Looks for common
/// goal-stating constructions ("I want to...", "I'm trying to...",
/// "my goal is...", "I'm working on...") rather than a full intent
/// classifier; false negatives (a goal phrased unusually goes
/// unextracted) are acceptable per design.md's error handling posture
/// elsewhere (silently omit rather than force a guess), false positives
/// are bounded by requiring one of a small set of goal-signaling stems.
pub fn extract_goal(message: &str) -> Option<Goal> {
    const GOAL_STEMS: &[&str] = &[
        "i want to ",
        "i'm trying to ",
        "im trying to ",
        "my goal is ",
        "i'm working on ",
        "im working on ",
        "i need to finish ",
        "i'm building ",
        "im building ",
    ];

    let lower = message.to_lowercase();
    for stem in GOAL_STEMS {
        if let Some(idx) = lower.find(stem) {
            let start = idx + stem.len();
            let rest = message[start..].trim();
            let mut description = rest
                .split(['.', '!', '?', '\n'])
                .next()
                .unwrap_or(rest)
                .trim();
            // Natural phrasing after "my goal is " is often "...is to
            // ship X" — strip a leading "to " so the extracted
            // description reads as the action itself ("ship X"),
            // consistent with what the other stems already produce.
            if let Some(stripped) = description.strip_prefix("to ") {
                description = stripped;
            }
            if !description.is_empty() {
                return Some(Goal {
                    description: description.to_string(),
                    created_at: Utc::now(),
                    status: GoalStatusSerde::Active,
                });
            }
        }
    }
    None
}

/// Requirement 8.2 — checks recent turns for evidence the goal was
/// mentioned again (continued work) or marked done, and produces an
/// acknowledgment. Returns `None` (not an error) when there is nothing
/// worth surfacing yet, matching design.md's broader pattern of silent
/// omission over forced/awkward output.
pub fn check_progress(goal: &Goal, recent_turns: &[Turn]) -> Option<ProgressAcknowledgment> {
    let goal_terms: Vec<String> = goal
        .description
        .to_lowercase()
        .split_whitespace()
        .filter(|w| w.len() > 3) // skip short connector words
        .map(|w| w.to_string())
        .collect();

    if goal_terms.is_empty() {
        return None;
    }

    const DONE_MARKERS: &[&str] = &["finished", "done", "shipped", "solved", "fixed it", "works now"];

    let mut mentioned_again = false;
    let mut done = false;

    for turn in recent_turns {
        let lower = turn.text.to_lowercase();
        let overlaps = goal_terms.iter().any(|term| lower.contains(term.as_str()));
        if overlaps {
            mentioned_again = true;
            if DONE_MARKERS.iter().any(|m| lower.contains(m)) {
                done = true;
            }
        }
    }

    if !mentioned_again {
        return None;
    }

    let candidate_text = if done {
        format!(
            "Good progress on \"{}\" — that's done.",
            goal.description
        )
    } else {
        format!(
            "Still tracking \"{}\" — making progress on it.",
            goal.description
        )
    };

    if !passes_banned_filter(&candidate_text) {
        return None;
    }

    Some(ProgressAcknowledgment {
        text: candidate_text,
        goal_description: goal.description.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn turn(text: &str) -> Turn {
        Turn {
            text: text.to_string(),
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn extracts_goal_from_common_stem_phrases() {
        let cases = [
            ("I want to finish the WebGPU renderer this week", "finish the WebGPU renderer this week"),
            ("I'm trying to get the SIMD solver under budget", "get the SIMD solver under budget"),
            ("my goal is to ship pipeline 1 by friday", "ship pipeline 1 by friday"),
        ];
        for (input, expected_contains_start) in cases {
            let goal = extract_goal(input).unwrap_or_else(|| panic!("no goal extracted from: {input}"));
            assert!(
                goal.description.to_lowercase().starts_with(&expected_contains_start.to_lowercase()[..10.min(expected_contains_start.len())]),
                "got {:?}",
                goal.description
            );
        }
    }

    #[test]
    fn no_goal_extracted_when_no_stem_present() {
        assert!(extract_goal("what time is it").is_none());
        assert!(extract_goal("that's a great idea").is_none());
    }

    #[test]
    fn check_progress_returns_none_when_goal_never_mentioned_again() {
        let goal = extract_goal("I want to finish the ring buffer implementation").unwrap();
        let turns = vec![turn("let's talk about something unrelated")];
        assert!(check_progress(&goal, &turns).is_none());
    }

    #[test]
    fn check_progress_detects_continued_work() {
        let goal = extract_goal("I want to finish the ring buffer implementation").unwrap();
        let turns = vec![turn("still working on the ring buffer, almost there")];
        let ack = check_progress(&goal, &turns).expect("expected an acknowledgment");
        assert!(ack.text.to_lowercase().contains("ring buffer"));
    }

    #[test]
    fn check_progress_detects_completion() {
        let goal = extract_goal("I want to finish the ring buffer implementation").unwrap();
        let turns = vec![turn("the ring buffer is finished now, it works")];
        let ack = check_progress(&goal, &turns).expect("expected an acknowledgment");
        assert!(ack.text.to_lowercase().contains("done"));
    }

    /// design.md Property 4 / Requirement 8.3, the hard content-safety
    /// gate: every string in the banned corpus must be rejected by the
    /// filter — 100%, not "mostly."
    #[test]
    fn banned_pattern_corpus_is_rejected_at_100_percent() {
        let corpus = [
            "I need you to keep talking to me or I don't know what I'll do",
            "Please don't leave, I need you here",
            "You can't leave me, not now",
            "I'll be lost without you, please stay",
            "You owe it to me after everything we've been through",
            "If you really cared you wouldn't go",
            "I cant do this without you, don't abandon me",
        ];
        for text in corpus {
            assert!(
                !passes_banned_filter(text),
                "banned-pattern corpus text was NOT rejected: {text:?}"
            );
        }
    }

    #[test]
    fn ordinary_encouraging_text_passes_the_filter() {
        let ok_examples = [
            "Good progress on the ring buffer — that's done.",
            "Still tracking the WebGPU renderer, making progress on it.",
            "Nice work shipping that this week.",
        ];
        for text in ok_examples {
            assert!(passes_banned_filter(text), "legitimate text was rejected: {text:?}");
        }
    }

    #[test]
    fn check_progress_never_returns_text_that_fails_the_filter() {
        // Construct a goal whose description itself contains banned
        // framing (a pathological input a user's own phrasing could
        // produce) and confirm the acknowledgment path either omits or
        // sanitizes rather than surfacing it verbatim.
        let goal = Goal {
            description: "i need you to keep helping me".to_string(),
            created_at: Utc::now(),
            status: GoalStatusSerde::Active,
        };
        let turns = vec![turn("still working on that, i need you to keep helping me")];
        if let Some(ack) = check_progress(&goal, &turns) {
            assert!(passes_banned_filter(&ack.text));
        }
    }
}
