//! WO-3 T9 — real-time verification harness.
//!
//! Runs the T8 dispatcher against a **real shared-memory bus in `/dev/shm`**
//! for a wall-clock duration, consumes every published frame from the other
//! side, and reports against WO-3's acceptance criteria:
//!
//! 1. Frame build time within the 16.6 ms budget.
//! 2. Zero dropped frames.
//! 3. No-Loop Video Protocol compliance on the **composed** output — no two
//!    consecutive frames identical, and no frame an exact repeat of any
//!    earlier frame.
//!
//! # Why this is separate from the unit tests
//!
//! T8's tests exercise the dispatcher through an in-process
//! `MirandaBus::in_memory()`. That is the right scope for testing frame
//! logic, but it cannot fail in the ways a real deployment fails: it never
//! touches an mmap, never crosses a page boundary in `/dev/shm`, and never
//! involves the kernel. This harness uses a real file-backed mapping and a
//! real second thread reading it, so what it measures is the transport that
//! ships.
//!
//! It is also deliberately *observational*. The checks run on frames read
//! back out of shared memory, not on the dispatcher's own copies. A frame
//! that was composed correctly but written to the wrong offset, or torn by a
//! racing writer, would pass every check in T8 and fail here.
//!
//! # Honest limits of what this proves
//!
//! - The frame rate figure is measured on a general-purpose desktop kernel
//!   with no real-time scheduling priority. It demonstrates the workload fits
//!   the budget with orders of magnitude to spare; it is not a hard real-time
//!   guarantee, and this document does not claim one.
//! - "No repeated frame" is verified over the run's duration, not proved for
//!   all time. The underlying reason repeats do not occur is that the
//!   oscillators are driven by continuous noise fields and non-commensurate
//!   periods rather than by a cycling table, but the *evidence* here is
//!   empirical and bounded by the run length.
//! - Nothing here verifies pixels. The frames are correct 52-channel weight
//!   vectors moving within physical limits. Whether they look like a living
//!   face on EVE's rig is a WO-5 rendering question that requires looking at
//!   rendered output.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use miranda_core::{BlendshapeFrame, BLENDSHAPE_COUNT};
use miranda_ipc::MirandaBus;

use crate::compositor::DEFAULT_MAX_VELOCITY_PER_SEC;
use crate::dispatcher::{DispatcherThread, FRAME_PERIOD, TARGET_FPS};

/// Per-frame time budget. 1/60 s.
pub const FRAME_BUDGET_US: f64 = 16_666.7;

/// A timestamp gap larger than this many frame periods means at least one
/// frame never reached the bus.
const DROP_GAP_PERIODS: f64 = 1.6;

/// Default verification bus path.
///
/// Deliberately **not** [`miranda_ipc::BUS_PATH`]: running the harness must
/// not stomp on a live session's production bus, and a verification run that
/// silently interferes with the thing being verified is worse than no run.
pub const DEFAULT_VERIFY_BUS_PATH: &str = "/dev/shm/miranda_wo3_verify";

/// Harness configuration.
#[derive(Debug, Clone)]
pub struct VerificationConfig {
    /// Wall-clock duration to run.
    pub duration: Duration,
    /// Master seed for the dispatcher's generators.
    pub seed: u64,
    /// Shared-memory path. `None` uses [`DEFAULT_VERIFY_BUS_PATH`].
    pub bus_path: Option<PathBuf>,
    /// Remove the shared-memory file when the run finishes.
    pub cleanup: bool,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            duration: Duration::from_secs(30),
            seed: 0x4D49_5241_4E44_41,
            bus_path: None,
            cleanup: true,
        }
    }
}

/// Everything observed during a run.
#[derive(Debug, Clone)]
pub struct VerificationReport {
    pub bus_path: PathBuf,
    pub requested_duration: Duration,
    pub elapsed: Duration,

    // Producer-side counters, from the dispatcher's own atomics.
    pub frames_published: u64,
    pub frames_dropped: u64,
    pub publish_failures: u64,
    pub late_frames: u64,
    pub mean_build_us: f64,
    pub max_build_us: f64,

