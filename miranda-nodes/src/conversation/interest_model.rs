//! Task 4 — Interest model & curiosity engine.
//!
//! Requirement 4.1-4.4: tracks how often topics come up and with what
//! sentiment, and periodically surfaces a genuine question about one of
//! them — rate-limited to at most once per hour so it reads as curiosity,
//! not nagging. "Historical topic data" (integrating with WO-Memory) is
//! left as an injection point: this module owns the in-session model and
//! question logic; a caller wires in persisted history by pre-seeding
//! `InterestModel` via `record_mention` before the session starts, rather
//! than this module reaching into the memory crate directly and creating
//! a dependency in the wrong direction (memory has no reason to depend on
//! conversation, and conversation doesn't need to know about DuckDB/Neo4j
//! specifics to track interest).

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sentiment {
    Positive,
    Neutral,
    Negative,
}

#[derive(Debug, Clone)]
pub struct InterestEntry {
    pub topic: String,
    pub frequency: u32,
    pub sentiment: Sentiment,
    pub last_mentioned: DateTime<Utc>,
}

pub struct InterestModel {
    entries: HashMap<String, InterestEntry>,
    last_question_at: Option<DateTime<Utc>>,
    /// Topics whose curiosity questions were dismissed — deprioritized
    /// in `next_curiosity_question` per Requirement 4.3's
    /// dismissal-based deprioritization.
    dismissed_topics: HashMap<String, u32>,
    question_cooldown: Duration,
}

impl Default for InterestModel {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            last_question_at: None,
            dismissed_topics: HashMap::new(),
            question_cooldown: Duration::hours(1),
        }
    }
}

impl InterestModel {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    fn with_cooldown(mut self, cooldown: Duration) -> Self {
        self.question_cooldown = cooldown;
        self
    }

    /// Requirement 4.1 — records that `topics` came up with the given
    /// `sentiment` at `now`, incrementing frequency for topics already
    /// tracked. Takes `now` explicitly (rather than calling `Utc::now()`
    /// internally) so tests can drive time deterministically without a
    /// clock-mocking dependency.
    pub fn record_mention(&mut self, topics: &[String], sentiment: Sentiment, now: DateTime<Utc>) {
        for topic in topics {
            let key = topic.to_lowercase();
            self.entries
                .entry(key)
                .and_modify(|e| {
                    e.frequency += 1;
                    e.sentiment = sentiment;
                    e.last_mentioned = now;
                })
                .or_insert(InterestEntry {
                    topic: topic.clone(),
                    frequency: 1,
                    sentiment,
                    last_mentioned: now,
                });
        }
    }

    /// Requirement 4.3 — a dismissed curiosity question about `topic`
    /// lowers its priority for future selection, without deleting the
    /// interest data itself (frequency/sentiment tracking is unaffected;
    /// only question-worthiness is).
    pub fn record_dismissal(&mut self, topic: &str) {
        *self.dismissed_topics.entry(topic.to_lowercase()).or_insert(0) += 1;
    }

