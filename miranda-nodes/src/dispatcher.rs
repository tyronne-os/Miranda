//! WO-3 T8 — the 60 FPS face dispatcher.
//!
//! This is where the pieces built in T2–T7 become a running system: a paced
//! loop that ticks the autonomic oscillators, solves speech weights from
//! audio, composes and damps the result, and publishes one
//! [`BlendshapeFrame`] per frame into the shared-memory ring buffer built in
//! WO-1.
//!
//! # Structure
//!
//! [`FaceDispatcher`] is the pure, synchronous frame builder — give it a
//! `dt` and a block of audio, get a frame. It owns every buffer it needs, so
//! [`FaceDispatcher::tick`] performs no allocation, no locking, no I/O, and
//! no logging. That makes it directly testable frame by frame, and it is
//! what T9's benchmark measures.
//!
//! [`DispatcherThread`] wraps it in a real paced 60 FPS thread that drains
//! audio from the bus and publishes frames to it.
//!
//! # Threading: a documented deviation
//!
//! WO-3's design.md calls for each oscillator to run on its own thread. That
//! is implemented here as *one* dispatcher thread ticking the oscillators
//! sequentially, and the deviation is deliberate. The reason is arithmetic,
//! not preference:
//!
//! - The whole per-frame workload is a few microseconds against a 16,667 µs
//!   budget (T7 measured the most expensive stage, the acoustic solver, at
//!   2.7 µs). [`bench_frame_cost`] prints the real composed figure.
//! - The development target is a dual-core Celeron N4500. Fanning three
//!   sub-microsecond jobs onto three threads and joining them every frame
//!   means the frame's cost becomes the *slowest* thread's wakeup latency
//!   plus synchronisation, on a machine with fewer cores than threads. Thread
//!   wakeup latency alone is tens of microseconds — an order of magnitude
//!   more than the work being parallelised.
//! - Sequential ticking is also exactly reproducible for a given seed, which
//!   is what T9's No-Loop verification and any future frame-capture
//!   regression needs. Three racing threads are not.
//!
//! The genuinely valuable concurrency boundary is the one that already
//! exists: the lock-free ring buffer. Producers (microphone, ASR, this
//! dispatcher) and consumers (the renderer) are decoupled through
//! shared memory with no shared locks, which is what actually keeps a slow
//! consumer from stalling the face.
//!
//! If the speech stage is ever replaced by a learned model costing
//! milliseconds rather than microseconds, that stage — and only that stage —
//! is worth moving to its own thread, feeding the compositor through the
//! bus like any other producer. The oscillators will still not be worth
//! splitting.
//!
//! # Real-time discipline in the hot loop
//!
//! [`FaceDispatcher::tick`] and [`FaceDispatcher::drain_audio`] contain no
//! allocation, no `Mutex`, no formatting, and no logging. Logging is the
//! easiest of those to get wrong, because a single `eprintln!` in an error
//! branch takes a lock on stderr and can block for milliseconds behind
//! another writer — a frame-dropping stall that only appears under the exact
//! conditions that triggered the error. Errors are therefore counted in
//! atomics ([`DispatchStats`]) and reported by whoever reads them, off the
//! hot path.
//!
//! `zero_allocations_in_the_hot_loop` in this module's tests proves the
//! allocation claim against a counting global allocator rather than
//! asserting it.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use miranda_core::{BlendshapeFrame, BLENDSHAPE_COUNT};
use miranda_ipc::MirandaBus;

use crate::blink::BlinkGenerator;
use crate::breath::BreathGenerator;
use crate::compositor::Compositor;
use crate::gaze::GazeGenerator;
use crate::solver::AudioBlendshapeSolver;

/// Target frame rate.
pub const TARGET_FPS: u32 = 60;

/// Nominal frame period. 16,666,667 ns — deliberately the exact 1/60 s
/// rather than a rounded 16 ms (0.4% fast, which is 14 extra frames per
/// hour) or 17 ms (2% slow).
pub const FRAME_PERIOD: Duration = Duration::from_nanos(16_666_667);

/// Nominal `dt` handed to the oscillators, in seconds.
pub const FRAME_DT: f32 = 1.0 / TARGET_FPS as f32;

/// Capacity of the audio staging buffer, in samples.
///
/// One frame needs about 267 samples at 16 kHz. This holds roughly six
/// frames' worth so a burst of buffered chunks after a scheduling hiccup
/// still fits, and it is a fixed array rather than a `Vec` precisely so the
/// hot loop cannot allocate.
pub const AUDIO_STAGING_SAMPLES: usize = 1600;

/// Maximum audio chunks drained per frame.
///
/// Bounded so that a producer flooding the ring cannot turn one frame's
/// drain into an unbounded loop and blow the frame budget. The excess stays
/// in the ring and is consumed next frame, or is eventually overwritten —
/// dropping stale audio is the correct choice for a live face, since audio
/// older than a frame or two describes a mouth shape that is already in the
/// past.
pub const MAX_CHUNKS_PER_FRAME: usize = 12;

