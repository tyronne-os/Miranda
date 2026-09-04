//! Task 1/9 — Job parser & conversational intent routing.
//!
//! Parses a free-text conversational message into a structured
//! [`JobSpec`] for fine-tuning or merging, or detects that a message is
//! not a Model Forge request at all (`None`), or detects an out-of-scope
//! from-scratch pretraining request (Requirement 6 / Task 9).
//!
//! This is a real deterministic keyword+pattern classifier, not an LLM
//! call — appropriate for a CAT 3 conversational-routing gate that needs
//! to be fast and unit-testable against labeled fixtures. If false
//! negatives on real conversation become a problem later, this is a
//! stable, swappable seam to route through an actual intent-classification
//! model instead.

use std::path::PathBuf;
use std::time::Duration;

/// Which base models a job needs, and where to find them.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelRef {
    pub name: String,
    pub family: String,
    pub size: String,
    pub local_path: Option<PathBuf>,
    pub hf_repo: Option<String>,
}

impl ModelRef {
    /// Build a `ModelRef` from a bare mention like "GLM-9B" or "Nemotron".
    /// Family/size are best-effort parsed from the token itself.
    fn from_mention(mention: &str) -> Self {
        let mention = mention.trim();
        let (family, size) = split_family_size(mention);
        ModelRef {
            name: mention.to_string(),
            family,
            size,
            local_path: None,
            hf_repo: None,
        }
    }
}

