//! Control-plane telemetry hub — JSON snapshots for management tools.
//!
//! [`TelemetryHub`] broadcasts [`TelemetrySnapshot`] messages to every
//! connected management tool at a configurable interval (default 500 ms).
//! The snapshot JSON is produced once and fanned out to all subscribers,
//! so the serialization cost is O(1) regardless of subscriber count.
//!
//! # Circuit-breaker state
//!
//! The circuit breaker is a simple three-state guard around the blendshape
//! bus drain. If the dispatcher drops frames for more than
//! [`CIRCUIT_BREAKER_TRIP_THRESHOLD_MS`] milliseconds, the breaker trips
//! and the hub stops broadcasting stale data. When the dispatcher catches up
//! for [`CIRCUIT_BREAKER_RESET_THRESHOLD_MS`], the breaker resets.
//!
//! Tripped state is surfaced in every telemetry snapshot so the frontend
//! (`THE VANITY`) can show a visual circuit-breaker indicator rather than
//! showing a frozen face with no explanation.

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// Dropped-frame window before the circuit breaker trips.
pub const CIRCUIT_BREAKER_TRIP_THRESHOLD_MS: u64 = 500;

/// Clean-frame window before the circuit breaker resets.
pub const CIRCUIT_BREAKER_RESET_THRESHOLD_MS: u64 = 1_000;

/// Backlog for each telemetry subscriber's channel.
/// 500 ms / 500 ms interval = 1 snapshot in flight. Give it 8 slots of
/// headroom in case a subscriber is briefly slow.
pub const TELEMETRY_CHANNEL_DEPTH: usize = 8;

/// A point-in-time snapshot of the transport and dispatcher state.
///
/// Field names are camelCase to match the existing ace-controller JSON
/// convention used by THE VANITY's node graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetrySnapshot {
    /// Microseconds since harness start.
    pub t_us: u64,

    // --- Dispatcher counters (from miranda-nodes DispatchStats) ---
    pub frames_published: u64,
    pub frames_dropped: u64,
    pub late_frames: u64,
    pub publish_failures: u64,
    pub mean_build_us: f64,
    pub max_build_us: f64,
    pub audio_chunks_consumed: u64,

    // --- Transport counters ---
    pub data_subscribers: usize,
    pub telemetry_subscribers: usize,
    pub frames_broadcast: u64,
    pub frames_dropped_backpressure: u64,

    // --- Circuit breaker ---
    pub circuit_breaker: CircuitBreakerState,
}

/// Three-state circuit breaker reported in every telemetry snapshot.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CircuitBreakerState {
    /// Normal operation — frames are being produced on time.
    Closed,
    /// Frames have been dropping for less than
    /// [`CIRCUIT_BREAKER_TRIP_THRESHOLD_MS`]. Monitoring is indicated;
    /// broadcast continues.
    HalfOpen,
    /// Frames have been dropping for more than the threshold. The renderer
    /// should show a connectivity indicator rather than a frozen face.
    Open,
}

/// A live source of dispatcher counters. Implemented by the wiring layer
/// (T5) so the telemetry hub does not depend on `miranda-nodes` directly —
/// it depends on this trait, which can also be backed by a mock in tests.
pub trait DispatchSource: Send + Sync + 'static {
    fn frames_published(&self) -> u64;
    fn frames_dropped(&self) -> u64;
    fn late_frames(&self) -> u64;
    fn publish_failures(&self) -> u64;
    fn mean_build_us(&self) -> f64;
    fn max_build_us(&self) -> f64;
    fn audio_chunks_consumed(&self) -> u64;
}

/// Telemetry subscriber receive end. The WebSocket handler task reads from
/// this and forwards JSON text frames to the browser.
pub type TelemetryRx = mpsc::Receiver<String>;

struct TelemetrySubscriber {
    tx: mpsc::Sender<String>,
}

/// The control-plane telemetry hub.
///
/// Clone cheaply — every clone shares the same subscriber list and the same
/// circuit-breaker state.
#[derive(Clone)]
pub struct TelemetryHub {
    inner: Arc<TelemetryInner>,
}

struct TelemetryInner {
    subscribers: Mutex<Vec<TelemetrySubscriber>>,
    start_us: u64,
    // Circuit-breaker state.
    breaker: AtomicU64, // encodes CircuitBreakerState as 0/1/2
    // How long (in ticks) we have been in the current state.
    bad_streak_ms: AtomicU64,
    good_streak_ms: AtomicU64,
    tripped: AtomicBool,
}

fn encode_state(s: CircuitBreakerState) -> u64 {
    match s {
        CircuitBreakerState::Closed => 0,
        CircuitBreakerState::HalfOpen => 1,
        CircuitBreakerState::Open => 2,
    }
}