/// How long before the deadline the pacer stops sleeping and starts
/// spinning.
///
/// `thread::sleep` on Linux is accurate to roughly the timer slack (about
/// 50 µs by default) but can overshoot further under load, and an overshoot
/// is a late frame. Sleeping to within this margin and then spinning trades
/// a small amount of CPU for deadline accuracy. 300 µs is 1.8% of a frame.
pub const SPIN_MARGIN: Duration = Duration::from_micros(300);

// ---------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------

/// Counters shared with the dispatcher thread.
///
/// Atomics rather than a mutex-guarded struct so the hot loop never blocks,
/// and so a reader can sample them at any time without perturbing pacing.
/// `Relaxed` ordering throughout: these are independent counters, and no
/// correctness decision depends on observing them in a particular order
/// relative to each other.
#[derive(Debug, Default)]
pub struct DispatchStats {
    /// Frames successfully written to the bus.
    pub frames_published: AtomicU64,
    /// Frames the loop never produced because it missed a whole period.
    /// This is the dropped-frame count T9 must show as zero.
    pub frames_dropped: AtomicU64,
    /// Frames produced but rejected by the bus because the consumer was not
    /// draining. Distinct from `frames_dropped`: the face kept up, the
    /// reader did not.
    pub publish_failures: AtomicU64,
    /// Frames that were produced but finished after their deadline, without
    /// a whole period being lost.
    pub late_frames: AtomicU64,
    /// Worst observed cost of building one frame, in nanoseconds. Excludes
    /// pacing, so it measures the work rather than the wait.
    pub max_build_ns: AtomicU64,
    /// Sum of frame build costs, in nanoseconds. With `frames_published`
    /// this gives a mean without storing a histogram in the hot loop.
    pub total_build_ns: AtomicU64,
    /// Audio chunks consumed from the bus.
    pub audio_chunks_consumed: AtomicU64,
}