    // Consumer-side observations, from frames read back out of `/dev/shm`.
    pub frames_consumed: u64,
    pub observed_fps: f64,
    pub distinct_frames: usize,
    pub repeated_frames: u64,
    pub identical_consecutive_frames: u64,
    pub non_monotonic_timestamps: u64,
    pub min_gap_us: u64,
    pub max_gap_us: u64,
    pub mean_gap_us: f64,
    pub gaps_implying_a_drop: u64,
    pub out_of_range_values: u64,
    /// Largest single-frame change on any channel, as seen on the bus.
    pub max_channel_delta: f32,
    /// The damper's per-frame ceiling, for comparison.
    pub channel_delta_cap: f32,
}

impl VerificationReport {
    /// Criteria that failed, empty if the run passed.
    ///
    /// Returned as a list rather than a bool so a failing run says *which*
    /// property broke. A bare `false` would send the next person back to
    /// re-run it to find out.
    pub fn failures(&self) -> Vec<String> {
        let mut out = Vec::new();

        if self.frames_consumed == 0 {
            out.push("no frames were read back from shared memory".into());
            // Everything below is meaningless without frames.
            return out;
        }

        if self.max_build_us > FRAME_BUDGET_US {
            out.push(format!(
                "frame build time {:.3} us exceeds the {FRAME_BUDGET_US} us budget",
                self.max_build_us
            ));
        }
        if self.frames_dropped != 0 {
            out.push(format!("{} dropped frames (must be 0)", self.frames_dropped));
        }
        if self.gaps_implying_a_drop != 0 {
            out.push(format!(
                "{} inter-frame gaps on the bus imply a missing frame",
                self.gaps_implying_a_drop
            ));
        }
        if self.identical_consecutive_frames != 0 {
            out.push(format!(
                "{} consecutive frame pairs were bit-identical — the face froze",
                self.identical_consecutive_frames
            ));
        }
        if self.repeated_frames != 0 {
            out.push(format!(
                "{} frames exactly repeated an earlier frame — the idle \
                 animation is looping",
                self.repeated_frames
            ));
        }
        if self.non_monotonic_timestamps != 0 {
            out.push(format!(
                "{} timestamps did not increase — frames arrived out of order",
                self.non_monotonic_timestamps
            ));
        }
        if self.out_of_range_values != 0 {
            out.push(format!(
                "{} channel values left [0, 1]",
                self.out_of_range_values
            ));
        }
        if self.max_channel_delta > self.channel_delta_cap + 1e-4 {
            out.push(format!(
                "a channel moved {} in one frame, over the {} cap — the \
                 velocity clamp is not holding on the bus",
                self.max_channel_delta, self.channel_delta_cap
            ));
        }
        // A run that publishes far fewer frames than its duration implies is
        // not meaningfully verifying a 60 FPS pipeline, even if every other
        // check passes.
        let expected = self.requested_duration.as_secs_f64() * TARGET_FPS as f64;
        if (self.frames_published as f64) < expected * 0.75 {
            out.push(format!(
                "only {} frames published in {:.2} s, well under the ~{:.0} \
                 expected at {TARGET_FPS} fps",
                self.frames_published,
                self.elapsed.as_secs_f64(),
                expected
            ));
        }

        out
    }

    pub fn passed(&self) -> bool {
        self.failures().is_empty()
    }

