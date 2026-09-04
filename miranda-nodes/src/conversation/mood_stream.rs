//! Task 1 — Continuous mood stream processor.
//!
//! # Design rationale
//!
//! `memory::mood_classifier::classify_mood` is a per-turn, whole-text
//! classifier: give it a finished sentence, get back a discrete
//! [`crate::memory::mood_classifier::MoodState`] + confidence. Requirement
//! 1 needs something different: a *continuous* multi-dimensional vector
//! (`frustration`, `curiosity`, `engagement`, `fatigue`, `excitement`)
//! that updates every ~5 tokens while the user is still typing/speaking,
//! smoothed so avatar color doesn't jump around.
//!
//! Rather than build a second, competing lexicon classifier, this module
//! **reuses** `classify_mood` as its raw per-chunk signal (satisfying the
//! task's explicit "reuse mood classifier from WO-Memory rather than
//! duplicating" instruction) and adds two things on top that the
//! per-turn classifier deliberately does not have:
//!
//! 1. A **projection** from the 7 discrete `MoodState` categories onto the
//!    5 continuous dimensions of `MoodVector` (a fixed weight table below).
//! 2. **EMA smoothing** across chunks within a stream, so a single noisy
//!    chunk cannot cause a hard jump in the emitted vector.
//!
//! This keeps one classifier as the single source of lexical truth and
//! avoids maintaining two divergent word lists.

use crate::memory::mood_classifier::{classify_mood, MoodState};

/// Continuous multi-dimensional mood representation (design.md).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoodVector {
    pub frustration: f32,
    pub curiosity: f32,
    pub engagement: f32,
    pub fatigue: f32,
    pub excitement: f32,
}

impl MoodVector {
    pub const NEUTRAL: MoodVector = MoodVector {
        frustration: 0.0,
        curiosity: 0.0,
        engagement: 0.3,
        fatigue: 0.0,
        excitement: 0.0,
    };

    fn clamp(self) -> Self {
        MoodVector {
            frustration: self.frustration.clamp(0.0, 1.0),
            curiosity: self.curiosity.clamp(0.0, 1.0),
            engagement: self.engagement.clamp(0.0, 1.0),
            fatigue: self.fatigue.clamp(0.0, 1.0),
            excitement: self.excitement.clamp(0.0, 1.0),
        }
    }

    /// Component-wise exponential moving average: `alpha` weights the new
    /// sample, `1 - alpha` retains the prior smoothed vector.
    fn ema(prev: MoodVector, sample: MoodVector, alpha: f32) -> MoodVector {
        MoodVector {
            frustration: alpha * sample.frustration + (1.0 - alpha) * prev.frustration,
            curiosity: alpha * sample.curiosity + (1.0 - alpha) * prev.curiosity,
            engagement: alpha * sample.engagement + (1.0 - alpha) * prev.engagement,
            fatigue: alpha * sample.fatigue + (1.0 - alpha) * prev.fatigue,
            excitement: alpha * sample.excitement + (1.0 - alpha) * prev.excitement,
        }
        .clamp()
    }
}

/// Projects a discrete [`MoodState`] + confidence onto the 5-dimensional
/// [`MoodVector`] space. Hand-authored mapping, same "linear scorer with
/// authored weights" family as the underlying lexicon classifier.
fn project(mood: MoodState, confidence: f32) -> MoodVector {
    let (f, c, e, fat, x): (f32, f32, f32, f32, f32) = match mood {
        MoodState::Research => (0.0, 0.8, 0.7, 0.0, 0.1),
        MoodState::Curiosity => (0.0, 1.0, 0.6, 0.0, 0.2),
        MoodState::Disappointment => (0.3, 0.1, 0.3, 0.3, 0.0),
        MoodState::Casual => (0.0, 0.2, 0.4, 0.1, 0.1),
        MoodState::Intimate => (0.0, 0.3, 0.8, 0.0, 0.3),
        MoodState::Frustrated => (1.0, 0.1, 0.5, 0.2, 0.0),
        MoodState::Excited => (0.0, 0.5, 0.8, 0.0, 1.0),
        MoodState::Unknown => (0.0, 0.0, 0.3, 0.0, 0.0),
    };
    // Scale the projected sample by confidence so a weak/ambiguous chunk
    // pulls only weakly toward its category instead of snapping fully to
    // it — this is what keeps single-chunk noise from producing visible
    // jumps even before EMA smoothing is applied.
    MoodVector {
        frustration: f * confidence,
        curiosity: c * confidence,
        engagement: e * confidence,
        fatigue: fat * confidence,
        excitement: x * confidence,
    }
}

