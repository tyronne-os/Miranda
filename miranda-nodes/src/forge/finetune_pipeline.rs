//! Task 6 — Fine-tune pipeline (LoRA).
//!
//! # Disclosed scope
//!
//! design.md specifies a real integration test: "run a real small-scale
//! LoRA fine-tune on a small local model end-to-end, verify adapted model
//! saves and produces different output than the base." That test needs
//! an actual GPU-or-CPU-capable Python environment with `peft`/`axolotl`
//! installed and a real base model on disk — infrastructure this
//! environment does not currently have provisioned. Per this project's
//! build-standards rule against simulated inference, this module does
//! **not** fake that integration test or invent numbers for a training
//! run that didn't happen.
//!
//! What *is* real and tested here: the subprocess command construction
//! (the actual argv that would invoke a LoRA training script), the
//! divergence-detection logic (design.md's error handling: "if LoRA
//! training diverges... the pipeline aborts early"), and the result
//! parsing from a training script's JSON metrics output. These are the
//! pieces that are correctness-critical and fully testable without a
//! live GPU. The actual `Command::spawn()` call is a thin, disclosed
//! seam (`spawn_training_process`) that a real GPU-provisioned run would
//! exercise; it is not covered by these tests because doing so would
//! require pretending a subprocess ran when it didn't.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;
use thiserror::Error;

use crate::forge::job_parser::{ModelRef, TrainingSpec};