fn decode_state(v: u64) -> CircuitBreakerState {
    match v {
        1 => CircuitBreakerState::HalfOpen,
        2 => CircuitBreakerState::Open,
        _ => CircuitBreakerState::Closed,
    }
}

impl TelemetryHub {
    /// Creates the hub. `start_us` is the harness epoch (microseconds) used
    /// to compute `t_us` in snapshots.
    pub fn new(start_us: u64) -> Self {
        Self {
            inner: Arc::new(TelemetryInner {
                subscribers: Mutex::new(Vec::new()),
                start_us,
                breaker: AtomicU64::new(0),
                bad_streak_ms: AtomicU64::new(0),
                good_streak_ms: AtomicU64::new(0),
                tripped: AtomicBool::new(false),
            }),
        }
    }

    /// Registers a new telemetry subscriber and returns its receive end.
    pub fn subscribe(&self) -> TelemetryRx {
        let (tx, rx) = mpsc::channel(TELEMETRY_CHANNEL_DEPTH);
        self.inner
            .subscribers
            .lock()
            .unwrap()
            .push(TelemetrySubscriber { tx });
        rx
    }

    /// Advances the circuit-breaker state based on the latest drop count.
    ///
    /// Called once per telemetry tick (every `interval_ms`).
    pub fn tick_circuit_breaker(&self, frames_dropped_this_tick: u64, interval_ms: u64) {
        if frames_dropped_this_tick > 0 {
            let bad = self.inner.bad_streak_ms.fetch_add(interval_ms, Ordering::Relaxed)
                + interval_ms;
            self.inner.good_streak_ms.store(0, Ordering::Relaxed);
            let new_state = if bad >= CIRCUIT_BREAKER_TRIP_THRESHOLD_MS {
                self.inner.tripped.store(true, Ordering::Relaxed);
                CircuitBreakerState::Open
            } else {
                CircuitBreakerState::HalfOpen
            };
            self.inner
                .breaker
                .store(encode_state(new_state), Ordering::Relaxed);
        } else {
            self.inner.bad_streak_ms.store(0, Ordering::Relaxed);
            let good = self.inner.good_streak_ms.fetch_add(interval_ms, Ordering::Relaxed)
                + interval_ms;
            if self.inner.tripped.load(Ordering::Relaxed)
                && good >= CIRCUIT_BREAKER_RESET_THRESHOLD_MS
            {
                self.inner.tripped.store(false, Ordering::Relaxed);
                self.inner
                    .breaker
                    .store(encode_state(CircuitBreakerState::Closed), Ordering::Relaxed);
            } else if !self.inner.tripped.load(Ordering::Relaxed) {
                self.inner
                    .breaker
                    .store(encode_state(CircuitBreakerState::Closed), Ordering::Relaxed);
            }
        }
    }

    /// Current circuit-breaker state.
    pub fn circuit_breaker(&self) -> CircuitBreakerState {
        decode_state(self.inner.breaker.load(Ordering::Relaxed))
    }

    /// Builds a snapshot and broadcasts it to all subscribers as a JSON
    /// text frame. Prunes disconnected subscribers.
    pub fn publish_snapshot(
        &self,
        now_us: u64,
        source: &dyn DispatchSource,
        data_subscribers: usize,
        telemetry_subscribers: usize,
        frames_broadcast: u64,
        frames_dropped_bp: u64,
    ) {
        let snap = TelemetrySnapshot {
            t_us: now_us.saturating_sub(self.inner.start_us),
            frames_published: source.frames_published(),
            frames_dropped: source.frames_dropped(),
            late_frames: source.late_frames(),
            publish_failures: source.publish_failures(),
            mean_build_us: source.mean_build_us(),
            max_build_us: source.max_build_us(),
            audio_chunks_consumed: source.audio_chunks_consumed(),
            data_subscribers,
            telemetry_subscribers,
            frames_broadcast,
            frames_dropped_backpressure: frames_dropped_bp,
            circuit_breaker: self.circuit_breaker(),
        };

        let json = match serde_json::to_string(&snap) {
            Ok(s) => s,
            Err(_) => return, // can't fail in practice for this struct
        };

        let mut subs = self.inner.subscribers.lock().unwrap();
        subs.retain(|s| !s.tx.is_closed());
        for s in subs.iter() {
            let _ = s.tx.try_send(json.clone());
        }
    }

    /// Number of active telemetry subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.inner.subscribers.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use std::sync::atomic::AtomicU64 as StdAtomicU64;

    struct MockSource {
        published: u64,
        dropped: u64,
        late: u64,
    }

    impl DispatchSource for MockSource {
        fn frames_published(&self) -> u64 { self.published }
        fn frames_dropped(&self) -> u64 { self.dropped }
        fn late_frames(&self) -> u64 { self.late }
        fn publish_failures(&self) -> u64 { 0 }
        fn mean_build_us(&self) -> f64 { 11.5 }
        fn max_build_us(&self) -> f64 { 250.0 }
        fn audio_chunks_consumed(&self) -> u64 { 100 }
    }

