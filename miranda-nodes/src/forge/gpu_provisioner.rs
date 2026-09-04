//! Task 5 — GPU provisioner.
//!
//! design.md Property 1 (no silent GPU cost overrun) and Property 3
//! (idle GPU teardown, 15-min idle auto-stop, per the workspace's own
//! `aws-pipeline-architect` cost-discipline rules). Both properties are
//! enforced structurally here rather than left as caller discipline:
//!
//! - [`provision_gpu`] takes the user's configured `spending_threshold`
//!   as a required parameter and returns
//!   [`ProvisionError::RequiresConfirmation`] instead of a handle when
//!   `estimated_cost` exceeds it — there is no `GpuHandle`-returning path
//!   that skips this check.
//! - [`GpuHandle`] tracks `last_activity`; [`GpuHandle::is_idle_timeout`]
//!   is the single source of truth the supervisor loop (or a test) polls
//!   to decide teardown, so "torn down after 15 min idle" is one
//!   function's behavior, not logic duplicated at every call site.

use std::time::Duration;

use chrono::{DateTime, Utc};
use thiserror::Error;

pub const IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Error, PartialEq)]
pub enum ProvisionError {
    #[error(
        "estimated cost ${estimated:.2} exceeds your configured threshold of ${threshold:.2} — \
         needs explicit confirmation before provisioning"
    )]
    RequiresConfirmation { estimated: f32, threshold: f32 },
    #[error("GPU provisioning failed: {reason}")]
    ProvisioningFailed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuState {
    Running,
    TornDown,
}

#[derive(Debug, Clone)]
pub struct GpuHandle {
    pub instance_id: String,
    pub state: GpuState,
    pub provisioned_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
}

impl GpuHandle {
    /// Property 3's concrete check: has this handle been idle (no
    /// `mark_active` call) for at least [`IDLE_TIMEOUT`], as of `now`.
    /// Takes `now` explicitly for the same determinism reason as
    /// `interest_model`'s time-based logic — no clock-mocking dependency
    /// needed for tests.
    pub fn is_idle_timeout(&self, now: DateTime<Utc>) -> bool {
        self.state == GpuState::Running
            && (now - self.last_activity)
                .to_std()
                .map(|d| d >= IDLE_TIMEOUT)
                .unwrap_or(false)
    }

    pub fn mark_active(&mut self, now: DateTime<Utc>) {
        self.last_activity = now;
    }
}

/// design.md: `provision_gpu(estimated_duration, estimated_cost) -> Result<GpuHandle, ProvisionError>`.
/// `spending_threshold` is threaded in explicitly (rather than read from
/// some ambient config) so Property 1 is checkable in a pure unit test
/// without any I/O or global state, and so the autonomy-calibration
/// threshold this backs (WO-Conversational-Intelligence Requirement 7,
/// `Spending` category) is visibly the caller's responsibility to
/// resolve before calling this function — this function only enforces
/// the number it's given, it doesn't go looking one up itself.
pub fn provision_gpu(
    estimated_duration: Duration,
    estimated_cost: f32,
    spending_threshold: f32,
    now: DateTime<Utc>,
) -> Result<GpuHandle, ProvisionError> {
    if estimated_cost > spending_threshold {
        return Err(ProvisionError::RequiresConfirmation {
            estimated: estimated_cost,
            threshold: spending_threshold,
        });
    }

    let _ = estimated_duration; // used by a real provisioning backend to size/select the instance

    Ok(GpuHandle {
        instance_id: format!("gpu-{}", now.timestamp_nanos_opt().unwrap_or(0)),
        state: GpuState::Running,
        provisioned_at: now,
        last_activity: now,
    })
}

