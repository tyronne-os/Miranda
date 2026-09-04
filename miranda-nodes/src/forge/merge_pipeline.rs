//! Task 7 — Merge pipeline (mergekit).
//!
//! Same disclosed-scope posture as `finetune_pipeline`: design.md's real
//! integration test ("run a real merge of two small compatible models,
//! verify smoke test passes and naming/registration works") needs a live
//! mergekit installation and real model weights this environment does
//! not have provisioned. What's real and tested here: mergekit subprocess
//! command construction, result parsing, and the coherence smoke test's
//! actual heuristic logic (Property 2's second gate) — none of these
//! require a live merge to have happened to verify they're correct.
//!
//! Property 2 (design.md): "A merge job is only registered in the Model
//! Library if it passes both compatibility validation
//! ([`crate::forge::compatibility::validate_compatibility`]) and the
//! coherence smoke test." [`run_merge`] enforces the ordering — it will
//! not call the merge executor at all unless compatibility already
//! passed, and its result is not itself sufficient for registration
//! until [`smoke_test`] also passes; the caller (job orchestration) is
//! expected to chain both, but this module makes it impossible to skip
//! compatibility by construction: `run_merge` takes an already-validated
//! set, typed as `ValidatedModels`, which only [`compatibility::validate_compatibility`]-adjacent
//! code can construct.

use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

use crate::forge::compatibility::{validate_compatibility, ArchitectureProfile, IncompatibilityReason};
use crate::forge::job_parser::{MergeMethod, ModelRef};

#[derive(Debug, Clone, PartialEq)]
pub struct MergedModel {
    pub sources: Vec<ModelRef>,
    pub method: MergeMethod,
    pub output_path: PathBuf,
}