impl DispatchStats {
    fn record_build(&self, ns: u64) {
        self.total_build_ns.fetch_add(ns, Ordering::Relaxed);
        // Monotonic maximum via compare-exchange. `fetch_max` would do, but
        // the loop makes the intent explicit and is equally lock-free.
        let mut current = self.max_build_ns.load(Ordering::Relaxed);
        while ns > current {
            match self.max_build_ns.compare_exchange_weak(
                current,
                ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
    }

    /// Mean frame build cost in microseconds, or `None` before any frame.
    pub fn mean_build_us(&self) -> Option<f64> {
        let n = self.frames_published.load(Ordering::Relaxed);
        if n == 0 {
            return None;
        }
        Some(self.total_build_ns.load(Ordering::Relaxed) as f64 / n as f64 / 1000.0)
    }

    /// Worst frame build cost in microseconds.
    pub fn max_build_us(&self) -> f64 {
        self.max_build_ns.load(Ordering::Relaxed) as f64 / 1000.0
    }
}

// ---------------------------------------------------------------------
// The frame builder
// ---------------------------------------------------------------------

/// Folds a 64-bit seed down to 32 bits without discarding the high half.
/// A bare `as u32` would make seeds differing only above bit 32 identical.
fn mix32(mut x: u64) -> u32 {
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((x ^ (x >> 31)) & 0xFFFF_FFFF) as u32
}

/// Builds one composed [`BlendshapeFrame`] per call. Owns every generator
/// and every buffer, so `tick` allocates nothing.
pub struct FaceDispatcher {
    blink: BlinkGenerator,
    gaze: GazeGenerator,
    breath: BreathGenerator,
    solver: AudioBlendshapeSolver,
    compositor: Compositor,
    /// Fixed staging buffer for drained audio. Never reallocated.
    audio: [f32; AUDIO_STAGING_SAMPLES],
    audio_len: usize,
    /// The frame handed back each tick. Reused rather than constructed, so
    /// no 216-byte copy is created on the stack per frame either.
    frame: BlendshapeFrame,
}

impl FaceDispatcher {
    /// Builds a dispatcher with all generators seeded from `seed`.
    ///
    /// Each generator gets a *different* derived seed. Handing all three the
    /// same seed would correlate their noise fields, and blinks would tend
    /// to land on breath peaks — a subtle periodicity that is exactly what
    /// the No-Loop Video Protocol exists to prevent.
    pub fn from_seed(seed: u64) -> Self {
        // Derived with the golden-ratio and splitmix constants so that
        // adjacent master seeds still produce well-separated sub-seeds. The
        // gaze and breath generators take `u32` (the `noise` crate's seed
        // type), so the derivation folds the high bits in rather than
        // truncating them away.
        let gaze_seed = mix32(seed ^ 0x9E37_79B9_7F4A_7C15);
        let breath_seed = mix32(seed ^ 0x85EB_CA6B_C2B2_AE35);

        Self {
            blink: BlinkGenerator::from_seed(seed),
            gaze: GazeGenerator::from_seed(gaze_seed),
            breath: BreathGenerator::from_seed(breath_seed),
            solver: AudioBlendshapeSolver::new(),
            compositor: Compositor::new(),
            audio: [0.0; AUDIO_STAGING_SAMPLES],
            audio_len: 0,
            frame: BlendshapeFrame {
                timestamp_us: 0,
                weights: [0.0; BLENDSHAPE_COUNT],
            },
        }
    }

    /// Drains up to [`MAX_CHUNKS_PER_FRAME`] audio chunks from `bus` into
    /// the staging buffer, returning the number of chunks taken.
    ///
    /// Allocation-free and non-blocking: `pop_audio` returns `None` rather
    /// than waiting, so a silent microphone costs nothing.
    pub fn drain_audio(&mut self, bus: &MirandaBus) -> usize {
        self.audio_len = 0;
        let mut chunks = 0;

        while chunks < MAX_CHUNKS_PER_FRAME {
            let Some(chunk) = bus.pop_audio() else { break };
            chunks += 1;

            // `frame_count` is producer-supplied, so it is validated rather
            // than trusted: a corrupt or oversized value would index past
            // the chunk's own sample array.
            let valid = (chunk.frame_count as usize).min(chunk.samples.len());
            let room = AUDIO_STAGING_SAMPLES - self.audio_len;
            let take = valid.min(room);
            if take > 0 {
                self.audio[self.audio_len..self.audio_len + take]
                    .copy_from_slice(&chunk.samples[..take]);
                self.audio_len += take;
            }
            if room == take && valid > take {
                // Staging is full. Stop draining; the rest stays in the ring.
                break;
            }
        }

        chunks
    }

    /// The audio staged by the last [`Self::drain_audio`].
    pub fn staged_audio(&self) -> &[f32] {
        &self.audio[..self.audio_len]
    }

    /// Builds one frame.
    ///
    /// `external_speech`, when supplied, replaces the internal acoustic
    /// solver as the speech layer — that is the Pipeline 1 path, where
    /// [`crate::viseme`] has already produced weights from Polly viseme
    /// events. When it is `None` the internal solver runs on
    /// [`Self::staged_audio`], which is the Pipeline 2 path. The autonomic
    /// layer is identical either way, which is the property that makes the
    /// speech source a swappable slot.
    ///
    /// No allocation, no locking, no I/O, no logging.
    pub fn tick(
        &mut self,
        dt: f32,
        timestamp_us: u64,
        external_speech: Option<&[f32; BLENDSHAPE_COUNT]>,
    ) -> &BlendshapeFrame {
        let blink = self.blink.tick(dt);
        let gaze = self.gaze.tick(dt);
        let breath = self.breath.tick(dt);

        let speech: &[f32; BLENDSHAPE_COUNT] = match external_speech {
            Some(w) => w,
            None => {
                // Always solve, even on silence. The solver's release
                // smoothing is what relaxes the mouth; skipping the call
                // would freeze the last shape instead, leaving the face
                // holding a vowel.
                self.solver.solve(&self.audio[..self.audio_len])
            }
        };

        let composed = self.compositor.compose(
            Some(speech),
            Some(blink),
            Some(gaze),
            Some(breath),
            dt,
        );

        self.frame.timestamp_us = timestamp_us;
        self.frame.weights.copy_from_slice(composed);
        &self.frame
    }

    /// The most recently built frame.
    pub fn frame(&self) -> &BlendshapeFrame {
        &self.frame
    }
}

// ---------------------------------------------------------------------
// The paced thread
// ---------------------------------------------------------------------

/// A running 60 FPS dispatcher thread.
///
/// Dropping the handle signals the loop to stop and joins it, so a forgotten
/// handle cannot leave a thread publishing frames into a bus nobody reads.
pub struct DispatcherThread {
    stop: Arc<AtomicBool>,
    stats: Arc<DispatchStats>,
    join: Option<JoinHandle<()>>,
}

impl DispatcherThread {
    /// Starts the loop.
    ///
    /// The bus is shared rather than owned so the microphone producer and the
    /// renderer consumer can hold the same mapping. `MirandaBus` is `Sync`
    /// (WO-1), and the rings are single-producer/single-consumer per
    /// direction, so no lock is involved.
    pub fn spawn(bus: Arc<MirandaBus>, seed: u64) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(DispatchStats::default());

        let thread_stop = Arc::clone(&stop);
        let thread_stats = Arc::clone(&stats);

        let join = thread::Builder::new()
            .name("miranda-face-60fps".into())
            .spawn(move || run_loop(bus, thread_stop, thread_stats, seed))?;

        Ok(Self {
            stop,
            stats,
            join: Some(join),
        })
    }

    /// Live counters. Safe to read at any time from any thread.
    pub fn stats(&self) -> &Arc<DispatchStats> {
        &self.stats
    }

    /// Signals the loop to stop and waits for it.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for DispatcherThread {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_loop(
    bus: Arc<MirandaBus>,
    stop: Arc<AtomicBool>,
    stats: Arc<DispatchStats>,
    seed: u64,
) {
    // Everything allocated up front, before the first deadline.
    let mut dispatcher = FaceDispatcher::from_seed(seed);

    let start = Instant::now();
    let mut deadline = start + FRAME_PERIOD;

    while !stop.load(Ordering::Relaxed) {
        let build_start = Instant::now();

        let chunks = dispatcher.drain_audio(&bus);
        let timestamp_us = build_start.duration_since(start).as_micros() as u64;
        let frame = *dispatcher.tick(FRAME_DT, timestamp_us, None);

        let build_ns = build_start.elapsed().as_nanos() as u64;

        // Counted, never logged. See the module docs on why an eprintln! in
        // this branch would be a frame-dropping stall.
        if bus.push_blendshape(frame).is_err() {
            stats.publish_failures.fetch_add(1, Ordering::Relaxed);
        } else {
            stats.frames_published.fetch_add(1, Ordering::Relaxed);
        }
        if chunks > 0 {
            stats
                .audio_chunks_consumed
                .fetch_add(chunks as u64, Ordering::Relaxed);
        }
        stats.record_build(build_ns);

        let now = Instant::now();
        if now > deadline {
            let overrun = now - deadline;
            if overrun >= FRAME_PERIOD {
                // A whole period or more was lost. Count the frames that
                // never happened and re-base the deadline rather than
                // firing a burst to catch up — bursting makes the face
                // briefly run fast, which reads as a glitch, whereas a
                // re-base reads as a single dropped frame.
                let lost = (overrun.as_nanos() / FRAME_PERIOD.as_nanos()) as u64;
                stats.frames_dropped.fetch_add(lost, Ordering::Relaxed);
                deadline = now + FRAME_PERIOD;
            } else {
                stats.late_frames.fetch_add(1, Ordering::Relaxed);
                deadline += FRAME_PERIOD;
            }
            continue;
        }

        pace_until(deadline, &stop);
        // Advancing by a fixed period rather than from `Instant::now()` is
        // what keeps the cadence from drifting: any per-frame jitter is
        // absorbed instead of accumulating into a slow clock.
        deadline += FRAME_PERIOD;
    }
}

/// Sleeps then spins until `deadline`, bailing out early if asked to stop.
fn pace_until(deadline: Instant, stop: &AtomicBool) {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let remaining = deadline - now;
        if remaining > SPIN_MARGIN {
            // Wake early by the margin; the spin below covers the rest.
            thread::sleep(remaining - SPIN_MARGIN);
            if stop.load(Ordering::Relaxed) {
                return;
            }
        } else {
            std::hint::spin_loop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use miranda_core::{arkit, AudioChunk, AUDIO_CHUNK_FRAMES, AUDIO_SAMPLE_RATE_HZ};
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::AtomicUsize;

    // -----------------------------------------------------------------
    // Allocation counting
    // -----------------------------------------------------------------

    /// A pass-through allocator that counts allocations, so the "zero-alloc
    /// hot loop" claim can be *proved* rather than asserted. Installed only
    /// under `cfg(test)`.
    struct CountingAlloc;

    static ALLOCS: AtomicUsize = AtomicUsize::new(0);

    // SAFETY: every method forwards directly to `System`, which is a valid
    // allocator, with identical layouts and pointers. The only added
    // behaviour is a relaxed counter increment, which has no bearing on
    // memory validity.
    unsafe impl GlobalAlloc for CountingAlloc {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            System.alloc(layout)
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            System.dealloc(ptr, layout)
        }
        unsafe fn realloc(
            &self,
            ptr: *mut u8,
            layout: Layout,
            new_size: usize,
        ) -> *mut u8 {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            System.realloc(ptr, layout, new_size)
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            System.alloc_zeroed(layout)
        }
    }

    #[global_allocator]
    static ALLOCATOR: CountingAlloc = CountingAlloc;

    fn chunk(samples: &[f32], timestamp_us: u64) -> AudioChunk {
        let mut c = AudioChunk {
            timestamp_us,
            sample_rate: AUDIO_SAMPLE_RATE_HZ,
            frame_count: samples.len().min(AUDIO_CHUNK_FRAMES) as u32,
            samples: [0.0; AUDIO_CHUNK_FRAMES],
        };
        let n = c.frame_count as usize;
        c.samples[..n].copy_from_slice(&samples[..n]);
        c
    }

    fn speech_chunk(timestamp_us: u64) -> AudioChunk {
        // A formant pair around /a/ at conversational level.
        let s: Vec<f32> = (0..AUDIO_CHUNK_FRAMES)
            .map(|i| {
                let t = i as f32 / AUDIO_SAMPLE_RATE_HZ as f32;
                0.3 * (std::f32::consts::TAU * 730.0 * t).sin()
                    + 0.2 * (std::f32::consts::TAU * 1090.0 * t).sin()
            })
            .collect();
        chunk(&s, timestamp_us)
    }

    // -----------------------------------------------------------------
    // The zero-allocation guarantee
    // -----------------------------------------------------------------

    /// **The real-time safety criterion, proved rather than asserted.**
    ///
    /// A single allocation inside the frame loop is a potential multi-
    /// millisecond stall the moment the allocator has to hit the OS for more
    /// memory, and it will happen unpredictably rather than in testing.
    /// Counting real allocator calls is the only way to know the hot path is
    /// clean; reading the code is not, because a `Vec`, a `format!`, a
    /// boxed closure, or an iterator adaptor that collects can all hide one.
    #[test]
    fn zero_allocations_in_the_hot_loop() {
        let bus = MirandaBus::in_memory();
        let mut d = FaceDispatcher::from_seed(7);

        // Built once, outside the measured window. `speech_chunk` collects
        // into a `Vec`, so calling it inside the loop would have the test
        // measuring its own allocation — which is exactly what it did on the
        // first run: 600 allocations for 600 iterations, all of them the
        // test's own.
        let audio = speech_chunk(0);

        // Warm up: the first tick may still touch lazily-initialised state,
        // and that is startup cost, not per-frame cost.
        for i in 0..10 {
            bus.push_audio(audio).unwrap();
            d.drain_audio(&bus);
            d.tick(FRAME_DT, i, None);
        }

        let before = ALLOCS.load(Ordering::Relaxed);
        for i in 0..600 {
            // Feed audio so the solver path is exercised, not skipped.
            let _ = bus.push_audio(audio);
            d.drain_audio(&bus);
            let frame = d.tick(FRAME_DT, i * 16_667, None);
            std::hint::black_box(frame.weights[arkit::JAW_OPEN]);
            let _ = bus.push_blendshape(*frame);
            let _ = bus.pop_blendshape();
        }
        let allocations = ALLOCS.load(Ordering::Relaxed) - before;

        println!(
            "600 frames of drain + tick + push + pop: {allocations} allocations"
        );
        assert_eq!(
            allocations, 0,
            "{allocations} allocations occurred in the hot loop over 600 frames"
        );
    }

    // -----------------------------------------------------------------
    // Frame content
    // -----------------------------------------------------------------

    #[test]
    fn every_frame_is_in_unit_range() {
        let bus = MirandaBus::in_memory();
        let mut d = FaceDispatcher::from_seed(11);
        for i in 0..3_600 {
            if i % 2 == 0 {
                let _ = bus.push_audio(speech_chunk(i));
            }
            d.drain_audio(&bus);
            let f = d.tick(FRAME_DT, i * 16_667, None);
            for (c, w) in f.weights.iter().enumerate() {
                assert!(
                    (0.0..=1.0).contains(w),
                    "frame {i} channel {c} = {w} out of range"
                );
            }
        }
    }

    /// Timestamps must increase strictly and monotonically. This is the only
    /// mechanism the consumer has for detecting a dropped frame, since the
    /// 216-byte `BlendshapeFrame` layout carries no sequence counter (see
    /// WO-3's note on why it was not extended).
    #[test]
    fn frame_timestamps_are_strictly_monotonic() {
        let mut d = FaceDispatcher::from_seed(3);
        let mut prev = 0u64;
        for i in 1..600u64 {
            let ts = i * 16_667;
            let f = d.tick(FRAME_DT, ts, None);
            assert!(
                f.timestamp_us > prev,
                "timestamp went from {prev} to {}",
                f.timestamp_us
            );
            prev = f.timestamp_us;
        }
    }

    /// **No-Loop Video Protocol at the composed layer.** Zero motion for
    /// more than one frame interval is a defect, so no two consecutive
    /// composed frames may be identical — with no audio at all, which is the
    /// hard case. Blink contributes exactly nothing for seconds at a time;
    /// gaze and respiration have to carry the guarantee.
    #[test]
    fn composed_output_is_never_static_with_no_audio() {
        let mut d = FaceDispatcher::from_seed(19);
        let mut prev = d.tick(FRAME_DT, 0, None).weights;

        // Ten minutes of frames.
        let frames = 60 * 600;
        let mut identical = 0;
        for i in 1..frames {
            let now = d.tick(FRAME_DT, i as u64 * 16_667, None).weights;
            if now == prev {
                identical += 1;
            }
            prev = now;
        }
        assert_eq!(
            identical, 0,
            "{identical} of {frames} consecutive frame pairs were bit-identical \
             over 10 minutes of idle — the face froze"
        );
    }

    /// Non-repetition is stronger than non-staticness: a two-frame
    /// alternation would pass the test above while being an obvious loop.
    /// Sampling every frame's full 52-channel state over ten minutes and
    /// finding no exact repeat is the actual No-Loop bar.
    #[test]
    fn composed_output_never_exactly_repeats_a_previous_frame() {
        use std::collections::HashSet;

        let mut d = FaceDispatcher::from_seed(23);
        let mut seen: HashSet<[u32; BLENDSHAPE_COUNT]> = HashSet::new();
        let frames = 60 * 600;
        let mut repeats = 0;

        for i in 0..frames {
            let w = d.tick(FRAME_DT, i as u64 * 16_667, None).weights;
            // Compare bit patterns: f32 has no Hash, and bitwise identity is
            // the strictest possible reading of "the same frame".
            let mut key = [0u32; BLENDSHAPE_COUNT];
            for (k, v) in key.iter_mut().zip(w.iter()) {
                *k = v.to_bits();
            }
            if !seen.insert(key) {
                repeats += 1;
            }
        }

        println!(
            "{frames} idle frames over 10 minutes: {} distinct, {repeats} repeats",
            seen.len()
        );
        assert_eq!(
            repeats, 0,
            "{repeats} frames exactly repeated an earlier frame — the idle \
             animation is looping"
        );
    }

    #[test]
    fn audio_drives_the_mouth_and_silence_relaxes_it() {
        let bus = MirandaBus::in_memory();
        let mut d = FaceDispatcher::from_seed(5);

        for i in 0..120u64 {
            let _ = bus.push_audio(speech_chunk(i));
            let _ = bus.push_audio(speech_chunk(i));
            d.drain_audio(&bus);
            d.tick(FRAME_DT, i * 16_667, None);
        }
        let speaking = d.frame().weights[arkit::JAW_OPEN];

        for i in 120..300u64 {
            d.drain_audio(&bus);
            d.tick(FRAME_DT, i * 16_667, None);
        }
        let silent = d.frame().weights[arkit::JAW_OPEN];

        println!("jawOpen: speaking {speaking:.4}, after silence {silent:.4}");
        assert!(
            speaking > 0.15,
            "audio did not open the jaw ({speaking}) — the solver is not \
             receiving the drained samples"
        );
        assert!(
            silent < speaking * 0.5,
            "jaw stayed at {silent} after silence (was {speaking}) — the mouth \
             is holding a vowel"
        );
        // Breath keeps a small resting bias, so it must not fall to exactly
        // zero either.
        assert!(silent > 0.0, "jaw fell to a dead zero — respiration is missing");
    }

    #[test]
    fn external_speech_overrides_the_internal_solver() {
        let bus = MirandaBus::in_memory();
        let mut d = FaceDispatcher::from_seed(13);

        let mut polly = [0.0f32; BLENDSHAPE_COUNT];
        polly[arkit::MOUTH_PUCKER] = 0.8;

        for i in 0..120u64 {
            // Loud audio present the whole time. If the external override is
            // ignored, the solver's shape appears instead.
            let _ = bus.push_audio(speech_chunk(i));
            d.drain_audio(&bus);
            d.tick(FRAME_DT, i * 16_667, Some(&polly));
        }

        let w = d.frame().weights;
        assert!(
            w[arkit::MOUTH_PUCKER] > 0.7,
            "external pucker 0.8 produced {} — override ignored",
            w[arkit::MOUTH_PUCKER]
        );
        // The autonomic layer must still be running underneath.
        assert!(
            w[arkit::JAW_OPEN] > 0.0,
            "autonomic breath vanished when speech came from outside"
        );
    }

    // -----------------------------------------------------------------
    // Audio draining
    // -----------------------------------------------------------------

    #[test]
    fn drain_reads_all_available_chunks_up_to_the_cap() {
        let bus = MirandaBus::in_memory();
        let mut d = FaceDispatcher::from_seed(1);

        for i in 0..3u64 {
            bus.push_audio(speech_chunk(i)).unwrap();
        }
        assert_eq!(d.drain_audio(&bus), 3);
        assert_eq!(d.staged_audio().len(), 3 * AUDIO_CHUNK_FRAMES);

        // Nothing left.
        assert_eq!(d.drain_audio(&bus), 0);
        assert_eq!(d.staged_audio().len(), 0);
    }

    #[test]
    fn drain_is_bounded_when_the_ring_is_flooded() {
        let bus = MirandaBus::in_memory();
        let mut d = FaceDispatcher::from_seed(1);
        // Push until the ring refuses more.
        let mut pushed = 0;
        for i in 0..10_000u64 {
            if bus.push_audio(speech_chunk(i)).is_err() {
                break;
            }
            pushed += 1;
        }
        assert!(pushed > MAX_CHUNKS_PER_FRAME, "ring too small to test the cap");

        let taken = d.drain_audio(&bus);
        assert!(
            taken <= MAX_CHUNKS_PER_FRAME,
            "drained {taken} chunks, over the {MAX_CHUNKS_PER_FRAME} cap"
        );
    }

    /// A producer reporting a `frame_count` larger than the chunk can hold
    /// must not cause an out-of-bounds read. The field crosses a shared
    /// memory boundary, so it is untrusted input.
    #[test]
    fn oversized_frame_count_is_clamped_not_trusted() {
        let bus = MirandaBus::in_memory();
        let mut d = FaceDispatcher::from_seed(1);

        let mut c = speech_chunk(0);
        c.frame_count = u32::MAX;
        bus.push_audio(c).unwrap();

        assert_eq!(d.drain_audio(&bus), 1);
        assert_eq!(
            d.staged_audio().len(),
            AUDIO_CHUNK_FRAMES,
            "frame_count was trusted over the real array length"
        );
    }

    #[test]
    fn staging_buffer_cannot_overflow() {
        let bus = MirandaBus::in_memory();
        let mut d = FaceDispatcher::from_seed(1);
        // MAX_CHUNKS_PER_FRAME * 160 = 1920 samples against a 1600 capacity,
        // so the cap alone does not protect the buffer — the room check must.
        for i in 0..MAX_CHUNKS_PER_FRAME as u64 {
            let _ = bus.push_audio(speech_chunk(i));
        }
        d.drain_audio(&bus);
        assert!(d.staged_audio().len() <= AUDIO_STAGING_SAMPLES);
    }

    // -----------------------------------------------------------------
    // Determinism
    // -----------------------------------------------------------------

    /// Reproducibility for a given seed. T9's verification and any future
    /// frame-capture regression depend on it, and it is the property the
    /// sequential-oscillator decision buys.
    #[test]
    fn same_seed_produces_identical_frame_sequences() {
        let run = |seed: u64| {
            let mut d = FaceDispatcher::from_seed(seed);
            (0..300)
                .map(|i| d.tick(FRAME_DT, i as u64 * 16_667, None).weights)
                .collect::<Vec<_>>()
        };
        assert_eq!(run(42), run(42), "dispatcher is not reproducible per seed");
        assert_ne!(run(42), run(43), "different seeds produced identical output");
    }

    /// Generators must be seeded differently from each other. If they shared
    /// a seed their noise fields would correlate and blinks would tend to
    /// land on breath peaks — a periodicity the No-Loop Protocol forbids.
    #[test]
    fn generators_are_decorrelated_within_one_dispatcher() {
        let mut d = FaceDispatcher::from_seed(77);
        let mut gaze_series = Vec::new();
        let mut breath_series = Vec::new();
        for i in 0..(60 * 120) {
            let w = d.tick(FRAME_DT, i as u64 * 16_667, None).weights;
            gaze_series.push(w[arkit::EYE_LOOK_IN_LEFT] - w[arkit::EYE_LOOK_OUT_LEFT]);
            breath_series.push(w[arkit::JAW_OPEN]);
        }

        let corr = correlation(&gaze_series, &breath_series);
        println!("gaze/breath correlation over 2 min: {corr:.4}");
        assert!(
            corr.abs() < 0.35,
            "gaze and respiration correlate at {corr} — the generators are \
             sharing a seed, so the idle animation has a hidden period"
        );
    }

    fn correlation(a: &[f32], b: &[f32]) -> f32 {
        let n = a.len() as f32;
        let ma = a.iter().sum::<f32>() / n;
        let mb = b.iter().sum::<f32>() / n;
        let mut num = 0.0;
        let mut da = 0.0;
        let mut db = 0.0;
        for (x, y) in a.iter().zip(b.iter()) {
            let dx = x - ma;
            let dy = y - mb;
            num += dx * dy;
            da += dx * dx;
            db += dy * dy;
        }
        if da <= f32::EPSILON || db <= f32::EPSILON {
            return 0.0;
        }
        num / (da.sqrt() * db.sqrt())
    }

    #[test]
    fn dispatcher_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<FaceDispatcher>();
    }

