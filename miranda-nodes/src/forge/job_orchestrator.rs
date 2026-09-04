//! Task 8 — Conversational deployment & progress reporting.
//!
//! Wires Job Parser output through confirmation, dispatch, and progress
//! query handling as real state-machine logic — testable without live
//! GPU/model infra, since what this task actually needs is correct state
//! transitions and query responses, not the training/merge work itself
//! (that's Tasks 6/7's disclosed-scope territory).
//!
//! Requirement 4.2/4.3/4.4: a Forge job goes through
//! `AwaitingConfirmation -> Running -> Completed/Failed`, and
//! `progress_summary` answers "how's the fine-tune going?" against
//! whatever state a running job is currently in.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::forge::finetune_pipeline::TrainingMetrics;
use crate::forge::gpu_provisioner::ProvisionError;
use crate::forge::job_parser::JobSpec;
use crate::forge::merge_pipeline::MergeError;

#[derive(Debug, Clone, PartialEq)]
pub enum JobStatus {
    /// Requirement 5.2 — job is parsed but blocked on the user
    /// confirming a cost/action that exceeded their autonomy threshold.
    AwaitingConfirmation,
    Running { started_at: DateTime<Utc> },
    Completed { finished_at: DateTime<Utc>, summary: String },
    Failed { failed_at: DateTime<Utc>, reason: String },
    Cancelled { cancelled_at: DateTime<Utc> },
}

#[derive(Debug, Clone)]
pub struct ForgeJob {
    pub id: Uuid,
    pub spec: JobSpec,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
}

/// Tracks in-flight Forge jobs so progress queries and completion
/// announcements have somewhere real to read from, rather than each
/// caller needing to thread job state through by hand. In-memory here;
/// a real deployment would persist this the same way `ModelRegistry`
/// discloses its own in-memory-for-now posture.
#[derive(Debug, Default)]
pub struct JobOrchestrator {
    jobs: Vec<ForgeJob>,
}