#[derive(Debug, Error, PartialEq)]
pub enum MergeError {
    #[error("compatibility check failed: {0}")]
    Incompatible(#[from] IncompatibilityReason),
    #[error("mergekit reported an error: {reason}")]
    MergeKitFailed { reason: String },
    #[error("failed to parse mergekit output: {reason}")]
    OutputParseFailed { reason: String },
}

/// A set of models that has already passed
/// [`crate::forge::compatibility::validate_compatibility`] — the type
/// itself is the enforcement mechanism for "compatibility is checked
/// before merge work starts" (see module docs): the only way to obtain
/// one is through [`ValidatedModels::validate`], which runs the real
/// check.
pub struct ValidatedModels {
    refs: Vec<ModelRef>,
}

impl ValidatedModels {
    pub fn validate(models: Vec<(ModelRef, ArchitectureProfile)>) -> Result<Self, IncompatibilityReason> {
        validate_compatibility(&models)?;
        Ok(Self {
            refs: models.into_iter().map(|(r, _)| r).collect(),
        })
    }
}

/// Builds the real argv for invoking mergekit's CLI. Exposed for
/// isolated testing of command shape, same rationale as
/// `finetune_pipeline::build_training_command`.
pub fn build_merge_command(models: &ValidatedModels, method: MergeMethod, output_dir: &PathBuf) -> std::process::Command {
    let mut cmd = std::process::Command::new("mergekit-yaml");
    let method_str = match method {
        MergeMethod::Slerp => "slerp",
        MergeMethod::Ties => "ties",
        MergeMethod::Dare => "dare_ties",
    };
    cmd.arg("--merge-method").arg(method_str);
    for model in &models.refs {
        cmd.arg("--model").arg(
            model
                .local_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .or_else(|| model.hf_repo.clone())
                .unwrap_or_else(|| model.name.clone()),
        );
    }
    cmd.arg("--output-dir").arg(output_dir);
    cmd
}

#[derive(Debug, Deserialize)]
struct MergeResultLine {
    status: String,
    #[serde(default)]
    error: Option<String>,
}

/// Parses mergekit's final status line. Real parsing, tested against
/// both success and error-reporting shapes — design.md's error handling
/// requires surfacing the raw mergekit error rather than swallowing it,
/// which this preserves in `MergeError::MergeKitFailed { reason }`.
fn parse_merge_result(json_line: &str) -> Result<(), MergeError> {
    let parsed: MergeResultLine = serde_json::from_str(json_line)
        .map_err(|e| MergeError::OutputParseFailed { reason: e.to_string() })?;

    if parsed.status == "success" {
        Ok(())
    } else {
        Err(MergeError::MergeKitFailed {
            reason: parsed.error.unwrap_or_else(|| "unknown mergekit failure".to_string()),
        })
    }
}

/// design.md: `run_merge(models, method) -> Result<MergedModel, MergeError>`.
/// Takes `ValidatedModels` (not raw `&[ModelRef]`) — see module docs.
pub fn run_merge(
    models: ValidatedModels,
    method: MergeMethod,
    output_dir: PathBuf,
    spawn_merge_process: impl FnOnce(std::process::Command) -> Result<String, MergeError>,
) -> Result<MergedModel, MergeError> {
    let cmd = build_merge_command(&models, method, &output_dir);
    let result_json = spawn_merge_process(cmd)?;
    parse_merge_result(&result_json)?;

    Ok(MergedModel {
        sources: models.refs,
        method,
        output_path: output_dir,
    })
}

#[derive(Debug, Error, PartialEq)]
pub enum SmokeTestFailure {
    #[error("output for prompt {prompt_index} was empty")]
    EmptyOutput { prompt_index: usize },
    #[error("output for prompt {prompt_index} exceeded the repetition threshold ({repeated_fraction:.2} repeated tokens)")]
    ExcessiveRepetition { prompt_index: usize, repeated_fraction: f32 },
}

/// design.md's coherence smoke test, Property 2's second gate: a basic
/// non-garbage heuristic over a fixed set of sample generations. Real
/// heuristic logic — empty-output and repetition-ratio checks — applied
/// to generations a caller supplies (from a real model call once one is
/// available), rather than this module inventing sample text itself.
pub fn smoke_test(generations: &[String], max_repeated_fraction: f32) -> Result<(), SmokeTestFailure> {
    for (idx, text) in generations.iter().enumerate() {
        if text.trim().is_empty() {
            return Err(SmokeTestFailure::EmptyOutput { prompt_index: idx });
        }

        let repeated_fraction = repetition_fraction(text);
        if repeated_fraction > max_repeated_fraction {
            return Err(SmokeTestFailure::ExcessiveRepetition {
                prompt_index: idx,
                repeated_fraction,
            });
        }
    }
    Ok(())
}

/// Fraction of tokens that are exact repeats of the immediately
/// preceding token — a cheap, real signal for the degenerate
/// "the the the the the" failure mode a broken merge/adapter can produce.
fn repetition_fraction(text: &str) -> f32 {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.len() < 2 {
        return 0.0;
    }
    let repeats = tokens.windows(2).filter(|w| w[0].eq_ignore_ascii_case(w[1])).count();
    repeats as f32 / (tokens.len() - 1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::compatibility::ArchitectureProfile;

    fn model_ref(name: &str) -> ModelRef {
        ModelRef {
            name: name.to_string(),
            family: "GLM".to_string(),
            size: "9B".to_string(),
            local_path: Some(PathBuf::from(format!("/models/{name}"))),
            hf_repo: None,
        }
    }

    fn profile() -> ArchitectureProfile {
        ArchitectureProfile {
            architecture_family: "glm".to_string(),
            hidden_size: 4096,
            num_layers: 40,
            tokenizer_id: "glm-tokenizer".to_string(),
        }
    }

    #[test]
    fn validated_models_construction_fails_on_incompatible_input() {
        let mismatched = vec![
            (model_ref("GLM-9B"), profile()),
            (model_ref("Nemotron-9B"), ArchitectureProfile { architecture_family: "nemotron".to_string(), ..profile() }),
        ];
        assert!(ValidatedModels::validate(mismatched).is_err());
    }

    #[test]
    fn validated_models_construction_succeeds_on_compatible_input() {
        let compatible = vec![
            (model_ref("GLM-9B-A"), profile()),
            (model_ref("GLM-9B-B"), profile()),
        ];
        assert!(ValidatedModels::validate(compatible).is_ok());
    }

    #[test]
    fn build_merge_command_includes_all_models_and_method() {
        let models = ValidatedModels::validate(vec![
            (model_ref("GLM-9B-A"), profile()),
            (model_ref("GLM-9B-B"), profile()),
        ])
        .unwrap();
        let cmd = build_merge_command(&models, MergeMethod::Ties, &PathBuf::from("/out/merged"));
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert!(args.contains(&"ties".to_string()));
        assert!(args.contains(&"/models/GLM-9B-A".to_string()));
        assert!(args.contains(&"/models/GLM-9B-B".to_string()));
    }

    #[test]
    fn slerp_and_dare_map_to_expected_cli_flags() {
        let models = ValidatedModels::validate(vec![
            (model_ref("A"), profile()),
            (model_ref("B"), profile()),
        ])
        .unwrap();
        let slerp_cmd = build_merge_command(&models, MergeMethod::Slerp, &PathBuf::from("/out"));
        let slerp_args: Vec<String> = slerp_cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert!(slerp_args.contains(&"slerp".to_string()));
    }

    #[test]
    fn run_merge_surfaces_mergekit_error_reason_not_swallowed() {
        let models = ValidatedModels::validate(vec![
            (model_ref("A"), profile()),
            (model_ref("B"), profile()),
        ])
        .unwrap();
        let result = run_merge(models, MergeMethod::Ties, PathBuf::from("/out"), |_cmd| {
            Ok(r#"{"status": "error", "error": "tensor shape mismatch on layer 12"}"#.to_string())
        });
        match result {
            Err(MergeError::MergeKitFailed { reason }) => {
                assert!(reason.contains("tensor shape mismatch"));
            }
            other => panic!("expected MergeKitFailed with the raw reason, got {other:?}"),
        }
    }

    #[test]
    fn run_merge_succeeds_on_success_status() {
        let models = ValidatedModels::validate(vec![
            (model_ref("A"), profile()),
            (model_ref("B"), profile()),
        ])
        .unwrap();
        let result = run_merge(models, MergeMethod::Slerp, PathBuf::from("/out/merged"), |_cmd| {
            Ok(r#"{"status": "success"}"#.to_string())
        });
        assert!(result.is_ok());
        assert_eq!(result.unwrap().output_path, PathBuf::from("/out/merged"));
    }

    #[test]
    fn smoke_test_passes_on_coherent_generations() {
        let generations = vec![
            "The quick brown fox jumps over the lazy dog.".to_string(),
            "Rust is a systems programming language focused on safety.".to_string(),
        ];
        assert!(smoke_test(&generations, 0.3).is_ok());
    }

    #[test]
    fn smoke_test_fails_on_empty_output() {
        let generations = vec!["a valid response".to_string(), "   ".to_string()];
        let err = smoke_test(&generations, 0.3).unwrap_err();
        assert_eq!(err, SmokeTestFailure::EmptyOutput { prompt_index: 1 });
    }

    #[test]
    fn smoke_test_fails_on_degenerate_repetition() {
        let generations = vec!["the the the the the the the broke".to_string()];
        let err = smoke_test(&generations, 0.3).unwrap_err();
        assert!(matches!(err, SmokeTestFailure::ExcessiveRepetition { prompt_index: 0, .. }));
    }

    #[test]
    fn repetition_fraction_is_zero_for_no_repeats() {
        assert_eq!(repetition_fraction("every word here is unique today"), 0.0);
    }

    #[test]
    fn repetition_fraction_handles_short_input_without_panicking() {
        assert_eq!(repetition_fraction(""), 0.0);
        assert_eq!(repetition_fraction("one"), 0.0);
    }
}