    // -----------------------------------------------------------------
    // Frame build cost
    // -----------------------------------------------------------------

    /// Real per-frame cost of the composed pipeline. This is the number the
    /// threading decision in the module docs rests on, so it is measured
    /// rather than estimated.
    ///
    /// Enforced only in release, for the same reason as T7's solver budget:
    /// an unoptimised build says nothing about shipped cost. Run
    /// `cargo test --release -p miranda-nodes bench_frame_cost -- --nocapture`.
    #[test]
    fn bench_frame_cost() {
        const ITERS: usize = 20_000;
        let bus = MirandaBus::in_memory();
        let mut d = FaceDispatcher::from_seed(101);

        for i in 0..2_000u64 {
            let _ = bus.push_audio(speech_chunk(i));
            d.drain_audio(&bus);
            d.tick(FRAME_DT, i, None);
        }

        let start = Instant::now();
        for i in 0..ITERS as u64 {
            let _ = bus.push_audio(speech_chunk(i));
            d.drain_audio(&bus);
            let f = d.tick(FRAME_DT, i * 16_667, None);
            std::hint::black_box(f.weights[arkit::JAW_OPEN]);
        }
        let per_frame_us = start.elapsed().as_secs_f64() * 1e6 / ITERS as f64;

        println!(
            "composed frame: {per_frame_us:.3} us (drain + 3 oscillators + \
             acoustic solver + compositor + damper) = {:.4}% of the \
             16666.7 us budget",
            per_frame_us / 16_666.7 * 100.0
        );

        if cfg!(debug_assertions) {
            println!("NOTE: debug build — budget NOT enforced.");
        } else {
            assert!(
                per_frame_us < 16_666.7,
                "frame build cost {per_frame_us} us exceeds the frame period"
            );
        }
    }

