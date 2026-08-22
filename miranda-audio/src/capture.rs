//! WO-2 T5 — native microphone capture via `cpal`, Pipeline 2's mic ingress.
//!
//! This is deliberately the *only* mic-capture path in the whole project
//! that touches real audio hardware. Pipeline 1 (WO-2 T1-T4) captures in
//! the browser instead, because the AWS deployment target (a headless
//! EC2 instance) has no sound card and `cpal::default_input_device()`
//! would panic there — see the "headless hardware constraint" note in
//! `.kiro/specs/wo2-acoustic-ingress-routing/tasks.md`. This module is
//! for local/bare-metal deployment, where a real microphone exists.
//!
//! The `cpal` callback runs on cpal's own dedicated audio thread and must
//! never block or allocate — any allocation (`Vec::push`, `Box::new`,
//! etc.) risks a page fault or malloc lock contention that can stall the
//! callback past its deadline, which on some backends causes an audible
//! glitch or a dropped buffer. The only work permitted here is copying
//! into a fixed-size `AudioChunk` and doing a lock-free ring push.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, StreamConfig};

use miranda_core::{AudioChunk, AUDIO_CHUNK_FRAMES, AUDIO_SAMPLE_RATE_HZ};
use miranda_ipc::MirandaBus;

/// Errors specific to starting native mic capture. Kept separate from
/// `miranda_core`'s `BackpressureError` because these are capture-setup
/// failures (no device, unsupported config), not ring-buffer conditions.
#[derive(Debug)]
pub enum CaptureError {
    /// No input device was found on this host at all.
    NoInputDevice,
    /// The device rejected the exact config this module requires
    /// (mono, 16 kHz, fixed 160-sample buffer). Carries cpal's own error
    /// for diagnostics.
    UnsupportedConfig(cpal::BuildStreamError),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureError::NoInputDevice => write!(f, "no default input device on this host"),
            CaptureError::UnsupportedConfig(e) => {
                write!(f, "device rejected required capture config: {e}")
            }
        }
    }
}

impl std::error::Error for CaptureError {}

/// Starts native mic capture and returns the live `cpal::Stream` handle.
///
/// The caller must keep the returned `Stream` alive for as long as capture
/// should continue — dropping it stops the stream (this is `cpal`'s own
/// contract, not something this function enforces itself).
///
/// `bus` is an `Arc<MirandaBus>` rather than `&'static MirandaBus`: the
/// design.md sketch used a `'static` reference, but that requires the bus
/// to be leaked or held in a `static`, which is an unnecessary constraint
/// on callers (e.g. a test that creates and tears down a bus per test
/// case). `Arc` gives the callback closure its own owned handle with the
/// same "lives as long as needed" property, without forcing `'static`.
pub fn start_capture(bus: Arc<MirandaBus>) -> Result<cpal::Stream, CaptureError> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or(CaptureError::NoInputDevice)?;

    let config = StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(AUDIO_SAMPLE_RATE_HZ),
        buffer_size: BufferSize::Fixed(AUDIO_CHUNK_FRAMES as u32),
    };

    // Telemetry only (dropped frames due to backpressure) — an AtomicU64
    // rather than a plain counter because it is read/written from the
    // audio callback thread and may be inspected from another thread for
    // diagnostics. Not on any correctness path.
    let dropped_frames = Arc::new(AtomicU64::new(0));
    let dropped_frames_cb = Arc::clone(&dropped_frames);
    let bus_cb = Arc::clone(&bus);

    let stream = device
        .build_input_stream(
            &config,
            move |data: &[f32], _info: &cpal::InputCallbackInfo| {
                // SAFETY-relevant invariant (not an `unsafe` block, but a
                // correctness one): this closure must not allocate. The
                // chunk below is a fixed-size stack value — no Vec, no
                // Box, no String — and `push_audio` is documented (WO-1)
                // as a lock-free, allocation-free atomic ring write.
                on_audio_data(data, &bus_cb, &dropped_frames_cb);
            },
            move |err| {
                // cpal's error callback fires on stream-level failures
                // (device disconnected, backend error) — not per-frame.
                // eprintln! is acceptable here: this path is already the
                // "something is badly wrong" path, not the steady-state
                // hot path the no-allocation rule protects.
                eprintln!("[miranda-audio] cpal input stream error: {err}");
            },
            None,
        )
        .map_err(CaptureError::UnsupportedConfig)?;

    stream.play().map_err(|e| {
        // play() failures are rare (device already in use, etc.) but real;
        // surface them the same way build errors are surfaced rather than
        // silently returning a stream that will never produce data.
        CaptureError::UnsupportedConfig(cpal::BuildStreamError::BackendSpecific {
            err: cpal::BackendSpecificError {
                description: format!("stream.play() failed: {e}"),
            },
        })
    })?;

    Ok(stream)
}

/// The actual per-callback work, factored out of the closure so it can be
/// unit-tested directly without spinning up a real audio device.
///
/// `data.len()` is expected to equal `AUDIO_CHUNK_FRAMES` (160) because of
/// `BufferSize::Fixed` above, but a defensive check handles a backend that
/// doesn't perfectly honour the fixed size (some do deliver a short final
/// buffer at stream start/stop) — in that case the chunk is zero-padded
/// rather than panicking or indexing out of bounds.
fn on_audio_data(data: &[f32], bus: &MirandaBus, dropped_frames: &AtomicU64) {
    let mut samples = [0.0f32; AUDIO_CHUNK_FRAMES];
    let n = data.len().min(AUDIO_CHUNK_FRAMES);
    samples[..n].copy_from_slice(&data[..n]);

    let chunk = AudioChunk {
        timestamp_us: now_us(),
        sample_rate: AUDIO_SAMPLE_RATE_HZ,
        frame_count: n as u32,
        samples,
    };

    if bus.push_audio(chunk).is_err() {
        // Ring full: WO-1 REQ-6 forbids silently corrupting the buffer,
        // which push_audio already upholds by rejecting instead of
        // overwriting. The "silent" part that WO-2 must not repeat is
        // silently dropping telemetry about it — count it, don't just
        // discard and say nothing.
        dropped_frames.fetch_add(1, Ordering::Relaxed);
    }
}

