//! Wiring: connects real `miranda-nodes` types to the transport layer's
//! abstraction traits.
//!
//! This module is the only place `miranda-transport` reaches into
//! `miranda-nodes`. Every other module in this crate works through the
//! [`DispatchSource`] trait so it is testable with a mock. Keeping the
//! concrete dependency in one file makes it easy to swap the source (e.g.
//! to a different WO-3 dispatcher implementation) without touching the hub
//! or telemetry logic.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use miranda_nodes::dispatcher::DispatchStats;

use crate::telemetry::DispatchSource;

/// Wraps an `Arc<DispatchStats>` and implements [`DispatchSource`] so the
/// telemetry hub can read live counters from the running dispatcher.
///
/// All counters use `Relaxed` ordering — the same ordering `DispatchStats`
/// itself uses internally. `Relaxed` is correct here because:
///
/// 1. These are independent monotonic counters with no happens-before
///    relationship to enforce between them.
/// 2. The telemetry snapshot is a *sample*, not a synchronization point.
///    A slightly stale count on one field while another is up-to-date does
///    not affect correctness — it just means the snapshot was taken mid-
///    frame, which is unavoidable without a mutex the dispatcher would need
///    to hold across the entire frame build.
pub struct StatsSource {
    stats: Arc<DispatchStats>,
}

impl StatsSource {
    /// Wraps a shared `DispatchStats` reference.
    pub fn new(stats: Arc<DispatchStats>) -> Self {
        Self { stats }
    }
}

impl DispatchSource for StatsSource {
    fn frames_published(&self) -> u64 {
        self.stats.frames_published.load(Ordering::Relaxed)
    }

    fn frames_dropped(&self) -> u64 {
        self.stats.frames_dropped.load(Ordering::Relaxed)
    }

    fn late_frames(&self) -> u64 {
        self.stats.late_frames.load(Ordering::Relaxed)
    }

    fn publish_failures(&self) -> u64 {
        self.stats.publish_failures.load(Ordering::Relaxed)
    }

    fn mean_build_us(&self) -> f64 {
        // `DispatchStats::mean_build_us()` returns `None` before the first
        // frame is published. Map that to `0.0` so the telemetry subscriber
        // always receives a valid JSON number rather than `null`.
        self.stats.mean_build_us().unwrap_or(0.0)
    }

    fn max_build_us(&self) -> f64 {
        self.stats.max_build_us()
    }

    fn audio_chunks_consumed(&self) -> u64 {
        self.stats.audio_chunks_consumed.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    fn make_stats(
        published: u64,
        dropped: u64,
        late: u64,
        failures: u64,
        total_build_ns: u64,
        max_build_ns: u64,
        audio: u64,
    ) -> Arc<DispatchStats> {
        let s = Arc::new(DispatchStats::default());
        s.frames_published.store(published, Ordering::Relaxed);
        s.frames_dropped.store(dropped, Ordering::Relaxed);
        s.late_frames.store(late, Ordering::Relaxed);
        s.publish_failures.store(failures, Ordering::Relaxed);
        s.total_build_ns.store(total_build_ns, Ordering::Relaxed);
        s.max_build_ns.store(max_build_ns, Ordering::Relaxed);
        s.audio_chunks_consumed.store(audio, Ordering::Relaxed);
        s
    }

    #[test]
    fn all_fields_are_forwarded_correctly() {
        let stats = make_stats(1801, 0, 2, 3, 21_570_000, 1_102_301, 360);
        let src = StatsSource::new(stats);

        assert_eq!(src.frames_published(), 1801);
        assert_eq!(src.frames_dropped(), 0);
        assert_eq!(src.late_frames(), 2);
        assert_eq!(src.publish_failures(), 3);
        // mean = total_build_ns / published / 1000 = 21_570_000 / 1801 / 1000 ≈ 11.98 µs
        let mean = src.mean_build_us();
        assert!(
            (mean - 11.977).abs() < 0.01,
            "mean_build_us {mean:.4} not close to expected 11.977"
        );
        // max = max_build_ns / 1000 = 1_102_301 ns = 1102.3 µs
        let max = src.max_build_us();
        assert!(
            (max - 1102.3).abs() < 0.01,
            "max_build_us {max:.4} not close to expected 1102.3"
        );
        assert_eq!(src.audio_chunks_consumed(), 360);
    }

    #[test]
    fn mean_is_zero_before_any_frames() {
        let stats = make_stats(0, 0, 0, 0, 0, 0, 0);
        let src = StatsSource::new(stats);
        assert_eq!(src.mean_build_us(), 0.0);
    }

    #[test]
    fn stats_source_is_send_and_sync() {
        fn assert_ss<T: Send + Sync>() {}
        assert_ss::<StatsSource>();
    }
}