    /// Requirement 4.2/4.3 — picks the most-mentioned, least-dismissed,
    /// positively-or-neutrally-received topic and turns it into a
    /// question, gated to at most one per `question_cooldown` window.
    pub fn next_curiosity_question(&mut self, now: DateTime<Utc>) -> Option<String> {
        if let Some(last) = self.last_question_at {
            if now - last < self.question_cooldown {
                return None;
            }
        }

        let dismissed = &self.dismissed_topics;
        let candidate = self
            .entries
            .values()
            .filter(|e| e.sentiment != Sentiment::Negative)
            .max_by_key(|e| {
                let dismissal_penalty = dismissed.get(&e.topic.to_lowercase()).copied().unwrap_or(0);
                // Frequency biased up, dismissals biased down heavily
                // enough that a couple of dismissals outweighs a modest
                // frequency lead — this is what "deprioritized" means
                // concretely rather than just "tie-broken against."
                (e.frequency as i64) - (dismissal_penalty as i64 * 3)
            })?;

        if dismissed.get(&candidate.topic.to_lowercase()).copied().unwrap_or(0) >= candidate.frequency {
            // Dismissed at least as many times as it's been mentioned —
            // don't ask about it again at all.
            return None;
        }

        let question = format!(
            "You've mentioned {} a few times — what's drawing you to it lately?",
            candidate.topic
        );
        self.last_question_at = Some(now);
        Some(question)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, hour, 0, 0).unwrap()
    }

    #[test]
    fn records_new_topic_with_frequency_one() {
        let mut model = InterestModel::new();
        model.record_mention(&["Rust".to_string()], Sentiment::Positive, t(0));
        assert_eq!(model.entries.get("rust").unwrap().frequency, 1);
    }

    #[test]
    fn repeated_mentions_increment_frequency() {
        let mut model = InterestModel::new();
        model.record_mention(&["Rust".to_string()], Sentiment::Positive, t(0));
        model.record_mention(&["rust".to_string()], Sentiment::Positive, t(1));
        assert_eq!(model.entries.get("rust").unwrap().frequency, 2);
    }

    #[test]
    fn curiosity_question_generated_for_frequent_topic() {
        let mut model = InterestModel::new();
        for h in 0..5 {
            model.record_mention(&["SIMD kinematics".to_string()], Sentiment::Positive, t(h));
        }
        let q = model.next_curiosity_question(t(6));
        assert!(q.is_some());
        assert!(q.unwrap().to_lowercase().contains("simd kinematics"));
    }

    #[test]
    fn no_question_when_no_topics_tracked() {
        let mut model = InterestModel::new();
        assert!(model.next_curiosity_question(t(0)).is_none());
    }

    #[test]
    fn question_rate_limited_to_once_per_cooldown_window() {
        let mut model = InterestModel::new().with_cooldown(Duration::hours(1));
        model.record_mention(&["Neo4j".to_string()], Sentiment::Positive, t(0));
        let first = model.next_curiosity_question(t(1));
        assert!(first.is_some());
        let second = model.next_curiosity_question(t(1) + Duration::minutes(30));
        assert!(second.is_none(), "should be rate-limited within the cooldown window");
    }

    #[test]
    fn question_allowed_again_after_cooldown_elapses() {
        let mut model = InterestModel::new().with_cooldown(Duration::hours(1));
        model.record_mention(&["Neo4j".to_string()], Sentiment::Positive, t(0));
        model.next_curiosity_question(t(1));
        model.record_mention(&["Neo4j".to_string()], Sentiment::Positive, t(2));
        let after_cooldown = model.next_curiosity_question(t(3));
        assert!(after_cooldown.is_some());
    }

    #[test]
    fn dismissed_topic_is_deprioritized_below_a_fresher_topic() {
        let mut model = InterestModel::new();
        model.record_mention(&["DuckDB".to_string()], Sentiment::Positive, t(0));
        model.record_mention(&["DuckDB".to_string()], Sentiment::Positive, t(1));
        model.record_mention(&["Obsidian".to_string()], Sentiment::Positive, t(2));
        model.record_dismissal("DuckDB");
        model.record_dismissal("DuckDB");

        let q = model.next_curiosity_question(t(3)).expect("expected a question");
        assert!(q.to_lowercase().contains("obsidian"), "got: {q}");
    }

    #[test]
    fn heavily_dismissed_topic_is_never_asked_about_again() {
        let mut model = InterestModel::new();
        model.record_mention(&["Podman".to_string()], Sentiment::Neutral, t(0));
        model.record_dismissal("Podman");
        let q = model.next_curiosity_question(t(1));
        assert!(q.is_none());
    }

    #[test]
    fn negative_sentiment_topics_are_excluded_from_curiosity_questions() {
        let mut model = InterestModel::new();
        model.record_mention(&["flaky CI".to_string()], Sentiment::Negative, t(0));
        let q = model.next_curiosity_question(t(1));
        assert!(q.is_none());
    }
}