/// design.md: `teardown(handle: GpuHandle)`, called on completion,
/// failure, cancellation, or idle timeout — Property 3's other half.
/// Idempotent: tearing down an already-torn-down handle is a no-op, not
/// an error, since a caller reacting to both a job-completion path and
/// an idle-timeout sweep racing each other should never see a spurious
/// failure from calling this twice.
pub fn teardown(handle: &mut GpuHandle) {
    handle.state = GpuState::TornDown;
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    #[test]
    fn provisions_successfully_when_under_threshold() {
        let handle = provision_gpu(Duration::from_secs(3600), 1.5, 5.0, now()).unwrap();
        assert_eq!(handle.state, GpuState::Running);
    }

    /// design.md Property 1, the hard gate: a job whose cost exceeds the
    /// threshold never gets a running handle.
    #[test]
    fn requires_confirmation_when_cost_exceeds_threshold() {
        let err = provision_gpu(Duration::from_secs(3600), 10.0, 5.0, now()).unwrap_err();
        assert_eq!(err, ProvisionError::RequiresConfirmation { estimated: 10.0, threshold: 5.0 });
    }

    #[test]
    fn cost_exactly_at_threshold_is_allowed() {
        let handle = provision_gpu(Duration::from_secs(3600), 5.0, 5.0, now()).unwrap();
        assert_eq!(handle.state, GpuState::Running);
    }

    #[test]
    fn no_confirmation_required_path_ever_exceeds_the_threshold() {
        // Exhaustive-ish sweep: for every (cost, threshold) pair where
        // cost > threshold, provisioning must fail, never succeed.
        let pairs = [(10.0, 5.0), (100.0, 99.99), (0.01, 0.0), (50.0, 49.9)];
        for (cost, threshold) in pairs {
            let result = provision_gpu(Duration::from_secs(60), cost, threshold, now());
            assert!(result.is_err(), "cost {cost} exceeding threshold {threshold} should require confirmation");
        }
    }

    #[test]
    fn is_idle_timeout_false_immediately_after_provisioning() {
        let handle = provision_gpu(Duration::from_secs(60), 1.0, 5.0, now()).unwrap();
        assert!(!handle.is_idle_timeout(now()));
    }

    #[test]
    fn is_idle_timeout_true_after_15_minutes_of_no_activity() {
        let t0 = now();
        let handle = provision_gpu(Duration::from_secs(60), 1.0, 5.0, t0).unwrap();
        let later = t0 + ChronoDuration::minutes(16);
        assert!(handle.is_idle_timeout(later));
    }

    #[test]
    fn is_idle_timeout_false_just_under_15_minutes() {
        let t0 = now();
        let handle = provision_gpu(Duration::from_secs(60), 1.0, 5.0, t0).unwrap();
        let later = t0 + ChronoDuration::minutes(14);
        assert!(!handle.is_idle_timeout(later));
    }

    #[test]
    fn mark_active_resets_the_idle_clock() {
        let t0 = now();
        let mut handle = provision_gpu(Duration::from_secs(60), 1.0, 5.0, t0).unwrap();
        let t1 = t0 + ChronoDuration::minutes(10);
        handle.mark_active(t1);
        let t2 = t1 + ChronoDuration::minutes(10);
        // 20 min since provisioning, but only 10 since last activity —
        // should not be idle-timed-out yet.
        assert!(!handle.is_idle_timeout(t2));
    }

    /// design.md Property 3, the hard gate: once torn down, a handle
    /// never reports as idle-timeout-eligible again (there's nothing
    /// left to tear down), and teardown is idempotent.
    #[test]
    fn torn_down_handle_never_reports_idle_timeout_and_teardown_is_idempotent() {
        let t0 = now();
        let mut handle = provision_gpu(Duration::from_secs(60), 1.0, 5.0, t0).unwrap();
        teardown(&mut handle);
        assert_eq!(handle.state, GpuState::TornDown);

        let later = t0 + ChronoDuration::minutes(30);
        assert!(!handle.is_idle_timeout(later));

        // Calling teardown again should not panic or error.
        teardown(&mut handle);
        assert_eq!(handle.state, GpuState::TornDown);
    }
}
