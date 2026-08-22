//! Data-plane hub — broadcasts binary frame packets to all connected browsers.
//!
//! # Design
//!
//! [`DataChannelHub`] is the producer-side handle. It is cloned cheaply and
//! handed to whatever thread drains the `MirandaBus` blendshape + kinematic
//! rings. When it has a frame pair ready it calls [`DataChannelHub::broadcast`],
//! which sends the encoded packet to every currently-open subscriber channel.
//!
//! Each browser connection is represented as a [`tokio::sync::mpsc`] sender.
//! The channel is bounded at [`SUBSCRIBER_CHANNEL_DEPTH`] frames. When a slow
//! browser fills its channel the hub drops the frame for that subscriber and
//! increments a counter — backpressure is surfaced as telemetry, never as a
//! stall on the producer thread.
//!
//! # Thread safety
//!
//! `DataChannelHub` wraps its subscriber list in a `Mutex<Vec<…>>`. This lock
//! is taken on two very different timescales:
//!
//! - **Broadcast path** (60 Hz): holds the lock only to clone the sender list,
//!   then releases it before doing any I/O. The actual per-subscriber send
//!   happens after the lock is dropped, so a single slow subscriber cannot
//!   hold the lock while sending.
//! - **Subscribe/unsubscribe path** (human-speed): adds or removes one entry.
//!
//! The lock contention window on the broadcast path is O(n subscribers) for a
//! `clone()` of a `Vec` of `Arc`-wrapped senders — each clone is a pointer
//! copy, not a byte copy of frame data. At 60 Hz with 4 subscribers this is
//! well under 1 µs.

use std::sync::{Arc, Mutex};

use bytes::BytesMut;
use miranda_core::{BlendshapeFrame, KinematicTransformFrame};
use tokio::sync::mpsc;

use crate::frame;

/// Backlog a subscriber's channel can hold before frames are dropped.
///
/// At 60 FPS this is 333 ms of buffer. A subscriber that falls more than
/// 333 ms behind is not keeping up — the face it is rendering is already
/// stale. Dropping rather than growing the backlog is the correct choice:
/// sending stale frames would produce a visible lag-then-lurch artefact
/// worse than a momentary gap.
pub const SUBSCRIBER_CHANNEL_DEPTH: usize = 20;

/// A subscriber's receive end. The WebSocket handler task receives encoded
/// packets from this channel and forwards them to the browser.
pub type SubscriberRx = mpsc::Receiver<bytes::Bytes>;

/// Internal subscriber record.
struct Subscriber {
    tx: mpsc::Sender<bytes::Bytes>,
    /// How many frames were dropped because this subscriber's channel was
    /// full. Read by the telemetry hub.
    dropped: Arc<std::sync::atomic::AtomicU64>,
}

/// The data-plane broadcast hub.
///
/// Clone this to share it across threads — the clone is cheap (one
/// `Arc::clone`) and every clone broadcasts to the same subscriber set.
#[derive(Clone)]
pub struct DataChannelHub {
    inner: Arc<HubInner>,
}

struct HubInner {
    subscribers: Mutex<Vec<Subscriber>>,
    /// Total frames broadcast (to at least one subscriber).
    frames_broadcast: std::sync::atomic::AtomicU64,
    /// Total frames dropped across all subscribers.
    total_dropped: std::sync::atomic::AtomicU64,
}