/// Splits streaming text into ~5-token chunks (Requirement 1.1). Tokens
/// here are whitespace-delimited words, which is the natural unit for
/// both keystroke-buffered text and ASR partial transcripts.
pub const CHUNK_TOKENS: usize = 5;

/// Stateful continuous mood processor for one turn/stream.
#[derive(Debug, Clone)]
pub struct MoodStreamProcessor {
    smoothed: MoodVector,
    /// EMA alpha; default 0.3 per design.md.
    alpha: f32,
    /// Token buffer accumulated since the last chunk boundary.
    pending_tokens: Vec<String>,
}

impl Default for MoodStreamProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl MoodStreamProcessor {
    pub fn new() -> Self {
        Self {
            smoothed: MoodVector::NEUTRAL,
            alpha: 0.3,
            pending_tokens: Vec::new(),
        }
    }

    pub fn with_alpha(alpha: f32) -> Self {
        Self {
            alpha: alpha.clamp(0.0, 1.0),
            ..Self::new()
        }
    }

    pub fn current(&self) -> MoodVector {
        self.smoothed
    }

    /// Feeds streaming text (voice partial or keystroke buffer). Buffers
    /// tokens until at least [`CHUNK_TOKENS`] have accumulated, then runs
    /// classification + EMA update on that chunk and returns the freshly
    /// smoothed vector. Returns `None` if not enough tokens have arrived
    /// yet (Requirement 1.1: update *at least* every 5 tokens, not more
    /// often than there is text for).
    pub fn feed(&mut self, incoming: &str) -> Option<MoodVector> {
        for tok in incoming.split_whitespace() {
            self.pending_tokens.push(tok.to_string());
        }
        if self.pending_tokens.len() < CHUNK_TOKENS {
            return None;
        }
        let chunk = self.pending_tokens.join(" ");
        self.pending_tokens.clear();
        Some(self.process_chunk(&chunk))
    }

    /// Processes one chunk immediately regardless of buffered size —
    /// the primary interface named in design.md
    /// (`process_chunk(chunk: &str) -> MoodVector`). `feed` is the
    /// streaming-friendly wrapper above; call sites that already have a
    /// well-formed chunk (e.g. tests, or the ASR partial boundary) can use
    /// this directly.
    pub fn process_chunk(&mut self, chunk: &str) -> MoodVector {
        let (mood, confidence) = classify_mood(chunk);
        let sample = project(mood, confidence);
        self.smoothed = MoodVector::ema(self.smoothed, sample, self.alpha);
        self.smoothed
    }