    // -----------------------------------------------------------------
    // The real paced thread
    // -----------------------------------------------------------------

    /// Runs the actual thread against a real bus and checks the observed
    /// rate. This is the end-to-end check that pacing works — everything
    /// above tests the frame builder in isolation, which cannot catch a
    /// pacer that sleeps for the wrong duration.
    #[test]
    fn paced_thread_publishes_at_about_sixty_hertz() {
        let bus = Arc::new(MirandaBus::in_memory());
        let consumer = Arc::clone(&bus);

        // A consumer is required: the blendshape ring is small, and without
        // a drain the run would measure backpressure instead of pacing.
        let drain_stop = Arc::new(AtomicBool::new(false));
        let drain_flag = Arc::clone(&drain_stop);
        let drainer = thread::spawn(move || {
            let mut count = 0u64;
            while !drain_flag.load(Ordering::Relaxed) {
                while consumer.pop_blendshape().is_some() {
                    count += 1;
                }
                thread::sleep(Duration::from_millis(2));
            }
            while consumer.pop_blendshape().is_some() {
                count += 1;
            }
            count
        });

        let dispatcher = DispatcherThread::spawn(Arc::clone(&bus), 55).unwrap();
        let run_for = Duration::from_secs(3);
        let started = Instant::now();
        thread::sleep(run_for);
        let elapsed = started.elapsed();

        let stats = Arc::clone(dispatcher.stats());
        dispatcher.stop();
        drain_stop.store(true, Ordering::Relaxed);
        let consumed = drainer.join().unwrap();

        let published = stats.frames_published.load(Ordering::Relaxed);
        let dropped = stats.frames_dropped.load(Ordering::Relaxed);
        let late = stats.late_frames.load(Ordering::Relaxed);
        let failures = stats.publish_failures.load(Ordering::Relaxed);
        let observed_fps = published as f64 / elapsed.as_secs_f64();

        println!(
            "paced {:.2}s: {published} published ({observed_fps:.2} fps), \
             {dropped} dropped, {late} late, {failures} publish failures, \
             {consumed} consumed; build mean {:.3} us max {:.3} us",
            elapsed.as_secs_f64(),
            stats.mean_build_us().unwrap_or(0.0),
            stats.max_build_us()
        );

        // A wide band on purpose: this is a general-purpose desktop kernel
        // on a dual-core Celeron with no real-time priority, so a strict
        // bound here would be a flaky test rather than a meaningful one.
        // What it is really checking is that the pacer is in the right
        // order of magnitude — a broken one lands at 1000 fps (no sleep) or
        // 30 fps (double period), not at 58.
        assert!(
            (45.0..=62.0).contains(&observed_fps),
            "observed {observed_fps:.2} fps, expected roughly 60"
        );
        assert!(published > 0, "the thread published nothing");
        assert!(consumed > 0, "the consumer saw nothing on the bus");
    }