    /// Human-readable report.
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("=== WO-3 T9 real-time verification ===\n");
        s.push_str(&format!("bus                    {}\n", self.bus_path.display()));
        s.push_str(&format!(
            "duration               {:.2} s (requested {:.2} s)\n",
            self.elapsed.as_secs_f64(),
            self.requested_duration.as_secs_f64()
        ));
        s.push_str("\n-- producer (dispatcher thread) --\n");
        s.push_str(&format!("frames published       {}\n", self.frames_published));
        s.push_str(&format!(
            "frames dropped         {}   <- criterion: 0\n",
            self.frames_dropped
        ));
        s.push_str(&format!("late frames            {}\n", self.late_frames));
        s.push_str(&format!("publish failures       {}\n", self.publish_failures));
        s.push_str(&format!(
            "frame build mean       {:.3} us ({:.4}% of budget)\n",
            self.mean_build_us,
            self.mean_build_us / FRAME_BUDGET_US * 100.0
        ));
        s.push_str(&format!(
            "frame build max        {:.3} us ({:.4}% of budget)   <- criterion: <= {:.1} us\n",
            self.max_build_us,
            self.max_build_us / FRAME_BUDGET_US * 100.0,
            FRAME_BUDGET_US
        ));
        s.push_str("\n-- consumer (read back from /dev/shm) --\n");
        s.push_str(&format!("frames consumed        {}\n", self.frames_consumed));
        s.push_str(&format!("observed rate          {:.2} fps\n", self.observed_fps));
        s.push_str(&format!(
            "inter-frame gap        min {} us, mean {:.1} us, max {} us (nominal {} us)\n",
            self.min_gap_us,
            self.mean_gap_us,
            self.max_gap_us,
            FRAME_PERIOD.as_micros()
        ));
        s.push_str(&format!(
            "gaps implying a drop   {}   <- criterion: 0\n",
            self.gaps_implying_a_drop
        ));
        s.push_str(&format!(
            "non-monotonic stamps   {}\n",
            self.non_monotonic_timestamps
        ));
        s.push_str(&format!(
            "out-of-range values    {}\n",
            self.out_of_range_values
        ));
        s.push_str(&format!(
            "max channel delta      {:.6} (damper cap {:.6})\n",
            self.max_channel_delta, self.channel_delta_cap
        ));
        s.push_str("\n-- No-Loop Video Protocol (composed output) --\n");
        s.push_str(&format!(
            "distinct frames        {} of {}\n",
            self.distinct_frames, self.frames_consumed
        ));
        s.push_str(&format!(
            "identical consecutive  {}   <- criterion: 0\n",
            self.identical_consecutive_frames
        ));
        s.push_str(&format!(
            "exact repeats          {}   <- criterion: 0\n",
            self.repeated_frames
        ));

        s.push('\n');
        let failures = self.failures();
        if failures.is_empty() {
            s.push_str("RESULT: PASS — all WO-3 T9 criteria met.\n");
        } else {
            s.push_str("RESULT: FAIL\n");
            for f in failures {
                s.push_str(&format!("  - {f}\n"));
            }
        }
        s
    }
}

/// Runs a verification pass.
///
/// Starts the dispatcher on a real `/dev/shm` mapping, consumes frames from
/// the other side for the configured duration, then reports.
pub fn run(config: &VerificationConfig) -> io::Result<VerificationReport> {
    let path: PathBuf = config
        .bus_path
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_VERIFY_BUS_PATH));

    // A stale file from a previous run carries stale head/tail counters,
    // which would look like a partly-full ring and skew the very first
    // measurements. Start from a clean mapping.
    let _ = std::fs::remove_file(&path);

    let bus = Arc::new(MirandaBus::open_or_create_at(&path)?);

    let stop_consumer = Arc::new(AtomicBool::new(false));
    let consumer_bus = Arc::clone(&bus);
    let consumer_flag = Arc::clone(&stop_consumer);

    // The consumer runs on its own thread and does the analysis. It is
    // allowed to allocate freely — it is the renderer's side of the bus, not
    // the real-time producer, and pretending otherwise would mean giving up
    // the HashSet the repeat check needs.
    let consumer = thread::spawn(move || consume(&consumer_bus, &consumer_flag));

    let dispatcher = DispatcherThread::spawn(Arc::clone(&bus), config.seed)?;
    let started = Instant::now();
    thread::sleep(config.duration);
    let elapsed = started.elapsed();

    let stats = Arc::clone(dispatcher.stats());
    dispatcher.stop();

    // Give the consumer a moment to drain whatever the last frames were,
    // then stop it. Without this the tail of the run reads as dropped frames
    // that were in fact published fine.
    thread::sleep(FRAME_PERIOD * 8);
    stop_consumer.store(true, Ordering::Relaxed);
    let observed = consumer.join().map_err(|_| {
        io::Error::new(io::ErrorKind::Other, "verification consumer thread panicked")
    })?;

    if config.cleanup {
        let _ = std::fs::remove_file(&path);
    }

    let frames_consumed = observed.frames as u64;
    let mean_gap_us = if observed.gap_count > 0 {
        observed.gap_total_us as f64 / observed.gap_count as f64
    } else {
        0.0
    };

    Ok(VerificationReport {
        bus_path: path,
        requested_duration: config.duration,
        elapsed,

        frames_published: stats.frames_published.load(Ordering::Relaxed),
        frames_dropped: stats.frames_dropped.load(Ordering::Relaxed),
        publish_failures: stats.publish_failures.load(Ordering::Relaxed),
        late_frames: stats.late_frames.load(Ordering::Relaxed),
        mean_build_us: stats.mean_build_us().unwrap_or(0.0),
        max_build_us: stats.max_build_us(),

        frames_consumed,
        observed_fps: frames_consumed as f64 / elapsed.as_secs_f64(),
        distinct_frames: observed.distinct,
        repeated_frames: observed.repeats,
        identical_consecutive_frames: observed.identical_consecutive,
        non_monotonic_timestamps: observed.non_monotonic,
        min_gap_us: if observed.gap_count > 0 {
            observed.min_gap_us
        } else {
            0
        },
        max_gap_us: observed.max_gap_us,
        mean_gap_us,
        gaps_implying_a_drop: observed.drop_gaps,
        out_of_range_values: observed.out_of_range,
        max_channel_delta: observed.max_channel_delta,
        channel_delta_cap: DEFAULT_MAX_VELOCITY_PER_SEC * (1.0 / TARGET_FPS as f32),
    })
}