impl DataChannelHub {
    /// Creates a new hub with no subscribers.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(HubInner {
                subscribers: Mutex::new(Vec::new()),
                frames_broadcast: std::sync::atomic::AtomicU64::new(0),
                total_dropped: std::sync::atomic::AtomicU64::new(0),
            }),
        }
    }

    /// Registers a new subscriber and returns its receive end.
    ///
    /// Called by the axum WebSocket upgrade handler when a new browser
    /// connects. The returned `SubscriberRx` is moved into the per-connection
    /// task.
    pub fn subscribe(&self) -> (SubscriberRx, Arc<std::sync::atomic::AtomicU64>) {
        let dropped = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (tx, rx) = mpsc::channel(SUBSCRIBER_CHANNEL_DEPTH);
        self.inner.subscribers.lock().unwrap().push(Subscriber {
            tx,
            dropped: Arc::clone(&dropped),
        });
        (rx, dropped)
    }

    /// Encodes one frame pair and sends the packet to every subscriber.
    ///
    /// Drops the frame for any subscriber whose channel is full, and removes
    /// subscribers whose channels have been closed (browser disconnected).
    /// Allocation-free in the steady state: the scratch buffer is passed in
    /// by the caller so it can be reused across calls.
    pub fn broadcast(
        &self,
        blend: &BlendshapeFrame,
        kin: &KinematicTransformFrame,
        scratch: &mut BytesMut,
    ) {
        scratch.clear();
        frame::encode(blend, kin, scratch);
        let packet = scratch.clone().freeze();

        // Take a snapshot of the sender list, release the lock, then send.
        let senders: Vec<(mpsc::Sender<bytes::Bytes>, Arc<std::sync::atomic::AtomicU64>)> = {
            self.inner
                .subscribers
                .lock()
                .unwrap()
                .iter()
                .map(|s| (s.tx.clone(), Arc::clone(&s.dropped)))
                .collect()
        };

        if senders.is_empty() {
            return;
        }

        let mut any_closed = false;
        for (tx, dropped) in &senders {
            match tx.try_send(packet.clone()) {
                Ok(_) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    dropped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    self.inner
                        .total_dropped
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    any_closed = true;
                }
            }
        }
        self.inner
            .frames_broadcast
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Prune disconnected subscribers. Only pay the mutex cost when at
        // least one channel is gone.
        if any_closed {
            self.inner
                .subscribers
                .lock()
                .unwrap()
                .retain(|s| !s.tx.is_closed());
        }
    }

    /// Number of currently active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.inner.subscribers.lock().unwrap().len()
    }

    /// Total frames sent to at least one subscriber.
    pub fn frames_broadcast(&self) -> u64 {
        self.inner
            .frames_broadcast
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Total frames dropped across all subscribers due to backpressure.
    pub fn total_dropped(&self) -> u64 {
        self.inner
            .total_dropped
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Default for DataChannelHub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::PACKET_SIZE;
    use miranda_core::{BLENDSHAPE_COUNT, KINEMATIC_JOINT_COUNT, Quaternion};

    fn blend(ts: u64) -> BlendshapeFrame {
        BlendshapeFrame {
            timestamp_us: ts,
            weights: [0.0; BLENDSHAPE_COUNT],
        }
    }

    fn kin(ts: u64) -> KinematicTransformFrame {
        KinematicTransformFrame {
            timestamp_us: ts,
            joints: [Quaternion::IDENTITY; KINEMATIC_JOINT_COUNT],
            head_pitch_deg: 0.0,
            clavicle_rise: 0.0,
            _reserved: [0; 8],
        }
    }

    #[tokio::test]
    async fn subscriber_receives_broadcast() {
        let hub = DataChannelHub::new();
        let (mut rx, _dropped) = hub.subscribe();
        let mut scratch = BytesMut::with_capacity(PACKET_SIZE);

        hub.broadcast(&blend(1), &kin(1), &mut scratch);

        let pkt = rx.recv().await.expect("expected a packet");
        assert_eq!(pkt.len(), PACKET_SIZE);
        assert_eq!(&pkt[..4], b"MRD1");
    }

    #[tokio::test]
    async fn multiple_subscribers_all_receive() {
        let hub = DataChannelHub::new();
        let (mut rx1, _) = hub.subscribe();
        let (mut rx2, _) = hub.subscribe();
        let mut scratch = BytesMut::with_capacity(PACKET_SIZE);

        hub.broadcast(&blend(2), &kin(2), &mut scratch);

        assert!(rx1.recv().await.is_some());
        assert!(rx2.recv().await.is_some());
    }

    #[tokio::test]
    async fn broadcast_with_no_subscribers_is_a_noop() {
        let hub = DataChannelHub::new();
        let mut scratch = BytesMut::with_capacity(PACKET_SIZE);
        // Must not panic.
        hub.broadcast(&blend(0), &kin(0), &mut scratch);
        assert_eq!(hub.frames_broadcast(), 0);
    }

    #[tokio::test]
    async fn full_channel_drops_frame_and_increments_counter() {
        let hub = DataChannelHub::new();
        let (mut rx, dropped) = hub.subscribe();
        let mut scratch = BytesMut::with_capacity(PACKET_SIZE);

        // Fill the channel.
        for i in 0..(SUBSCRIBER_CHANNEL_DEPTH + 5) as u64 {
            hub.broadcast(&blend(i), &kin(i), &mut scratch);
        }

        let drop_count = dropped.load(std::sync::atomic::Ordering::Relaxed);
        assert!(drop_count >= 5, "expected at least 5 drops, got {drop_count}");
        assert!(hub.total_dropped() >= 5);

        // Drain to confirm the channel held SUBSCRIBER_CHANNEL_DEPTH frames.
        let mut received = 0usize;
        while rx.try_recv().is_ok() {
            received += 1;
        }
        assert_eq!(received, SUBSCRIBER_CHANNEL_DEPTH);
    }

    #[tokio::test]
    async fn closed_subscriber_is_pruned_on_next_broadcast() {
        let hub = DataChannelHub::new();
        let (rx, _) = hub.subscribe();
        let mut scratch = BytesMut::with_capacity(PACKET_SIZE);

        assert_eq!(hub.subscriber_count(), 1);
        drop(rx); // close the receive end

        // First broadcast detects the closed channel and prunes.
        hub.broadcast(&blend(0), &kin(0), &mut scratch);
        assert_eq!(hub.subscriber_count(), 0);
    }

    #[tokio::test]
    async fn packet_content_is_decoded_correctly_by_receiver() {
        let hub = DataChannelHub::new();
        let (mut rx, _) = hub.subscribe();
        let mut scratch = BytesMut::with_capacity(PACKET_SIZE);

        let b = BlendshapeFrame {
            timestamp_us: 12_345_678,
            weights: {
                let mut w = [0.0f32; BLENDSHAPE_COUNT];
                w[17] = 0.75;
                w
            },
        };
        let k = KinematicTransformFrame::from_breath(12_345_678, 1.2, 0.5);
        hub.broadcast(&b, &k, &mut scratch);

        let pkt = rx.recv().await.unwrap();
        let (b2, k2) = crate::frame::decode(&pkt).expect("decode failed");
        assert_eq!(b2.timestamp_us, 12_345_678);
        assert!((b2.weights[17] - 0.75).abs() < 1e-6);
        assert!((k2.head_pitch_deg - 1.2).abs() < 1e-6);
        assert!((k2.clavicle_rise - 0.5).abs() < 1e-6);
    }

    #[test]
    fn hub_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DataChannelHub>();
    }
}