    /// Stopping must actually stop, and must not hang. A pacer that sleeps a
    /// full period without checking the stop flag would make shutdown take
    /// as long as one frame; one that waits on something unsignalled would
    /// hang forever.
    #[test]
    fn thread_stops_promptly() {
        let bus = Arc::new(MirandaBus::in_memory());
        let dispatcher = DispatcherThread::spawn(bus, 9).unwrap();
        thread::sleep(Duration::from_millis(120));

        let start = Instant::now();
        dispatcher.stop();
        let shutdown = start.elapsed();

        println!("shutdown took {:.2} ms", shutdown.as_secs_f64() * 1000.0);
        assert!(
            shutdown < Duration::from_millis(200),
            "shutdown took {shutdown:?}"
        );
    }

    /// Dropping the handle must join the thread rather than leaking it.
    #[test]
    fn dropping_the_handle_stops_the_thread() {
        let bus = Arc::new(MirandaBus::in_memory());
        let published_after_drop = {
            let dispatcher = DispatcherThread::spawn(Arc::clone(&bus), 4).unwrap();
            thread::sleep(Duration::from_millis(120));
            let stats = Arc::clone(dispatcher.stats());
            drop(dispatcher);
            // If drop did not join, the loop would still be running and this
            // count would keep climbing.
            let at_drop = stats.frames_published.load(Ordering::Relaxed);
            thread::sleep(Duration::from_millis(120));
            stats.frames_published.load(Ordering::Relaxed) - at_drop
        };
        assert_eq!(
            published_after_drop, 0,
            "{published_after_drop} frames were published after the handle was \
             dropped — the thread outlived its owner"
        );
    }