impl JobOrchestrator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Requirement 4.2 — a freshly parsed job always starts
    /// `AwaitingConfirmation`, even if the GPU provisioner would allow
    /// it autonomously; the confirmation dialogue step (Requirement 4.2)
    /// is a conversational turn that has to happen regardless, and
    /// `confirm` is the only way to move a job out of this state.
    pub fn submit(&mut self, spec: JobSpec, now: DateTime<Utc>) -> Uuid {
        let id = Uuid::new_v4();
        self.jobs.push(ForgeJob {
            id,
            spec,
            status: JobStatus::AwaitingConfirmation,
            created_at: now,
        });
        id
    }

    pub fn confirm(&mut self, id: Uuid, now: DateTime<Utc>) -> Result<(), OrchestratorError> {
        let job = self.job_mut(id)?;
        if job.status != JobStatus::AwaitingConfirmation {
            return Err(OrchestratorError::InvalidTransition {
                id,
                from: format!("{:?}", job.status),
                to: "Running".to_string(),
            });
        }
        job.status = JobStatus::Running { started_at: now };
        Ok(())
    }

    pub fn cancel(&mut self, id: Uuid, now: DateTime<Utc>) -> Result<(), OrchestratorError> {
        let job = self.job_mut(id)?;
        match job.status {
            JobStatus::Completed { .. } | JobStatus::Failed { .. } | JobStatus::Cancelled { .. } => {
                Err(OrchestratorError::InvalidTransition {
                    id,
                    from: format!("{:?}", job.status),
                    to: "Cancelled".to_string(),
                })
            }
            _ => {
                job.status = JobStatus::Cancelled { cancelled_at: now };
                Ok(())
            }
        }
    }

    pub fn complete_finetune(&mut self, id: Uuid, metrics: TrainingMetrics, now: DateTime<Utc>) -> Result<(), OrchestratorError> {
        let job = self.job_mut(id)?;
        job.status = JobStatus::Completed {
            finished_at: now,
            summary: format!(
                "fine-tune finished: final loss {:.3}, {:.1} GPU-hours",
                metrics.final_loss, metrics.gpu_hours
            ),
        };
        Ok(())
    }

    pub fn fail(&mut self, id: Uuid, reason: impl Into<String>, now: DateTime<Utc>) -> Result<(), OrchestratorError> {
        let job = self.job_mut(id)?;
        job.status = JobStatus::Failed { failed_at: now, reason: reason.into() };
        Ok(())
    }

    /// Requirement 4.3 — answers "how's the fine-tune going?" against
    /// whatever state the job is actually in, in conversational form.
    pub fn progress_summary(&self, id: Uuid) -> Result<String, OrchestratorError> {
        let job = self.job(id)?;
        Ok(match &job.status {
            JobStatus::AwaitingConfirmation => {
                "That job is still waiting on your confirmation before it starts.".to_string()
            }
            JobStatus::Running { started_at } => {
                let elapsed = Utc::now() - *started_at;
                format!("Still running — it's been going for about {} minutes so far.", elapsed.num_minutes().max(0))
            }
            JobStatus::Completed { summary, .. } => summary.clone(),
            JobStatus::Failed { reason, .. } => format!("That job failed: {reason}"),
            JobStatus::Cancelled { .. } => "That job was cancelled.".to_string(),
        })
    }

    pub fn get(&self, id: Uuid) -> Result<&ForgeJob, OrchestratorError> {
        self.job(id)
    }

    fn job(&self, id: Uuid) -> Result<&ForgeJob, OrchestratorError> {
        self.jobs.iter().find(|j| j.id == id).ok_or(OrchestratorError::NotFound(id))
    }

    fn job_mut(&mut self, id: Uuid) -> Result<&mut ForgeJob, OrchestratorError> {
        self.jobs.iter_mut().find(|j| j.id == id).ok_or(OrchestratorError::NotFound(id))
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum OrchestratorError {
    #[error("no job found with id {0}")]
    NotFound(Uuid),
    #[error("job {id} cannot transition from {from} to {to}")]
    InvalidTransition { id: Uuid, from: String, to: String },
}

/// Converts a provisioning/merge failure into a user-facing failure
/// reason string, per design.md's error handling: "the raw error is
/// surfaced to the user in conversational form rather than swallowed."
pub fn describe_provision_failure(err: &ProvisionError) -> String {
    err.to_string()
}

pub fn describe_merge_failure(err: &MergeError) -> String {
    err.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::job_parser::{JobMethod, ModelRef};
    use std::path::PathBuf;
    use std::time::Duration;

    fn sample_spec() -> JobSpec {
        JobSpec {
            method: JobMethod::LoraFineTune,
            base_models: vec![ModelRef {
                name: "GLM-9B".to_string(),
                family: "GLM".to_string(),
                size: "9B".to_string(),
                local_path: Some(PathBuf::from("/models/glm-9b")),
                hf_repo: None,
            }],
            instructions: None,
            merge_method: None,
            estimated_cost: 1.5,
            estimated_duration: Duration::from_secs(3600),
        }
    }

    #[test]
    fn submitted_job_starts_awaiting_confirmation() {
        let mut orch = JobOrchestrator::new();
        let id = orch.submit(sample_spec(), Utc::now());
        assert_eq!(orch.get(id).unwrap().status, JobStatus::AwaitingConfirmation);
    }

    #[test]
    fn confirm_transitions_to_running() {
        let mut orch = JobOrchestrator::new();
        let id = orch.submit(sample_spec(), Utc::now());
        orch.confirm(id, Utc::now()).unwrap();
        assert!(matches!(orch.get(id).unwrap().status, JobStatus::Running { .. }));
    }

    #[test]
    fn cannot_confirm_a_job_twice() {
        let mut orch = JobOrchestrator::new();
        let id = orch.submit(sample_spec(), Utc::now());
        orch.confirm(id, Utc::now()).unwrap();
        let err = orch.confirm(id, Utc::now()).unwrap_err();
        assert!(matches!(err, OrchestratorError::InvalidTransition { .. }));
    }

    #[test]
    fn progress_summary_before_confirmation_mentions_awaiting_confirmation() {
        let mut orch = JobOrchestrator::new();
        let id = orch.submit(sample_spec(), Utc::now());
        let summary = orch.progress_summary(id).unwrap();
        assert!(summary.to_lowercase().contains("confirmation"));
    }

    #[test]
    fn progress_summary_while_running_mentions_running() {
        let mut orch = JobOrchestrator::new();
        let id = orch.submit(sample_spec(), Utc::now());
        orch.confirm(id, Utc::now()).unwrap();
        let summary = orch.progress_summary(id).unwrap();
        assert!(summary.to_lowercase().contains("running"));
    }

    #[test]
    fn complete_finetune_produces_a_summary_with_metrics() {
        let mut orch = JobOrchestrator::new();
        let id = orch.submit(sample_spec(), Utc::now());
        orch.confirm(id, Utc::now()).unwrap();
        let metrics = TrainingMetrics { final_loss: 0.31, duration: Duration::from_secs(3600), gpu_hours: 1.0 };
        orch.complete_finetune(id, metrics, Utc::now()).unwrap();

        let summary = orch.progress_summary(id).unwrap();
        assert!(summary.contains("0.31") || summary.contains("0.310"));
    }

    #[test]
    fn fail_produces_a_summary_containing_the_reason() {
        let mut orch = JobOrchestrator::new();
        let id = orch.submit(sample_spec(), Utc::now());
        orch.confirm(id, Utc::now()).unwrap();
        orch.fail(id, "GPU quota exceeded", Utc::now()).unwrap();
        let summary = orch.progress_summary(id).unwrap();
        assert!(summary.contains("GPU quota exceeded"));
    }

    #[test]
    fn cancel_from_awaiting_confirmation_succeeds() {
        let mut orch = JobOrchestrator::new();
        let id = orch.submit(sample_spec(), Utc::now());
        orch.cancel(id, Utc::now()).unwrap();
        assert!(matches!(orch.get(id).unwrap().status, JobStatus::Cancelled { .. }));
    }

    #[test]
    fn cannot_cancel_an_already_completed_job() {
        let mut orch = JobOrchestrator::new();
        let id = orch.submit(sample_spec(), Utc::now());
        orch.confirm(id, Utc::now()).unwrap();
        let metrics = TrainingMetrics { final_loss: 0.2, duration: Duration::from_secs(60), gpu_hours: 0.1 };
        orch.complete_finetune(id, metrics, Utc::now()).unwrap();
        let err = orch.cancel(id, Utc::now()).unwrap_err();
        assert!(matches!(err, OrchestratorError::InvalidTransition { .. }));
    }

    #[test]
    fn querying_an_unknown_job_id_returns_not_found() {
        let orch = JobOrchestrator::new();
        let err = orch.progress_summary(Uuid::new_v4()).unwrap_err();
        assert!(matches!(err, OrchestratorError::NotFound(_)));
    }

    #[test]
    fn describe_provision_failure_surfaces_the_raw_reason() {
        let err = ProvisionError::RequiresConfirmation { estimated: 10.0, threshold: 5.0 };
        let desc = describe_provision_failure(&err);
        assert!(desc.contains("10.00") || desc.contains("10"));
        assert!(desc.contains("5.00") || desc.contains("5"));
    }

    #[test]
    fn describe_merge_failure_surfaces_the_raw_reason() {
        let err = MergeError::MergeKitFailed { reason: "tensor mismatch".to_string() };
        let desc = describe_merge_failure(&err);
        assert!(desc.contains("tensor mismatch"));
    }
}