/// Microseconds since the Unix epoch. `AudioChunk.timestamp_us` is
/// documented (WO-1) as "microseconds since harness start," but no
/// harness-start epoch has been established anywhere in this codebase yet
/// (no shared `Instant`/`SystemTime` origin exists outside this module) —
/// using wall-clock time here is a deliberate, documented choice, not a
/// silent deviation: it keeps timestamps monotonically increasing and
/// comparable across the audio and future kinematics (WO-3) writers until
/// a real shared origin is introduced.
fn now_us() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

/// Confirms the sample format this module assumes (`f32`) is actually
/// requestable on the current host's default input device. Not called
/// automatically by `start_capture` — exposed so a caller or test can
/// check compatibility up front and produce a clearer error than a
/// `BuildStreamError` deep inside `cpal`.
pub fn default_input_supports_f32() -> Result<bool, CaptureError> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or(CaptureError::NoInputDevice)?;
    let supports = device
        .supported_input_configs()
        .map(|mut configs| configs.any(|c| c.sample_format() == SampleFormat::F32))
        .unwrap_or(false);
    Ok(supports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use miranda_core::BLENDSHAPE_COUNT;
    use std::sync::atomic::AtomicU64;

    /// T5's specified evidence test: an `AudioChunk` built from 160
    /// samples must report `frame_count == 160` and
    /// `sample_rate == 16000`. Named `test_chunk_size` to match the task
    /// spec exactly.
    #[test]
    fn test_chunk_size() {
        let bus = MirandaBus::in_memory();
        let dropped = AtomicU64::new(0);
        let data = [0.5f32; AUDIO_CHUNK_FRAMES];

        on_audio_data(&data, &bus, &dropped);

        let chunk = bus.pop_audio().expect("one chunk was pushed");
        assert_eq!(chunk.frame_count, 160);
        assert_eq!(chunk.sample_rate, 16_000);
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
    }

    /// A short/partial buffer (fewer than 160 samples — the defensive
    /// case some backends hit at stream start/stop) must not panic and
    /// must zero-pad rather than read out of bounds.
    #[test]
    fn short_buffer_is_zero_padded_not_truncated_incorrectly() {
        let bus = MirandaBus::in_memory();
        let dropped = AtomicU64::new(0);
        let data = [0.25f32; 100]; // shorter than AUDIO_CHUNK_FRAMES

        on_audio_data(&data, &bus, &dropped);

        let chunk = bus.pop_audio().expect("chunk was pushed despite short input");
        assert_eq!(chunk.frame_count, 100, "reported count matches actual samples received");
        assert_eq!(&chunk.samples[..100], &[0.25f32; 100][..]);
        assert_eq!(
            &chunk.samples[100..],
            &[0.0f32; AUDIO_CHUNK_FRAMES - 100][..],
            "unfilled tail must be zero, not garbage"
        );
    }

    /// A backend delivering more than 160 samples in one callback (should
    /// not happen with BufferSize::Fixed, but defensively checked) must
    /// not write out of bounds — extra samples are dropped, not
    /// segfaulted on.
    #[test]
    fn oversized_buffer_does_not_panic_or_overflow() {
        let bus = MirandaBus::in_memory();
        let dropped = AtomicU64::new(0);
        let data = [0.9f32; AUDIO_CHUNK_FRAMES + 40];

        on_audio_data(&data, &bus, &dropped);

        let chunk = bus.pop_audio().expect("chunk was pushed");
        assert_eq!(chunk.frame_count, AUDIO_CHUNK_FRAMES as u32);
        assert_eq!(chunk.samples, [0.9f32; AUDIO_CHUNK_FRAMES]);
    }

    /// Backpressure telemetry: when the ring is genuinely full,
    /// on_audio_data must count the drop rather than panic or silently
    /// discard without any signal at all.
    #[test]
    fn full_ring_increments_dropped_counter_not_panic() {
        let bus = MirandaBus::in_memory();
        let dropped = AtomicU64::new(0);
        let data = [0.1f32; AUDIO_CHUNK_FRAMES];

        // Fill the audio ring to capacity (64 slots per WO-1 design).
        for _ in 0..64 {
            on_audio_data(&data, &bus, &dropped);
        }
        assert_eq!(dropped.load(Ordering::Relaxed), 0, "ring should not be full yet");

        // This push must be rejected by the full ring.
        on_audio_data(&data, &bus, &dropped);
        assert_eq!(dropped.load(Ordering::Relaxed), 1, "one push must have been dropped");
    }

    /// Sanity check that this module didn't accidentally start depending
    /// on BlendshapeFrame's channel count — capture.rs must only ever
    /// touch the audio ring, never blendshape_bus (that is WO-3's role,
    /// per the design.md struct-ownership rule).
    #[test]
    fn capture_never_touches_blendshape_bus() {
        let bus = MirandaBus::in_memory();
        let dropped = AtomicU64::new(0);
        let data = [0.0f32; AUDIO_CHUNK_FRAMES];
        on_audio_data(&data, &bus, &dropped);
        assert!(bus.pop_blendshape().is_none(), "capture.rs must never write blendshapes");
        // BLENDSHAPE_COUNT referenced only to document the boundary this
        // test checks, not because capture.rs uses it.
        let _ = BLENDSHAPE_COUNT;
    }
}