    /// A stalled consumer must show up as `publish_failures` and must not
    /// stall the face. Backpressure on the render side is a real operating
    /// condition, not an error to crash on.
    #[test]
    fn a_stalled_consumer_produces_publish_failures_not_a_stall() {
        let bus = Arc::new(MirandaBus::in_memory());

        // Fill the blendshape ring before starting, so backpressure is the
        // condition under test from the first frame. Waiting for the
        // dispatcher to fill 128 slots itself would take over two seconds of
        // real time and make this a slow test measuring the same thing.
        let filler = BlendshapeFrame {
            timestamp_us: 0,
            weights: [0.0; BLENDSHAPE_COUNT],
        };
        let mut prefilled = 0;
        while bus.push_blendshape(filler).is_ok() {
            prefilled += 1;
            assert!(prefilled < 100_000, "ring never reported full");
        }
        println!("pre-filled the blendshape ring with {prefilled} frames");

        let dispatcher = DispatcherThread::spawn(Arc::clone(&bus), 31).unwrap();
        // No consumer at all, and the ring is already full, so every push
        // must fail.
        thread::sleep(Duration::from_millis(600));

        let stats = Arc::clone(dispatcher.stats());
        dispatcher.stop();

        let failures = stats.publish_failures.load(Ordering::Relaxed);
        let published = stats.frames_published.load(Ordering::Relaxed);
        let dropped = stats.frames_dropped.load(Ordering::Relaxed);
        println!(
            "no consumer: {published} published, {failures} publish failures, \
             {dropped} dropped"
        );
        assert!(
            failures > 0,
            "the ring should have filled with no consumer draining it"
        );
        // The loop must have kept its cadence regardless. Roughly 36 frames
        // fit in 600 ms; requiring most of them rules out a stall.
        assert!(
            published + failures > 25,
            "only {} frames were attempted in 600 ms — the loop stalled on \
             backpressure",
            published + failures
        );
        assert_eq!(dropped, 0, "backpressure caused {dropped} dropped frames");
    }
}