#[derive(Default)]
struct Observed {
    frames: usize,
    distinct: usize,
    repeats: u64,
    identical_consecutive: u64,
    non_monotonic: u64,
    min_gap_us: u64,
    max_gap_us: u64,
    gap_total_us: u128,
    gap_count: u64,
    drop_gaps: u64,
    out_of_range: u64,
    max_channel_delta: f32,
}

fn consume(bus: &MirandaBus, stop: &AtomicBool) -> Observed {
    let mut o = Observed {
        min_gap_us: u64::MAX,
        ..Default::default()
    };
    // Bit patterns, not floats: `f32` is not `Hash`, and bitwise identity is
    // the strictest possible reading of "the same frame". Two frames that
    // differ in the last mantissa bit of one channel are different frames,
    // and treating them as the same would let a real freeze hide behind a
    // tolerance.
    let mut seen: HashSet<[u32; BLENDSHAPE_COUNT]> = HashSet::new();
    let mut prev: Option<BlendshapeFrame> = None;

    let drop_threshold_us =
        (FRAME_PERIOD.as_micros() as f64 * DROP_GAP_PERIODS) as u64;

    loop {
        let mut got_any = false;
        while let Some(frame) = bus.pop_blendshape() {
            got_any = true;
            o.frames += 1;

            for w in frame.weights.iter() {
                if !(0.0..=1.0).contains(w) || !w.is_finite() {
                    o.out_of_range += 1;
                }
            }

            let mut key = [0u32; BLENDSHAPE_COUNT];
            for (k, v) in key.iter_mut().zip(frame.weights.iter()) {
                *k = v.to_bits();
            }
            if !seen.insert(key) {
                o.repeats += 1;
            }

            if let Some(p) = prev {
                if frame.weights == p.weights {
                    o.identical_consecutive += 1;
                }
                if frame.timestamp_us <= p.timestamp_us {
                    o.non_monotonic += 1;
                } else {
                    let gap = frame.timestamp_us - p.timestamp_us;
                    o.min_gap_us = o.min_gap_us.min(gap);
                    o.max_gap_us = o.max_gap_us.max(gap);
                    o.gap_total_us += gap as u128;
                    o.gap_count += 1;
                    if gap > drop_threshold_us {
                        o.drop_gaps += 1;
                    }
                }
                for i in 0..BLENDSHAPE_COUNT {
                    let d = (frame.weights[i] - p.weights[i]).abs();
                    if d > o.max_channel_delta {
                        o.max_channel_delta = d;
                    }
                }
            }
            prev = Some(frame);
        }

        if stop.load(Ordering::Relaxed) && !got_any {
            break;
        }
        if !got_any {
            // Sleep well under the ring's drain deadline. The blendshape ring
            // holds 128 frames, about 2.1 s at 60 FPS, so a 2 ms poll has
            // three orders of magnitude of headroom and the consumer cannot
            // be the reason a frame is lost.
            thread::sleep(Duration::from_millis(2));
        }
    }

    o.distinct = seen.len();
    o
}

/// Convenience wrapper: run with the default configuration for `secs`.
pub fn run_for_secs(secs: u64) -> io::Result<VerificationReport> {
    run(&VerificationConfig {
        duration: Duration::from_secs(secs),
        ..Default::default()
    })
}

/// Runs against a specific path, for callers that need to avoid the default.
pub fn run_at<P: AsRef<Path>>(
    path: P,
    duration: Duration,
    seed: u64,
) -> io::Result<VerificationReport> {
    run(&VerificationConfig {
        duration,
        seed,
        bus_path: Some(path.as_ref().to_path_buf()),
        cleanup: true,
    })
}