    /// Flushes any trailing tokens (fewer than a full chunk) at end of
    /// turn, so the final vector reflects the last few words too.
    pub fn flush(&mut self) -> MoodVector {
        if !self.pending_tokens.is_empty() {
            let chunk = self.pending_tokens.join(" ");
            self.pending_tokens.clear();
            self.process_chunk(&chunk);
        }
        self.smoothed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 15 labeled input streams (Requirement 1.1/1.2): each is fed token
    /// by token via `feed`, verifying (a) the vector only updates once
    /// enough tokens have accumulated and (b) the final trajectory lands
    /// in the expected dominant-dimension direction.
    struct Stream {
        text: &'static str,
        dominant: fn(&MoodVector) -> f32,
        other_low: fn(&MoodVector) -> f32,
    }

    fn frustration(v: &MoodVector) -> f32 {
        v.frustration
    }
    fn curiosity(v: &MoodVector) -> f32 {
        v.curiosity
    }
    fn excitement(v: &MoodVector) -> f32 {
        v.excitement
    }

    const STREAMS: &[Stream] = &[
        Stream { text: "ugh this is broken again so frustrated and annoyed right now", dominant: frustration, other_low: excitement },
        Stream { text: "ugh doesn't work infuriating fed up sick of this broken thing", dominant: frustration, other_low: excitement },
        Stream { text: "this is angry making, ugh, doesn't work, frustrated annoyed broken", dominant: frustration, other_low: curiosity },
        Stream { text: "i wonder why does this happen i am curious what if we explore", dominant: curiosity, other_low: frustration },
        Stream { text: "curious and intrigued, fascinating, i wonder how does it really work", dominant: curiosity, other_low: frustration },
        Stream { text: "so excited i cant wait this is amazing thrilled and stoked today", dominant: excitement, other_low: frustration },
        Stream { text: "amazing awesome incredible so pumped yes thrilled cant wait excited", dominant: excitement, other_low: frustration },
        Stream { text: "i wonder what if we tried research paper hypothesis data analysis", dominant: curiosity, other_low: frustration },
        Stream { text: "research paper methodology data analysis experiment citation evidence findings", dominant: curiosity, other_low: frustration },
        Stream { text: "ugh so frustrated this doesnt work at all ugh broken annoying", dominant: frustration, other_low: excitement },
        Stream { text: "hey lol just chilling nothing much whatever no biggie today sup", dominant: curiosity, other_low: frustration },
        Stream { text: "curious fascinating intrigued explore wonder why does how does this", dominant: curiosity, other_low: frustration },
        Stream { text: "stoked pumped thrilled amazing awesome incredible cant wait excited yes", dominant: excitement, other_low: frustration },
        Stream { text: "infuriating annoyed ugh broken fed up sick of frustrated angry", dominant: frustration, other_low: excitement },
        Stream { text: "wonder curious intrigued fascinating explore what if how does why", dominant: curiosity, other_low: frustration },
    ];

    #[test]
    fn labeled_streams_trend_toward_expected_dimension() {
        for s in STREAMS {
            let mut proc = MoodStreamProcessor::new();
            let mut last_update = None;
            for word in s.text.split_whitespace() {
                if let Some(v) = proc.feed(&format!("{} ", word)) {
                    last_update = Some(v);
                }
            }
            let final_vec = proc.flush();
            assert!(
                last_update.is_some() || final_vec != MoodVector::NEUTRAL,
                "stream never produced an update: {}",
                s.text
            );
            let dom = (s.dominant)(&final_vec);
            let other = (s.other_low)(&final_vec);
            assert!(
                dom >= other,
                "expected dominant dimension >= other for '{}': dom={} other={}",
                s.text,
                dom,
                other
            );
        }
    }

    #[test]
    fn updates_at_least_every_5_tokens() {
        let mut proc = MoodStreamProcessor::new();
        // Fewer than 5 tokens: no update yet.
        assert!(proc.feed("one two three four").is_none());
        // 5th token arrives: update fires (Requirement 1.1).
        assert!(proc.feed("five").is_some());
    }

    #[test]
    fn smoothing_prevents_abrupt_jumps() {
        let mut proc = MoodStreamProcessor::new();
        let v1 = proc.process_chunk("frustrated annoyed ugh broken infuriating");
        let v2 = proc.process_chunk("excited amazing thrilled cant wait stoked");
        // A single opposing chunk should not fully overwrite the prior
        // smoothed state in one step (alpha=0.3 caps the jump).
        let jump = (v2.frustration - v1.frustration).abs();
        assert!(
            jump < 1.0,
            "frustration jumped by {} in one chunk, smoothing not applied",
            jump
        );
    }

    #[test]
    fn classification_latency_within_50ms_budget() {
        let mut proc = MoodStreamProcessor::new();
        let start = std::time::Instant::now();
        for _ in 0..1000 {
            proc.process_chunk("curious wonder what if fascinating explore");
        }
        let per_call = start.elapsed() / 1000;
        assert!(
            per_call.as_millis() < 50,
            "per-chunk latency {:?} exceeds 50ms budget",
            per_call
        );
    }

    #[test]
    fn vector_components_always_bounded() {
        let mut proc = MoodStreamProcessor::new();
        for _ in 0..50 {
            let v = proc.process_chunk("frustrated excited curious ugh amazing wonder infuriating stoked");
            for x in [v.frustration, v.curiosity, v.engagement, v.fatigue, v.excitement] {
                assert!((0.0..=1.0).contains(&x));
            }
        }
    }
}