    fn source(published: u64, dropped: u64) -> MockSource {
        MockSource { published, dropped, late: 0 }
    }

    #[tokio::test]
    async fn subscriber_receives_json_snapshot() {
        let hub = TelemetryHub::new(0);
        let mut rx = hub.subscribe();

        hub.publish_snapshot(1_000_000, &source(60, 0), 1, 1, 60, 0);

        let json = rx.recv().await.expect("snapshot expected");
        let parsed: TelemetrySnapshot = serde_json::from_str(&json).expect("valid json");
        assert_eq!(parsed.frames_published, 60);
        assert_eq!(parsed.frames_dropped, 0);
        assert_eq!(parsed.circuit_breaker, CircuitBreakerState::Closed);
    }

    #[tokio::test]
    async fn multiple_subscribers_all_receive_same_snapshot() {
        let hub = TelemetryHub::new(0);
        let mut rx1 = hub.subscribe();
        let mut rx2 = hub.subscribe();

        hub.publish_snapshot(0, &source(1, 0), 0, 2, 1, 0);

        let j1 = rx1.recv().await.unwrap();
        let j2 = rx2.recv().await.unwrap();
        assert_eq!(j1, j2);
    }

    #[test]
    fn circuit_breaker_trips_after_threshold() {
        let hub = TelemetryHub::new(0);
        assert_eq!(hub.circuit_breaker(), CircuitBreakerState::Closed);

        // Feed drops in 100 ms ticks until we hit the trip threshold.
        let ticks = CIRCUIT_BREAKER_TRIP_THRESHOLD_MS / 100;
        for _ in 0..ticks {
            hub.tick_circuit_breaker(1, 100);
        }
        assert_eq!(hub.circuit_breaker(), CircuitBreakerState::Open);
    }

    #[test]
    fn circuit_breaker_enters_half_open_before_tripping() {
        let hub = TelemetryHub::new(0);
        // One tick of drops, below the full threshold.
        hub.tick_circuit_breaker(1, 100);
        assert_eq!(hub.circuit_breaker(), CircuitBreakerState::HalfOpen);
    }

    #[test]
    fn circuit_breaker_resets_after_clean_period() {
        let hub = TelemetryHub::new(0);

        // Trip it.
        let trip_ticks = CIRCUIT_BREAKER_TRIP_THRESHOLD_MS / 100;
        for _ in 0..trip_ticks {
            hub.tick_circuit_breaker(1, 100);
        }
        assert_eq!(hub.circuit_breaker(), CircuitBreakerState::Open);

        // Feed clean ticks until reset.
        let reset_ticks = CIRCUIT_BREAKER_RESET_THRESHOLD_MS / 100;
        for _ in 0..reset_ticks {
            hub.tick_circuit_breaker(0, 100);
        }
        assert_eq!(hub.circuit_breaker(), CircuitBreakerState::Closed);
    }

    #[test]
    fn bad_streak_resets_when_frames_recover() {
        let hub = TelemetryHub::new(0);
        hub.tick_circuit_breaker(1, 100); // half-open
        hub.tick_circuit_breaker(0, 100); // recover — should go back to closed
        assert_eq!(hub.circuit_breaker(), CircuitBreakerState::Closed);
    }

    #[tokio::test]
    async fn disconnected_subscriber_is_pruned() {
        let hub = TelemetryHub::new(0);
        let rx = hub.subscribe();
        assert_eq!(hub.subscriber_count(), 1);
        drop(rx);
        hub.publish_snapshot(0, &source(0, 0), 0, 0, 0, 0);
        assert_eq!(hub.subscriber_count(), 0);
    }

    #[test]
    fn hub_is_send_and_sync() {
        fn assert_ss<T: Send + Sync>() {}
        assert_ss::<TelemetryHub>();
    }

    #[test]
    fn snapshot_json_is_camel_case() {
        let snap = TelemetrySnapshot {
            t_us: 0,
            frames_published: 0,
            frames_dropped: 0,
            late_frames: 0,
            publish_failures: 0,
            mean_build_us: 0.0,
            max_build_us: 0.0,
            audio_chunks_consumed: 0,
            data_subscribers: 0,
            telemetry_subscribers: 0,
            frames_broadcast: 0,
            frames_dropped_backpressure: 0,
            circuit_breaker: CircuitBreakerState::Closed,
        };
        let json = serde_json::to_string(&snap).unwrap();
        // Spot-check a couple of the renamed fields.
        assert!(json.contains("\"framesPublished\""), "expected camelCase framesPublished");
        assert!(json.contains("\"meanBuildUs\""), "expected camelCase meanBuildUs");
        assert!(json.contains("\"circuitBreaker\""), "expected camelCase circuitBreaker");
    }
}