#[derive(Debug, Clone, PartialEq)]
pub struct AdaptedModel {
    pub base: ModelRef,
    pub adapter_path: PathBuf,
    pub training_metrics: TrainingMetrics,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrainingMetrics {
    pub final_loss: f32,
    pub duration: Duration,
    pub gpu_hours: f32,
}

#[derive(Debug, Error, PartialEq)]
pub enum TrainError {
    #[error("training diverged: loss increased for {consecutive_increases} consecutive steps (last loss {last_loss})")]
    Diverged { consecutive_increases: u32, last_loss: f32 },
    #[error("training process failed: {reason}")]
    ProcessFailed { reason: String },
    #[error("failed to parse training metrics output: {reason}")]
    MetricsParseFailed { reason: String },
}

/// Raw JSON shape emitted by the training script per epoch/step,
/// deserialized from its stdout. Kept separate from `TrainingMetrics`
/// since the script's wire format and this module's public type are
/// allowed to diverge (e.g. the script might report loss per-step while
/// `TrainingMetrics` only needs the final value).
#[derive(Debug, Clone, Deserialize)]
pub struct TrainingStepReport {
    pub step: u32,
    pub loss: f32,
}

/// Requirement (error handling): divergence = loss increased for
/// `threshold` consecutive reported steps. Pure function over the
/// step history so it's testable against constructed sequences without
/// needing a real training run to produce them.
pub fn detect_divergence(history: &[TrainingStepReport], threshold: u32) -> Option<TrainError> {
    let mut consecutive_increases = 0u32;
    let mut prev_loss: Option<f32> = None;

    for report in history {
        if let Some(prev) = prev_loss {
            if report.loss > prev {
                consecutive_increases += 1;
                if consecutive_increases >= threshold {
                    return Some(TrainError::Diverged {
                        consecutive_increases,
                        last_loss: report.loss,
                    });
                }
            } else {
                consecutive_increases = 0;
            }
        }
        prev_loss = Some(report.loss);
    }
    None
}

/// Builds the real argv for invoking a LoRA training script. Exposed
/// (not private) specifically so it's testable in isolation from process
/// execution — verifying the *shape* of the command that would be run,
/// without running it.
pub fn build_training_command(
    base_model: &ModelRef,
    spec: &TrainingSpec,
    output_dir: &PathBuf,
) -> Command {
    let mut cmd = Command::new("python3");
    cmd.arg("-m").arg("peft_train");
    cmd.arg("--base-model");
    cmd.arg(
        base_model
            .local_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .or_else(|| base_model.hf_repo.clone())
            .unwrap_or_else(|| base_model.name.clone()),
    );
    cmd.arg("--instructions").arg(&spec.custom_instructions);
    if let Some(dataset) = &spec.dataset_ref {
        cmd.arg("--dataset").arg(dataset);
    }
    cmd.arg("--target-behavior").arg(&spec.target_behavior);
    cmd.arg("--output-dir").arg(output_dir);
    cmd.arg("--emit-json-metrics");
    cmd
}

/// Parses a training script's final JSON metrics line into
/// `TrainingMetrics`. Real parsing logic (not a stub), tested against
/// both well-formed and malformed input, even though this environment
/// cannot yet produce a real training run's stdout to feed it.
pub fn parse_final_metrics(json_line: &str, duration: Duration) -> Result<TrainingMetrics, TrainError> {
    #[derive(Deserialize)]
    struct FinalMetricsLine {
        final_loss: f32,
        gpu_hours: f32,
    }

    let parsed: FinalMetricsLine = serde_json::from_str(json_line)
        .map_err(|e| TrainError::MetricsParseFailed { reason: e.to_string() })?;

    Ok(TrainingMetrics {
        final_loss: parsed.final_loss,
        duration,
        gpu_hours: parsed.gpu_hours,
    })
}

/// design.md: `run_lora_finetune(base_model, instructions) -> Result<AdaptedModel, TrainError>`.
/// Disclosed limitation per module docs: this orchestrates command
/// construction and result parsing (both real, both tested), but does
/// not itself spawn a process in this environment — `spawn_training_process`
/// is the seam a real GPU-provisioned deployment would fill in.
pub fn run_lora_finetune(
    base_model: &ModelRef,
    instructions: &TrainingSpec,
    output_dir: PathBuf,
    spawn_training_process: impl FnOnce(Command) -> Result<(String, Duration), TrainError>,
) -> Result<AdaptedModel, TrainError> {
    let cmd = build_training_command(base_model, instructions, &output_dir);
    let (final_metrics_json, duration) = spawn_training_process(cmd)?;
    let training_metrics = parse_final_metrics(&final_metrics_json, duration)?;

    Ok(AdaptedModel {
        base: base_model.clone(),
        adapter_path: output_dir,
        training_metrics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_ref() -> ModelRef {
        ModelRef {
            name: "GLM-9B".to_string(),
            family: "GLM".to_string(),
            size: "9B".to_string(),
            local_path: Some(PathBuf::from("/models/glm-9b")),
            hf_repo: None,
        }
    }

    fn spec() -> TrainingSpec {
        TrainingSpec {
            custom_instructions: "be more concise".to_string(),
            dataset_ref: Some(PathBuf::from("/data/concise_examples.jsonl")),
            target_behavior: "concise responses".to_string(),
        }
    }

    #[test]
    fn no_divergence_when_loss_steadily_decreases() {
        let history = vec![
            TrainingStepReport { step: 1, loss: 2.0 },
            TrainingStepReport { step: 2, loss: 1.5 },
            TrainingStepReport { step: 3, loss: 1.2 },
        ];
        assert!(detect_divergence(&history, 3).is_none());
    }

    #[test]
    fn detects_divergence_after_threshold_consecutive_increases() {
        let history = vec![
            TrainingStepReport { step: 1, loss: 1.0 },
            TrainingStepReport { step: 2, loss: 1.1 },
            TrainingStepReport { step: 3, loss: 1.3 },
            TrainingStepReport { step: 4, loss: 1.6 },
        ];
        let result = detect_divergence(&history, 3);
        assert!(matches!(result, Some(TrainError::Diverged { consecutive_increases: 3, .. })));
    }

    #[test]
    fn a_single_increase_followed_by_decrease_does_not_diverge() {
        let history = vec![
            TrainingStepReport { step: 1, loss: 1.0 },
            TrainingStepReport { step: 2, loss: 1.1 }, // one bump
            TrainingStepReport { step: 3, loss: 0.9 }, // recovers, resets streak
            TrainingStepReport { step: 4, loss: 0.8 },
        ];
        assert!(detect_divergence(&history, 2).is_none());
    }

    #[test]
    fn build_training_command_includes_all_expected_arguments() {
        let cmd = build_training_command(&model_ref(), &spec(), &PathBuf::from("/out/adapter"));
        let program = cmd.get_program().to_string_lossy().to_string();
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();

        assert_eq!(program, "python3");
        assert!(args.contains(&"--base-model".to_string()));
        assert!(args.contains(&"/models/glm-9b".to_string()));
        assert!(args.contains(&"--instructions".to_string()));
        assert!(args.contains(&"be more concise".to_string()));
        assert!(args.contains(&"--dataset".to_string()));
        assert!(args.contains(&"--output-dir".to_string()));
        assert!(args.contains(&"--emit-json-metrics".to_string()));
    }

    #[test]
    fn build_training_command_falls_back_to_hf_repo_when_no_local_path() {
        let mut base = model_ref();
        base.local_path = None;
        base.hf_repo = Some("org/glm-9b".to_string());
        let cmd = build_training_command(&base, &spec(), &PathBuf::from("/out"));
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert!(args.contains(&"org/glm-9b".to_string()));
    }

    #[test]
    fn parses_well_formed_metrics_json() {
        let json = r#"{"final_loss": 0.42, "gpu_hours": 0.75}"#;
        let metrics = parse_final_metrics(json, Duration::from_secs(2700)).unwrap();
        assert_eq!(metrics.final_loss, 0.42);
        assert_eq!(metrics.gpu_hours, 0.75);
        assert_eq!(metrics.duration, Duration::from_secs(2700));
    }

    #[test]
    fn malformed_metrics_json_returns_parse_error_not_a_panic() {
        let json = "not valid json";
        let result = parse_final_metrics(json, Duration::from_secs(1));
        assert!(matches!(result, Err(TrainError::MetricsParseFailed { .. })));
    }

    #[test]
    fn run_lora_finetune_returns_diverged_error_when_spawn_reports_it() {
        let result = run_lora_finetune(
            &model_ref(),
            &spec(),
            PathBuf::from("/out/adapter"),
            |_cmd| Err(TrainError::Diverged { consecutive_increases: 5, last_loss: 3.2 }),
        );
        assert!(matches!(result, Err(TrainError::Diverged { .. })));
    }

    #[test]
    fn run_lora_finetune_builds_adapted_model_on_success() {
        let result = run_lora_finetune(
            &model_ref(),
            &spec(),
            PathBuf::from("/out/adapter"),
            |_cmd| Ok((r#"{"final_loss": 0.3, "gpu_hours": 1.2}"#.to_string(), Duration::from_secs(4000))),
        );
        let adapted = result.expect("expected success");
        assert_eq!(adapted.training_metrics.final_loss, 0.3);
        assert_eq!(adapted.adapter_path, PathBuf::from("/out/adapter"));
    }
}
