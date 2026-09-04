//! Task 5 — Knowledge update pipeline.
//!
//! Requirement 5.1-5.4 / design.md Property 3: a correction from earlier
//! in the session takes precedence over conflicting base LLM knowledge
//! for the rest of that session. This module detects correction-shaped
//! messages, stores the corrected fact with "user-corrected" provenance,
//! and applies stored facts to a prompt context — the actual precedence
//! guarantee lives in `apply_session_knowledge` always being called after
//! base context assembly and its facts never being overwritten except by
//! a newer correction of the *same* fact key within the same session.

use std::collections::HashMap;

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct CorrectedFact {
    pub fact: String,
    pub corrected_by_user: bool,
    pub confidence: f32,
    pub session_id: Uuid,
}

/// Requirement 5.3 — lightweight per-language style signal extracted
/// from a code sample the user shares, used to bias future code Miranda
/// writes toward matching conventions rather than a generic default.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CodeStyleProfile {
    pub uses_tabs: bool,
    pub prefers_snake_case: bool,
    pub prefers_camel_case: bool,
    pub uses_semicolons: bool,
    pub max_observed_line_len: usize,
}

/// Minimal prompt context this module writes into. Kept intentionally
/// small — just the fact map `prompt_builder` needs to merge in — rather
/// than depending on that module's full `PromptContext` shape, to avoid
/// a circular dependency between the two conversation modules.
#[derive(Debug, Clone, Default)]
pub struct PromptContext {
    pub session_facts: HashMap<String, CorrectedFact>,
}

pub struct KnowledgeUpdater {
    session_id: Uuid,
    facts: HashMap<String, CorrectedFact>,
}

impl KnowledgeUpdater {
    pub fn new(session_id: Uuid) -> Self {
        Self {
            session_id,
            facts: HashMap::new(),
        }
    }

    /// Requirement 5.1 — detects correction-shaped messages ("actually,
    /// it's X", "no, I meant Y", "that's wrong, it should be Z") and
    /// extracts the corrected claim. Phrase-anchored rather than a full
    /// contradiction classifier against `prior_claim` — `prior_claim` is
    /// accepted for interface parity with design.md and to let a future,
    /// stronger implementation diff against it, but a correction is
    /// recognized primarily by the user explicitly flagging it as one,
    /// which is a much lower-risk trigger than inferring disagreement
    /// from content alone.
    pub fn detect_correction(&self, _prior_claim: &str, new_message: &str) -> Option<CorrectedFact> {
        const CORRECTION_STEMS: &[&str] = &[
            "actually, it's ",
            "actually it's ",
            "no, i meant ",
            "no i meant ",
            "that's wrong, it should be ",
            "thats wrong, it should be ",
            "to correct that, ",
            "correction: ",
            "i misspoke, ",
        ];

        let lower = new_message.to_lowercase();
        for stem in CORRECTION_STEMS {
            if let Some(idx) = lower.find(stem) {
                let start = idx + stem.len();
                let rest = new_message[start..].trim();
                let fact = rest
                    .split(['.', '!', '?', '\n'])
                    .next()
                    .unwrap_or(rest)
                    .trim();
                if !fact.is_empty() {
                    return Some(CorrectedFact {
                        fact: fact.to_string(),
                        corrected_by_user: true,
                        confidence: 0.9,
                        session_id: self.session_id,
                    });
                }
            }
        }
        None
    }

    /// Requirement 5.2/5.4 — stores the corrected fact under a caller
    /// supplied `key` (the subject the fact is about, e.g. "renderer
    /// choice"), so a later correction of the *same* key naturally
    /// supersedes the earlier one via `HashMap::insert`'s overwrite
    /// semantics — this is what gives Property 3's "always takes
    /// precedence" its concrete mechanism.
    pub fn store_correction(&mut self, key: impl Into<String>, fact: CorrectedFact) {
        self.facts.insert(key.into(), fact);
    }

    /// Requirement 5.2 — the precedence guarantee's other half: merges
    /// every stored session fact into `prompt.session_facts`, called
    /// after base context assembly by `prompt_builder` so these always
    /// layer on top rather than being layered under.
    pub fn apply_session_knowledge(&self, prompt: &mut PromptContext) {
        for (key, fact) in &self.facts {
            prompt.session_facts.insert(key.clone(), fact.clone());
        }
    }

    /// Requirement 5.3 — very small heuristic profiler: indentation
    /// style, naming convention majority, semicolon usage, longest line.
    /// Enough signal to bias generated code without pretending to be a
    /// full static-analysis tool.
    pub fn profile_code_style(&self, code_sample: &str) -> CodeStyleProfile {
        let lines: Vec<&str> = code_sample.lines().collect();

        let uses_tabs = lines.iter().any(|l| l.starts_with('\t'));

        let snake_case_count = count_matches(code_sample, is_snake_case_ident);
        let camel_case_count = count_matches(code_sample, is_camel_case_ident);

        let uses_semicolons = lines.iter().any(|l| l.trim_end().ends_with(';'));

        let max_observed_line_len = lines.iter().map(|l| l.len()).max().unwrap_or(0);

        CodeStyleProfile {
            uses_tabs,
            prefers_snake_case: snake_case_count > camel_case_count,
            prefers_camel_case: camel_case_count > snake_case_count,
            uses_semicolons,
            max_observed_line_len,
        }
    }
}