/// Splits a mention like "GLM-9B" into ("GLM", "9B"). Falls back to the
/// whole mention as family with an empty size when no `-<digits><unit>`
/// suffix is present.
fn split_family_size(mention: &str) -> (String, String) {
    if let Some(dash_idx) = mention.rfind('-') {
        let (family, size) = mention.split_at(dash_idx);
        let size = &size[1..]; // drop the dash
        if size.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return (family.to_string(), size.to_string());
        }
    }
    (mention.to_string(), String::new())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMethod {
    Slerp,
    Ties,
    Dare,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JobMethod {
    LoraFineTune,
    Merge(MergeMethod),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrainingSpec {
    pub custom_instructions: String,
    pub dataset_ref: Option<PathBuf>,
    pub target_behavior: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JobSpec {
    pub method: JobMethod,
    pub base_models: Vec<ModelRef>,
    pub instructions: Option<TrainingSpec>,
    pub merge_method: Option<MergeMethod>,
    pub estimated_cost: f32,
    pub estimated_duration: Duration,
}

/// Result of routing a conversational message.
#[derive(Debug, Clone, PartialEq)]
pub enum ForgeIntent {
    /// A parsed, ready-to-confirm job.
    Job(JobSpec),
    /// Message implies from-scratch pretraining, which is out of scope
    /// (Requirement 6). Carries a ready-to-speak explanation string.
    ScopeBoundary(String),
}

const FINETUNE_MARKERS: &[&str] = &[
    "fine-tune",
    "finetune",
    "fine tune",
    "lora",
    "adapt",
    "train on",
    "customize",
];

const MERGE_MARKERS: &[&str] = &["merge", "combine", "hybrid", "blend"];

const PRETRAIN_MARKERS: &[&str] = &[
    "from scratch",
    "from-scratch",
    "random weights",
    "random initialization",
    "randomly initialized",
    "pretrain a new model",
    "pretrain from nothing",
    "train a brand new model from scratch",
    "no base model",
    "without a base model",
];

/// Entry point: routes a conversational message into a Model Forge intent,
/// or returns `None` if the message is not a Forge request at all (normal
/// chat turn — Requirement 4.1's "bypass normal chat handling" gate).
pub fn parse_forge_intent(message: &str) -> Option<ForgeIntent> {
    let lower = message.to_lowercase();

    if let Some(reason) = detect_scope_boundary(&lower) {
        return Some(ForgeIntent::ScopeBoundary(reason));
    }

    if contains_any(&lower, MERGE_MARKERS) {
        let method = detect_merge_method(&lower);
        let models = extract_model_mentions(message);
        if models.len() < 2 {
            // A "merge" mention without two identifiable base models isn't
            // a well-formed job yet; treat as not-a-Forge-job so normal
            // chat can ask a clarifying question instead of guessing.
            return None;
        }
        return Some(ForgeIntent::Job(JobSpec {
            method: JobMethod::Merge(method),
            base_models: models,
            instructions: None,
            merge_method: Some(method),
            estimated_cost: 0.0,
            estimated_duration: Duration::from_secs(0),
        }));
    }

    if contains_any(&lower, FINETUNE_MARKERS) {
        let models = extract_model_mentions(message);
        if models.is_empty() {
            return None;
        }
        let instructions = TrainingSpec {
            custom_instructions: message.to_string(),
            dataset_ref: None,
            target_behavior: String::new(),
        };
        return Some(ForgeIntent::Job(JobSpec {
            method: JobMethod::LoraFineTune,
            base_models: models,
            instructions: Some(instructions),
            merge_method: None,
            estimated_cost: 0.0,
            estimated_duration: Duration::from_secs(0),
        }));
    }

    None
}

/// Task 9 — detects a from-scratch pretraining request and returns a
/// ready-to-speak explanation offering fine-tune/merge as the alternative.
/// Checked before fine-tune/merge markers since "train ... from scratch"
/// would otherwise false-positive-match the fine-tune marker "train on".
pub fn detect_scope_boundary(lower_message: &str) -> Option<String> {
    if contains_any(lower_message, PRETRAIN_MARKERS) {
        return Some(
            "Pretraining a model from scratch (random weight initialization on a raw \
             corpus) is research-scale compute and out of scope for a conversational \
             trigger. I can fine-tune a downloaded base model with LoRA using your \
             instructions instead, or merge two existing models into a hybrid — want \
             me to do either of those?"
                .to_string(),
        );
    }
    None
}

fn detect_merge_method(lower_message: &str) -> MergeMethod {
    if lower_message.contains("ties") {
        MergeMethod::Ties
    } else if lower_message.contains("dare") {
        MergeMethod::Dare
    } else {
        MergeMethod::Slerp
    }
}

fn contains_any(haystack: &str, markers: &[&str]) -> bool {
    markers.iter().any(|m| haystack.contains(m))
}

/// Best-effort extraction of model name mentions from a message: scans for
/// tokens matching `<Family><-Size>` patterns (e.g. "GLM-9B", "Phi-3",
/// "Nemotron-70B") or known bare family names.
fn extract_model_mentions(message: &str) -> Vec<ModelRef> {
    const KNOWN_FAMILIES: &[&str] = &[
        "glm", "nemotron", "gemma", "phi", "llama", "mistral", "qwen", "mixtral",
    ];

    let mut found = Vec::new();
    for raw_token in message.split(|c: char| c.is_whitespace() || c == ',' || c == '.') {
        let token = raw_token.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
        if token.is_empty() {
            continue;
        }
        let lower = token.to_lowercase();
        let family_part = lower.split('-').next().unwrap_or(&lower);
        if KNOWN_FAMILIES.contains(&family_part) {
            let candidate = ModelRef::from_mention(token);
            if !found.iter().any(|m: &ModelRef| m.name.eq_ignore_ascii_case(&candidate.name)) {
                found.push(candidate);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    // 15 labeled sample commands per Task 1's requirement: fine-tune,
    // merge, and non-Forge chat.
    const SAMPLES: &[(&str, &str)] = &[
        ("fine-tune GLM-9B with these instructions: be more concise", "finetune"),
        ("Can you finetune Phi-3 to write better SQL?", "finetune"),
        ("I want to fine tune Nemotron-70B on my custom dataset", "finetune"),
        ("please lora adapt Gemma-2B for customer support tone", "finetune"),
        ("customize Mistral-7B to sound more casual", "finetune"),
        ("merge GLM-9B and Nemotron-70B into a hybrid", "merge"),
        ("combine Phi-3 with Mistral-7B using ties", "merge"),
        ("blend Qwen-14B and Gemma-2B with dare", "merge"),
        ("merge Llama-8B and Mixtral-8x7B please", "merge"),
        ("what's the weather like today?", "none"),
        ("tell me a joke", "none"),
        ("how is the fine-tune going?", "progress_or_none"),
        ("what time is it", "none"),
        ("summarize this document for me", "none"),
        ("let's train a brand new model from scratch on raw internet text", "scope_boundary"),
    ];

    #[test]
    fn labeled_samples_route_correctly() {
        for (msg, expected) in SAMPLES {
            let result = parse_forge_intent(msg);
            match *expected {
                "finetune" => match result {
                    Some(ForgeIntent::Job(spec)) => {
                        assert_eq!(spec.method, JobMethod::LoraFineTune, "msg: {msg}");
                        assert!(!spec.base_models.is_empty(), "msg: {msg}");
                    }
                    other => panic!("expected finetune job for {msg:?}, got {other:?}"),
                },
                "merge" => match result {
                    Some(ForgeIntent::Job(spec)) => {
                        assert!(matches!(spec.method, JobMethod::Merge(_)), "msg: {msg}");
                        assert!(spec.base_models.len() >= 2, "msg: {msg}");
                    }
                    other => panic!("expected merge job for {msg:?}, got {other:?}"),
                },
                "scope_boundary" => match result {
                    Some(ForgeIntent::ScopeBoundary(_)) => {}
                    other => panic!("expected scope boundary for {msg:?}, got {other:?}"),
                },
                "none" => {
                    assert!(result.is_none(), "expected None for {msg:?}, got {result:?}");
                }
                "progress_or_none" => {
                    // "how is the fine-tune going" mentions "fine-tune" but has
                    // no identifiable base model, so it must NOT be misparsed
                    // into a bogus job — it should fall through to None so
                    // normal chat / progress-query handling (Task 8) can take it.
                    assert!(result.is_none(), "msg: {msg}, got {result:?}");
                }
                other => panic!("unknown label {other}"),
            }
        }
    }

    #[test]
    fn merge_method_detection() {
        let ForgeIntent::Job(spec) =
            parse_forge_intent("merge GLM-9B and Nemotron-70B using ties").unwrap()
        else {
            panic!("expected job");
        };
        assert_eq!(spec.merge_method, Some(MergeMethod::Ties));

        let ForgeIntent::Job(spec) =
            parse_forge_intent("merge GLM-9B and Nemotron-70B using dare").unwrap()
        else {
            panic!("expected job");
        };
        assert_eq!(spec.merge_method, Some(MergeMethod::Dare));

        let ForgeIntent::Job(spec) =
            parse_forge_intent("merge GLM-9B and Nemotron-70B").unwrap()
        else {
            panic!("expected job");
        };
        assert_eq!(spec.merge_method, Some(MergeMethod::Slerp));
    }

    #[test]
    fn scope_boundary_offers_alternative() {
        let result = parse_forge_intent("I want to pretrain a new model from scratch");
        match result {
            Some(ForgeIntent::ScopeBoundary(msg)) => {
                assert!(msg.contains("fine-tune") || msg.contains("merge"));
            }
            other => panic!("expected scope boundary, got {other:?}"),
        }
    }

    #[test]
    fn family_size_split() {
        let r = ModelRef::from_mention("GLM-9B");
        assert_eq!(r.family, "GLM");
        assert_eq!(r.size, "9B");

        let r = ModelRef::from_mention("Nemotron");
        assert_eq!(r.family, "Nemotron");
        assert_eq!(r.size, "");
    }
}