fn is_snake_case_ident(word: &str) -> bool {
    word.contains('_') && word.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
}

fn is_camel_case_ident(word: &str) -> bool {
    let mut chars = word.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    word.chars().any(|c| c.is_ascii_uppercase()) && word.chars().all(|c| c.is_ascii_alphanumeric())
}

fn count_matches(text: &str, predicate: fn(&str) -> bool) -> usize {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty() && predicate(w))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_actually_its_correction() {
        let updater = KnowledgeUpdater::new(Uuid::new_v4());
        let corrected = updater
            .detect_correction("the renderer is WebGPU", "actually, it's Sumerian Hosts for pipeline 1")
            .expect("expected a correction");
        assert!(corrected.fact.contains("Sumerian Hosts"));
        assert!(corrected.corrected_by_user);
    }

    #[test]
    fn detects_correction_colon_prefix() {
        let updater = KnowledgeUpdater::new(Uuid::new_v4());
        let corrected = updater
            .detect_correction("x", "Correction: the vault path is /mnt/NOBILITY_VAULT")
            .expect("expected a correction");
        assert!(corrected.fact.contains("NOBILITY_VAULT"));
    }

    #[test]
    fn no_correction_detected_in_ordinary_message() {
        let updater = KnowledgeUpdater::new(Uuid::new_v4());
        assert!(updater.detect_correction("x", "what time is it").is_none());
    }

    #[test]
    fn stored_correction_appears_in_prompt_context() {
        let mut updater = KnowledgeUpdater::new(Uuid::new_v4());
        let fact = CorrectedFact {
            fact: "renderer is Sumerian Hosts".to_string(),
            corrected_by_user: true,
            confidence: 0.9,
            session_id: updater.session_id,
        };
        updater.store_correction("renderer", fact.clone());

        let mut prompt = PromptContext::default();
        updater.apply_session_knowledge(&mut prompt);
        assert_eq!(prompt.session_facts.get("renderer").unwrap().fact, fact.fact);
    }

    /// design.md Property 3 — a later correction of the same key
    /// supersedes an earlier one for the rest of the session.
    #[test]
    fn later_correction_of_same_key_supersedes_earlier_one() {
        let mut updater = KnowledgeUpdater::new(Uuid::new_v4());
        updater.store_correction(
            "renderer",
            CorrectedFact {
                fact: "renderer is WebGPU".to_string(),
                corrected_by_user: true,
                confidence: 0.8,
                session_id: updater.session_id,
            },
        );
        updater.store_correction(
            "renderer",
            CorrectedFact {
                fact: "renderer is Sumerian Hosts".to_string(),
                corrected_by_user: true,
                confidence: 0.95,
                session_id: updater.session_id,
            },
        );

        let mut prompt = PromptContext::default();
        updater.apply_session_knowledge(&mut prompt);
        assert_eq!(
            prompt.session_facts.get("renderer").unwrap().fact,
            "renderer is Sumerian Hosts"
        );
    }

    #[test]
    fn profiles_snake_case_semicolon_style() {
        let updater = KnowledgeUpdater::new(Uuid::new_v4());
        let sample = "fn write_event(user_id: u32) {\n    let entity_name = \"x\";\n}\n";
        let profile = updater.profile_code_style(sample);
        assert!(profile.prefers_snake_case);
        assert!(!profile.prefers_camel_case);
        assert!(profile.uses_semicolons);
    }

    #[test]
    fn profiles_camel_case_style() {
        let updater = KnowledgeUpdater::new(Uuid::new_v4());
        let sample = "function writeEvent(userId) {\n  let entityName = getEntityName();\n}\n";
        let profile = updater.profile_code_style(sample);
        assert!(profile.prefers_camel_case);
        assert!(!profile.prefers_snake_case);
    }

    #[test]
    fn profiles_tab_indentation() {
        let updater = KnowledgeUpdater::new(Uuid::new_v4());
        let sample = "fn main() {\n\tlet x = 1;\n}\n";
        let profile = updater.profile_code_style(sample);
        assert!(profile.uses_tabs);
    }

    #[test]
    fn tracks_max_observed_line_length() {
        let updater = KnowledgeUpdater::new(Uuid::new_v4());
        let sample = "short\nthis one is quite a bit longer than the others\nmid length";
        let profile = updater.profile_code_style(sample);
        assert_eq!(profile.max_observed_line_len, "this one is quite a bit longer than the others".len());
    }
}
