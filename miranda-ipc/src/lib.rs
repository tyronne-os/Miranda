//! miranda-ipc — Work Order 1: the POSIX shared-memory ring buffer at
//! `/dev/shm/miranda_bus`, lock-free atomic head/tail pointers, and C-ABI
//! aligned payload structs for audio chunks, 52-channel ARKit blendshape
//! frames, and 9-coefficient spherical harmonic vectors.
//!
//! # What this is
//!
//! Three independent single-producer / single-consumer (SPSC) ring buffers
//! sharing one memory mapping. Each ring is lock-free: coordination is a pair
//! of `AtomicUsize` counters (`head` = read position, `tail` = write
//! position), never a mutex. The bus is on the critical path of a 60 FPS
//! render loop, so a mutex held by a stalled audio thread would cause the
//! renderer to miss its frame deadline. That is a latency constraint, not a
//! style preference (WO-1 REQ-3).
//!
//! # Concurrency contract — read this before using the bus
//!
//! Each ring is **SPSC**: at most one producer thread and at most one
//! consumer thread per ring, concurrently. The three rings are independent,
//! so an audio producer, a blendshape producer, and a renderer consumer may
//! all run concurrently without coordination. What is **not** supported is
//! two threads pushing to the *same* ring — that would race on `tail` and
//! silently interleave slot writes. If a future Work Order needs multiple
//! producers on one ring, it needs a different algorithm (MPSC), not a
//! tweak to this one.
//!
//! # Layout
//!
//! ```text
//! offset 0    : audio head   (AtomicUsize, own 64-byte cache line)
//! offset 64   : audio tail   (AtomicUsize, own 64-byte cache line)
//! offset 128  : AudioChunk slots × 64
//! ...         : blendshape head / tail / BlendshapeFrame slots × 128
//! ...         : sh head / tail / SphericalHarmonics slots × 128
//! ```
//!
//! `head` and `tail` for a given ring are deliberately placed on **separate
//! 64-byte cache lines**. If they shared a line, the producer's store to
//! `tail` would invalidate the consumer's cached copy of `head` on every
//! single push (and vice versa) — "false sharing" — which turns two
//! independent atomic counters into a ping-pong of cache-coherency traffic
//! and can cost an order of magnitude in throughput. This matters most on
//! ARM64/Graviton, where the memory model requires real fence instructions
//! rather than x86's near-free TSO ordering.

use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytemuck::Pod;
use memmap2::MmapMut;
use miranda_core::{AudioChunk, BackpressureError, BlendshapeFrame, RingId, SphericalHarmonics};

/// Canonical path for the production bus. `/dev/shm` is a tmpfs mount
/// (RAM-backed), which is what makes the ≤50 μs round-trip target
/// achievable — a regular on-disk file cannot meet it (WO-1 REQ-2).
pub const BUS_PATH: &str = "/dev/shm/miranda_bus";

/// Cache line size assumed for false-sharing avoidance. 64 bytes on both
/// x86-64 and the Graviton (Neoverse) cores this project targets.
const CACHE_LINE: usize = 64;

/// Bytes reserved for one ring's control block: `head` on its own cache
/// line, then `tail` on its own cache line.
const CTRL_BYTES: usize = 2 * CACHE_LINE;

/// Slot counts. All powers of two so the modulo that maps a monotonic
/// counter to a slot index becomes a single bitmask (`counter & (slots - 1)`)
/// instead of an integer division on the hot path.
const AUDIO_SLOTS: usize = 64;
const BLEND_SLOTS: usize = 128;
const SH_SLOTS: usize = 128;

/// Rounds `value` up to the next multiple of `align`. `align` must be a
/// power of two.
const fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

// Region offsets, all derived from `size_of` rather than hardcoded — the
// design doc's stated byte sizes are informational and one of them (44 for
// SphericalHarmonics) is arithmetically wrong once `#[repr(C)]` alignment is
// applied. Deriving from `size_of` makes that impossible to get wrong here.
const AUDIO_CTRL_OFF: usize = 0;
const AUDIO_DATA_OFF: usize = align_up(AUDIO_CTRL_OFF + CTRL_BYTES, CACHE_LINE);
const AUDIO_DATA_BYTES: usize = AUDIO_SLOTS * std::mem::size_of::<AudioChunk>();

const BLEND_CTRL_OFF: usize = align_up(AUDIO_DATA_OFF + AUDIO_DATA_BYTES, CACHE_LINE);
const BLEND_DATA_OFF: usize = align_up(BLEND_CTRL_OFF + CTRL_BYTES, CACHE_LINE);
const BLEND_DATA_BYTES: usize = BLEND_SLOTS * std::mem::size_of::<BlendshapeFrame>();

const SH_CTRL_OFF: usize = align_up(BLEND_DATA_OFF + BLEND_DATA_BYTES, CACHE_LINE);
const SH_DATA_OFF: usize = align_up(SH_CTRL_OFF + CTRL_BYTES, CACHE_LINE);
const SH_DATA_BYTES: usize = SH_SLOTS * std::mem::size_of::<SphericalHarmonics>();

/// Total size of the shared-memory region, rounded up to a cache line.
pub const BUS_TOTAL_BYTES: usize = align_up(SH_DATA_OFF + SH_DATA_BYTES, CACHE_LINE);

// Compile-time proof of every layout invariant the unsafe code below relies
// on. If any of these ever becomes false (someone adds a field, changes a
// slot count to a non-power-of-two, reorders a region), this fails the build
// rather than producing a misaligned atomic or a torn read at runtime.
const _: () = {
    assert!(AUDIO_SLOTS.is_power_of_two());
    assert!(BLEND_SLOTS.is_power_of_two());
    assert!(SH_SLOTS.is_power_of_two());

    // Control blocks must be aligned for AtomicUsize.
    assert!(AUDIO_CTRL_OFF % std::mem::align_of::<AtomicUsize>() == 0);
    assert!(BLEND_CTRL_OFF % std::mem::align_of::<AtomicUsize>() == 0);
    assert!(SH_CTRL_OFF % std::mem::align_of::<AtomicUsize>() == 0);
    // ...and head/tail must not share a cache line.
    assert!(CTRL_BYTES >= 2 * CACHE_LINE);
    assert!(std::mem::size_of::<AtomicUsize>() <= CACHE_LINE);

    // Every slot region must be aligned for its payload type, and every
    // slot within it too (which follows if the region is aligned and the
    // slot size is a multiple of the payload alignment).
    assert!(AUDIO_DATA_OFF % std::mem::align_of::<AudioChunk>() == 0);
    assert!(std::mem::size_of::<AudioChunk>() % std::mem::align_of::<AudioChunk>() == 0);
    assert!(BLEND_DATA_OFF % std::mem::align_of::<BlendshapeFrame>() == 0);
    assert!(std::mem::size_of::<BlendshapeFrame>() % std::mem::align_of::<BlendshapeFrame>() == 0);
    assert!(SH_DATA_OFF % std::mem::align_of::<SphericalHarmonics>() == 0);
    assert!(
        std::mem::size_of::<SphericalHarmonics>() % std::mem::align_of::<SphericalHarmonics>() == 0
    );

    // Regions must not overlap.
    assert!(AUDIO_DATA_OFF + AUDIO_DATA_BYTES <= BLEND_CTRL_OFF);
    assert!(BLEND_DATA_OFF + BLEND_DATA_BYTES <= SH_CTRL_OFF);
    assert!(SH_DATA_OFF + SH_DATA_BYTES <= BUS_TOTAL_BYTES);
};

/// A 64-byte, 64-byte-aligned block. Used only to obtain a cache-line-aligned
/// heap allocation for the in-memory backing (see [`MirandaBus::in_memory`]);
/// a plain `Vec<u8>` only guarantees 1-byte alignment, which is not enough to
/// place an `AtomicUsize` soundly.
#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct CacheLineBlock([u8; CACHE_LINE]);

/// Where the bus bytes actually live.
///
/// Neither variant's payload is read after construction — `base` is the
/// pointer everything else uses. This enum exists purely to keep the
/// backing storage (`MmapMut` or `Vec`) alive for as long as `MirandaBus`
/// does, so `base` never dangles. `#[allow(dead_code)]` documents that this
/// is intentional (an owner held only for its `Drop`/liveness side effect),
/// not an oversight — the alternative would be a `Box<dyn Any>` that hides
/// the same fact behind a vtable instead of stating it plainly.
#[allow(dead_code)]
enum Backing {
    /// Production: a `mmap` of a tmpfs file, shareable across processes.
    Mmap(MmapMut),
    /// Testing: a private heap allocation. Needed because MIRI cannot
    /// execute the `mmap` syscall, so the ring-buffer logic would otherwise
    /// be entirely unverifiable by Rust's undefined-behaviour checker.
    /// Same layout, same code paths, no syscall.
    Heap(Vec<CacheLineBlock>),
}

/// The Miranda IPC bus: three lock-free SPSC ring buffers over one shared
/// memory region.
pub struct MirandaBus {
    /// Kept alive for as long as the bus exists; `base` points into it.
    _backing: Backing,
    /// Base address of the region. Valid for `BUS_TOTAL_BYTES`.
    base: *mut u8,
}

// SAFETY: `MirandaBus` contains a raw pointer, which makes it neither `Send`
// nor `Sync` by default. Both are sound here:
//
// - The pointed-to region stays valid and at a fixed address for the whole
//   life of the `MirandaBus`, because `_backing` owns it and neither variant
//   stores the bytes inline: `MmapMut` refers to an OS mapping, and
//   `Vec<CacheLineBlock>` refers to a heap allocation. Moving the
//   `MirandaBus` therefore does not move the bytes, so `base` cannot dangle.
//   Nothing after construction ever mutates `_backing` (no push/reallocate),
//   so the heap buffer cannot be reallocated out from under `base`.
// - All shared mutation goes through `AtomicUsize` (interior mutability, safe
//   under `&self`) or through raw-pointer writes to slots that the SPSC
//   protocol guarantees are exclusively owned by the calling side at that
//   moment — see the `// SAFETY:` comment in `ring_push`.
//
// If this were violated — e.g. by adding a method that reallocates the Vec,
// or by allowing two producers on one ring — the result would be a dangling
// pointer or a data race on slot bytes, i.e. undefined behaviour.
unsafe impl Send for MirandaBus {}
unsafe impl Sync for MirandaBus {}

impl MirandaBus {
    /// Opens the production bus at [`BUS_PATH`], creating it if absent.
    ///
    /// A freshly created file is zero-filled by the kernel, and all-zero is a
    /// valid initial state for this layout (`head == tail == 0`, i.e. empty;
    /// all slots zeroed). That means no explicit initialisation pass is
    /// needed, which conveniently sidesteps the "how do you initialise an
    /// atomic you did not allocate" problem entirely.
    ///
    /// Reopening an existing bus inherits whatever `head`/`tail` the previous
    /// user left. That is consistent (never torn) but the contents are stale;
    /// callers that need a pristine bus should remove the file first or use
    /// [`MirandaBus::open_or_create_at`] with a unique path.
    pub fn open_or_create() -> io::Result<Self> {
        Self::open_or_create_at(BUS_PATH)
    }

    /// Same as [`MirandaBus::open_or_create`] but at an explicit path.
    ///
    /// Tests must use this with a unique path per test: `cargo test` runs
    /// tests in parallel by default, and two tests sharing one bus would
    /// interleave pushes on the same ring, violating the SPSC contract and
    /// producing spurious failures that look like a ring-buffer bug.
    pub fn open_or_create_at<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.set_len(BUS_TOTAL_BYTES as u64)?;

        // SAFETY: `MmapMut::map_mut` is unsafe because the mapping's contents
        // can be mutated by any other process with access to the same file,
        // so Rust cannot statically guarantee exclusivity. That is precisely
        // the intent here — this is an interprocess bus. Every access below
        // goes through atomics or raw-pointer copies sized within
        // `BUS_TOTAL_BYTES`, and `set_len` above guarantees the file is at
        // least that large, so no access can fall outside the mapping. If the
        // file were shorter than `BUS_TOTAL_BYTES`, touching a slot past the
        // end would fault (SIGBUS).
        let mut mmap = unsafe { MmapMut::map_mut(&file)? };
        let base = mmap.as_mut_ptr();
        Ok(Self {
            _backing: Backing::Mmap(mmap),
            base,
        })
    }

    /// Creates a private, process-local bus on the heap with the identical
    /// layout and code paths, but no `mmap` syscall.
    ///
    /// This exists so the ring-buffer logic is verifiable under MIRI. MIRI
    /// interprets Rust rather than executing real syscalls, so it cannot run
    /// `mmap` at all — without this backing, the atomic ordering and pointer
    /// arithmetic in `ring_push`/`ring_pop` could never be checked for
    /// undefined behaviour, which for a lock-free structure is the only check
    /// that really matters.
    pub fn in_memory() -> Self {
        let blocks = BUS_TOTAL_BYTES / CACHE_LINE;
        debug_assert_eq!(BUS_TOTAL_BYTES % CACHE_LINE, 0);
        let mut backing = vec![CacheLineBlock([0u8; CACHE_LINE]); blocks];
        let base = backing.as_mut_ptr() as *mut u8;
        Self {
            _backing: Backing::Heap(backing),
            base,
        }
    }

    /// Borrows the `head` counter (read position) of the ring whose control
    /// block starts at `ctrl_off`.
    fn head(&self, ctrl_off: usize) -> &AtomicUsize {
        // SAFETY: Casting `base + ctrl_off` to `*const AtomicUsize` and
        // dereferencing it is sound because:
        //
        // - Alignment: `base` is page-aligned (mmap) or 64-byte aligned
        //   (`CacheLineBlock`), and `ctrl_off` is one of the three
        //   `*_CTRL_OFF` constants, each asserted at compile time above to be
        //   a multiple of `align_of::<AtomicUsize>()`. So the address is
        //   correctly aligned for an atomic.
        // - Validity: `ctrl_off + size_of::<AtomicUsize>()` is well inside
        //   `BUS_TOTAL_BYTES` (asserted above: regions do not overlap and all
        //   fit), and the backing is guaranteed to be at least that large.
        // - Initialisation: the region is zero-filled at creation (kernel for
        //   a fresh tmpfs file, `vec![0u8; _]` for the heap backing), and
        //   every bit pattern is a valid `usize`, so the atomic is never read
        //   uninitialised.
        // - Aliasing: the returned `&AtomicUsize` permits concurrent
        //   modification through interior mutability, which is exactly what
        //   two threads (or processes) need here. No `&mut` to this address
        //   is ever created.
        //
        // If alignment were violated, atomic operations on x86 can tear and
        // on ARM64 will fault outright; if the offset were out of bounds we
        // would read or write unmapped memory.
        unsafe { &*(self.base.add(ctrl_off) as *const AtomicUsize) }
    }

    /// Borrows the `tail` counter (write position) of the ring whose control
    /// block starts at `ctrl_off`. Placed one full cache line after `head` to
    /// prevent false sharing between producer and consumer.
    fn tail(&self, ctrl_off: usize) -> &AtomicUsize {
        // SAFETY: Identical reasoning to `head` above. The address is
        // `ctrl_off + CACHE_LINE`; `CACHE_LINE` (64) is a multiple of
        // `align_of::<AtomicUsize>()` (8), so adding it preserves alignment,
        // and `CTRL_BYTES` (128) reserves room for both counters, asserted at
        // compile time to be at least `2 * CACHE_LINE`.
        unsafe { &*(self.base.add(ctrl_off + CACHE_LINE) as *const AtomicUsize) }
    }

    /// Pushes one payload into a ring. Returns `Err` if the ring is full —
    /// never overwrites unread data, never silently drops (WO-1 REQ-6).
    fn ring_push<T: Pod>(
        &self,
        ctrl_off: usize,
        data_off: usize,
        slots: usize,
        ring: RingId,
        value: T,
    ) -> Result<(), BackpressureError> {
        let tail = self.tail(ctrl_off);
        let head = self.head(ctrl_off);

        // Only this (single) producer writes `tail`, so a Relaxed load of our
        // own counter is sufficient — nobody else can change it.
        let current_tail = tail.load(Ordering::Relaxed);

        // `head` is written by the consumer, so this load must be Acquire.
        //
        // Deliberate deviation from design.md, which specifies Relaxed here:
        // Relaxed would be adequate for the *capacity check* alone (a stale,
        // smaller `head` only makes the producer more conservative, never less
        // safe). But this load also has to establish that the consumer has
        // finished reading the slot we are about to overwrite. The consumer
        // signals completion with a Release store to `head`; pairing it with
        // a Relaxed load creates no happens-before edge, so the consumer's
        // reads and our overwrite of the same slot bytes would be a data race
        // — undefined behaviour under the Rust/C++ memory model even though
        // it happens to work on x86's TSO hardware. Acquire creates the edge.
        // This is the textbook SPSC pairing and still satisfies REQ-3's
        // "AcqRel/Acquire/Release, never SeqCst" constraint.
        let current_head = head.load(Ordering::Acquire);

        if current_tail.wrapping_sub(current_head) >= slots {
            return Err(BackpressureError {
                ring,
                capacity: slots,
            });
        }

        let index = current_tail & (slots - 1);
        let slot_off = data_off + index * std::mem::size_of::<T>();

        // SAFETY: Writing `size_of::<T>()` bytes at `base + slot_off`.
        //
        // - Bounds: `index < slots` because of the bitmask, and the compile-
        //   time assertions guarantee `data_off + slots * size_of::<T>()`
        //   fits inside `BUS_TOTAL_BYTES`, so the whole write is in bounds.
        // - Exclusivity: the capacity check above proved
        //   `tail - head < slots`, which means slot `tail & (slots-1)` is not
        //   one of the slots in `[head, tail)` that the consumer may be
        //   reading. Combined with the SPSC contract (one producer per ring),
        //   this producer has exclusive access to this slot until it
        //   publishes below.
        // - Validity of the source: `bytes_of` gives a byte view of a local,
        //   fully-initialised `T: Pod` — `Pod` is the compile-time proof that
        //   `T` has no padding and no invalid bit patterns, so every byte is
        //   meaningful and the round-trip is lossless.
        // - Non-overlap: source is a stack local, destination is inside the
        //   mapping; they cannot overlap.
        //
        // Raw `copy_nonoverlapping` is used rather than building a
        // `&mut [u8]` over the slot: constructing a Rust reference into memory
        // another process may hold a reference to would assert exclusivity we
        // do not actually have. A raw pointer write asserts nothing.
        //
        // If exclusivity were violated (two producers, or a wrong capacity
        // check), the consumer could observe a half-written frame — the exact
        // silent corruption this module exists to prevent.
        unsafe {
            let src = bytemuck::bytes_of(&value);
            std::ptr::copy_nonoverlapping(
                src.as_ptr(),
                self.base.add(slot_off),
                std::mem::size_of::<T>(),
            );
        }

        // Release: publishes the slot write above. Any consumer that observes
        // this new `tail` with an Acquire load is guaranteed to see the fully
        // written slot, never a partial one.
        tail.store(current_tail.wrapping_add(1), Ordering::Release);
        Ok(())
    }

    /// Pops one payload from a ring, or `None` if empty.
    fn ring_pop<T: Pod>(&self, ctrl_off: usize, data_off: usize, slots: usize) -> Option<T> {
        let tail = self.tail(ctrl_off);
        let head = self.head(ctrl_off);

        // Only this (single) consumer writes `head`.
        let current_head = head.load(Ordering::Relaxed);
        // Acquire pairs with the producer's Release store to `tail`; this is
        // what guarantees the slot bytes we are about to read are the fully
        // written ones.
        let current_tail = tail.load(Ordering::Acquire);

        if current_head == current_tail {
            return None;
        }

        let index = current_head & (slots - 1);
        let slot_off = data_off + index * std::mem::size_of::<T>();

        // SAFETY: Reading `size_of::<T>()` bytes at `base + slot_off` into a
        // zeroed local.
        //
        // - Bounds: same argument as `ring_push` — `index < slots` via the
        //   bitmask, region fits inside `BUS_TOTAL_BYTES` by compile-time
        //   assertion.
        // - Exclusivity: `current_head != current_tail` proved this slot is in
        //   the readable range `[head, tail)`. The producer's capacity check
        //   guarantees it will not overwrite any slot in that range, so this
        //   consumer has exclusive read access until it publishes below.
        // - Validity: the Acquire load of `tail` above happens-before this
        //   read and after the producer's slot write, so the bytes are fully
        //   initialised. `T: Pod` guarantees any bit pattern is a valid `T`,
        //   so even a stale-but-complete slot yields a well-formed value.
        // - Destination is a fresh local; source is inside the mapping; no
        //   overlap.
        //
        // `copy_nonoverlapping` into `T::zeroed()` is used instead of
        // `bytemuck::from_bytes`, which would require materialising a
        // `&[u8]` over shared-memory bytes that another process is permitted
        // to write concurrently — a shared Rust reference asserts the
        // referent will not change, which is not true of an interprocess
        // mapping. The raw copy makes no such assertion. (Deliberate
        // deviation from design.md's suggested `from_bytes` call, for
        // soundness; `bytes_of` is still used on the write path as specified.)
        //
        // If exclusivity were violated, this would read a torn frame — for
        // BlendshapeFrame that means EVE's face driven by half of one
        // expression and half of another.
        let value = unsafe {
            let mut out = T::zeroed();
            std::ptr::copy_nonoverlapping(
                self.base.add(slot_off),
                &mut out as *mut T as *mut u8,
                std::mem::size_of::<T>(),
            );
            out
        };

        // Release: publishes that this slot is free. A producer that observes
        // this new `head` with an Acquire load is guaranteed to see our read
        // as complete, so it may safely overwrite the slot.
        head.store(current_head.wrapping_add(1), Ordering::Release);
        Some(value)
    }

    /// Pushes an [`AudioChunk`] onto the audio ring.
    pub fn push_audio(&self, chunk: AudioChunk) -> Result<(), BackpressureError> {
        self.ring_push(
            AUDIO_CTRL_OFF,
            AUDIO_DATA_OFF,
            AUDIO_SLOTS,
            RingId::Audio,
            chunk,
        )
    }

    /// Pops an [`AudioChunk`] from the audio ring.
    pub fn pop_audio(&self) -> Option<AudioChunk> {
        self.ring_pop(AUDIO_CTRL_OFF, AUDIO_DATA_OFF, AUDIO_SLOTS)
    }

    /// Pushes a [`BlendshapeFrame`] onto the blendshape ring.
    pub fn push_blendshape(&self, frame: BlendshapeFrame) -> Result<(), BackpressureError> {
        self.ring_push(
            BLEND_CTRL_OFF,
            BLEND_DATA_OFF,
            BLEND_SLOTS,
            RingId::Blendshape,
            frame,
        )
    }

    /// Pops a [`BlendshapeFrame`] from the blendshape ring.
    pub fn pop_blendshape(&self) -> Option<BlendshapeFrame> {
        self.ring_pop(BLEND_CTRL_OFF, BLEND_DATA_OFF, BLEND_SLOTS)
    }

    /// Pushes a [`SphericalHarmonics`] vector onto the lighting ring.
    pub fn push_sh(&self, sh: SphericalHarmonics) -> Result<(), BackpressureError> {
        self.ring_push(
            SH_CTRL_OFF,
            SH_DATA_OFF,
            SH_SLOTS,
            RingId::SphericalHarmonics,
            sh,
        )
    }

    /// Pops a [`SphericalHarmonics`] vector from the lighting ring.
    pub fn pop_sh(&self) -> Option<SphericalHarmonics> {
        self.ring_pop(SH_CTRL_OFF, SH_DATA_OFF, SH_SLOTS)
    }

    /// Capacity in slots of each ring, as `(audio, blendshape, sh)`.
    /// Exposed for telemetry — THE VANITY shows ring occupancy per node.
    pub const fn capacities() -> (usize, usize, usize) {
        (AUDIO_SLOTS, BLEND_SLOTS, SH_SLOTS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use miranda_core::{BLENDSHAPE_COUNT, SH_COEFF_COUNT};

    fn frame(i: u64) -> BlendshapeFrame {
        BlendshapeFrame {
            timestamp_us: i,
            weights: [i as f32; BLENDSHAPE_COUNT],
        }
    }

    /// Single-slot round trip: the most basic correctness property.
    #[test]
    fn single_round_trip_preserves_bytes() {
        let bus = MirandaBus::in_memory();
        assert!(bus.pop_blendshape().is_none(), "fresh ring must be empty");

        bus.push_blendshape(frame(42)).unwrap();
        let got = bus.pop_blendshape().expect("one frame was pushed");

        assert_eq!(got.timestamp_us, 42);
        assert_eq!(got.weights, [42.0f32; BLENDSHAPE_COUNT]);
        assert!(bus.pop_blendshape().is_none(), "ring must be empty again");
    }

    /// Fills the ring exactly to capacity, confirms the next push is rejected
    /// with backpressure rather than overwriting, then confirms every queued
    /// payload still reads back intact — i.e. a full-ring push did not
    /// corrupt existing contents (WO-1 REQ-6).
    #[test]
    fn full_ring_returns_backpressure_without_corruption() {
        let bus = MirandaBus::in_memory();
        for i in 0..BLEND_SLOTS as u64 {
            bus.push_blendshape(frame(i))
                .unwrap_or_else(|e| panic!("push {i} within capacity must succeed: {e}"));
        }

        let err = bus
            .push_blendshape(frame(9999))
            .expect_err("push beyond capacity must be rejected");
        assert_eq!(err.ring, RingId::Blendshape);
        assert_eq!(err.capacity, BLEND_SLOTS);

        for i in 0..BLEND_SLOTS as u64 {
            let got = bus.pop_blendshape().expect("queued frame must be present");
            assert_eq!(got.timestamp_us, i, "FIFO order and contents preserved");
            assert_eq!(got.weights, [i as f32; BLENDSHAPE_COUNT]);
        }
        assert!(bus.pop_blendshape().is_none());
    }

    /// Drives the monotonic counters well past `slots` so the bitmask
    /// index wraps many times. Catches off-by-one errors in the
    /// `counter & (slots - 1)` mapping that a short test would miss.
    #[test]
    fn index_wraparound_stays_fifo() {
        let bus = MirandaBus::in_memory();
        for i in 0..(BLEND_SLOTS as u64 * 5) {
            bus.push_blendshape(frame(i)).unwrap();
            let got = bus.pop_blendshape().expect("pushed then popped");
            assert_eq!(got.timestamp_us, i, "wraparound must preserve identity");
        }
    }

    /// The three rings must be fully independent — a full audio ring must not
    /// affect blendshape or lighting traffic, and payloads must never appear
    /// on the wrong ring.
    #[test]
    fn rings_are_independent() {
        let bus = MirandaBus::in_memory();

        for i in 0..AUDIO_SLOTS as u64 {
            bus.push_audio(AudioChunk {
                timestamp_us: i,
                sample_rate: miranda_core::AUDIO_SAMPLE_RATE_HZ,
                frame_count: miranda_core::AUDIO_CHUNK_FRAMES as u32,
                samples: [i as f32; miranda_core::AUDIO_CHUNK_FRAMES],
            })
            .unwrap();
        }
        assert!(
            bus.push_audio(AudioChunk {
                timestamp_us: 0,
                sample_rate: 0,
                frame_count: 0,
                samples: [0.0; miranda_core::AUDIO_CHUNK_FRAMES],
            })
            .is_err(),
            "audio ring should now be full"
        );

        // Other rings unaffected.
        bus.push_blendshape(frame(7)).unwrap();
        bus.push_sh(SphericalHarmonics {
            timestamp_us: 7,
            coefficients: [0.5; SH_COEFF_COUNT],
            _padding: [0; 4],
        })
        .unwrap();

        assert_eq!(bus.pop_blendshape().unwrap().timestamp_us, 7);
        let sh = bus.pop_sh().unwrap();
        assert_eq!(sh.timestamp_us, 7);
        assert_eq!(sh.coefficients, [0.5; SH_COEFF_COUNT]);
        assert_eq!(bus.pop_audio().unwrap().timestamp_us, 0, "audio intact");
    }

    /// Two real threads, genuinely concurrent, on the heap backing so MIRI
    /// can check the atomic ordering. MIRI's weak-memory emulation will
    /// report a data race here if the Acquire/Release pairing is wrong —
    /// which is the only way to catch that class of bug, since x86's strong
    /// memory model hides it at runtime.
    #[test]
    fn concurrent_spsc_under_miri() {
        // Kept small: MIRI is orders of magnitude slower than native.
        const N: u64 = 200;
        let bus = std::sync::Arc::new(MirandaBus::in_memory());

        let producer_bus = std::sync::Arc::clone(&bus);
        let producer = std::thread::spawn(move || {
            let mut sent = 0u64;
            while sent < N {
                if producer_bus.push_blendshape(frame(sent)).is_ok() {
                    sent += 1;
                } else {
                    std::thread::yield_now();
                }
            }
        });

        let consumer_bus = std::sync::Arc::clone(&bus);
        let consumer = std::thread::spawn(move || {
            let mut received = 0u64;
            while received < N {
                match consumer_bus.pop_blendshape() {
                    Some(got) => {
                        assert_eq!(
                            got.timestamp_us, received,
                            "SPSC must deliver in FIFO order with no gaps"
                        );
                        assert_eq!(
                            got.weights,
                            [received as f32; BLENDSHAPE_COUNT],
                            "payload must be byte-identical, not torn"
                        );
                        received += 1;
                    }
                    None => std::thread::yield_now(),
                }
            }
            received
        });

        producer.join().expect("producer thread panicked");
        let received = consumer.join().expect("consumer thread panicked");
        assert_eq!(received, N);
    }

    /// WO-1 REQ-5 / acceptance criteria: a real concurrent test, two actual
    /// OS threads, one pushing 1,000 `BlendshapeFrame`s with incrementing
    /// timestamps, one popping and asserting byte-identical contents via
    /// `assert_eq!`. Runs at native speed (unlike `concurrent_spsc_under_miri`,
    /// which is deliberately small so MIRI's interpreter can finish it) and
    /// prints per-thread evidence so the interleaving is visible in
    /// `--nocapture` output, not just asserted silently.
    #[test]
    fn test_blendshape_round_trip_concurrent() {
        const N: u64 = 1_000;
        let bus = std::sync::Arc::new(MirandaBus::in_memory());

        let producer_bus = std::sync::Arc::clone(&bus);
        let producer = std::thread::Builder::new()
            .name("blendshape-writer".into())
            .spawn(move || {
                let start = std::time::Instant::now();
                let mut sent = 0u64;
                while sent < N {
                    // weights filled with the frame index cast to f32, per spec.
                    if producer_bus.push_blendshape(frame(sent)).is_ok() {
                        if sent % 250 == 0 {
                            println!(
                                "[{:?}] writer pushed frame {sent} at {:?} since start",
                                std::thread::current().name().unwrap_or("?"),
                                start.elapsed()
                            );
                        }
                        sent += 1;
                    } else {
                        std::thread::yield_now();
                    }
                }
                println!(
                    "[{:?}] writer done: {N} frames in {:?}",
                    std::thread::current().name().unwrap_or("?"),
                    start.elapsed()
                );
            })
            .expect("spawn writer thread");

        let consumer_bus = std::sync::Arc::clone(&bus);
        let consumer = std::thread::Builder::new()
            .name("blendshape-reader".into())
            .spawn(move || {
                let start = std::time::Instant::now();
                let mut received = 0u64;
                while received < N {
                    match consumer_bus.pop_blendshape() {
                        Some(got) => {
                            assert_eq!(
                                got.timestamp_us, received,
                                "byte-identical round trip: timestamp must match what was pushed"
                            );
                            assert_eq!(
                                got.weights,
                                [received as f32; BLENDSHAPE_COUNT],
                                "byte-identical round trip: weights must match what was pushed"
                            );
                            if received % 250 == 0 {
                                println!(
                                    "[{:?}] reader popped frame {received} at {:?} since start",
                                    std::thread::current().name().unwrap_or("?"),
                                    start.elapsed()
                                );
                            }
                            received += 1;
                        }
                        None => std::thread::yield_now(),
                    }
                }
                println!(
                    "[{:?}] reader done: {N} frames in {:?}",
                    std::thread::current().name().unwrap_or("?"),
                    start.elapsed()
                );
                received
            })
            .expect("spawn reader thread");

        producer.join().expect("writer thread panicked");
        let received = consumer.join().expect("reader thread panicked");
        assert_eq!(received, N, "reader must observe every frame the writer sent");
    }

    /// WO-1 REQ-1 / performance target: measures real mean round-trip
    /// latency over 10,000 push+pop cycles and asserts it stays within the
    /// ≤50 μs budget. If this fails, the most likely cause is false sharing
    /// between the head/tail atomics and slot data — verify the
    /// `#[repr(align(64))]`-equivalent cache-line separation in `CTRL_BYTES`
    /// is actually being honoured (see `layout_is_internally_consistent`).
    ///
    /// Deliberately native-only: this measures wall-clock performance, which
    /// MIRI's interpreter cannot represent (it reports emulated, not real,
    /// timing) — this test is meaningless under `cargo miri test` and is not
    /// run there.
    #[test]
    #[cfg(not(miri))]
    fn test_round_trip_latency() {
        let bus = MirandaBus::in_memory();
        let payload = frame(0);

        // Warm up: first-touch page faults / cache effects should not count
        // against the measured budget.
        for _ in 0..1_000 {
            bus.push_blendshape(payload).unwrap();
            let _ = bus.pop_blendshape();
        }

        const ITERS: u32 = 10_000;
        let start = std::time::Instant::now();
        for _ in 0..ITERS {
            bus.push_blendshape(payload).unwrap();
            let _ = bus.pop_blendshape();
        }
        let elapsed = start.elapsed();
        let elapsed_us = elapsed.as_micros() as u64 / ITERS as u64;

        println!(
            "Mean round-trip latency over {ITERS} iterations: {elapsed_us} μs \
             (total {elapsed:?}, target ≤50 μs)"
        );
        assert!(
            elapsed_us <= 50,
            "Round-trip latency {elapsed_us} μs exceeds ≤50 μs target — \
             check for false sharing on head/tail atomics"
        );
    }

    /// Verifies the real mmap-backed path at `/dev/shm`. Skipped under MIRI,
    /// which cannot execute the `mmap` syscall — the logic itself is covered
    /// by the heap-backed tests above.
    #[test]
    #[cfg_attr(miri, ignore = "MIRI cannot execute the mmap syscall")]
    fn mmap_backed_round_trip() {
        let path = format!("/dev/shm/miranda_bus_test_mmap_{}", std::process::id());
        let _ = std::fs::remove_file(&path);

        {
            let bus = MirandaBus::open_or_create_at(&path).expect("open bus");
            bus.push_blendshape(frame(123)).unwrap();
            let got = bus.pop_blendshape().expect("frame present");
            assert_eq!(got.timestamp_us, 123);
            assert_eq!(got.weights, [123.0f32; BLENDSHAPE_COUNT]);
        }

        // A second opener sees the same region (interprocess semantics,
        // exercised here across two mappings in one process).
        {
            let bus = MirandaBus::open_or_create_at(&path).expect("reopen bus");
            bus.push_audio(AudioChunk {
                timestamp_us: 55,
                sample_rate: miranda_core::AUDIO_SAMPLE_RATE_HZ,
                frame_count: 160,
                samples: [1.5; miranda_core::AUDIO_CHUNK_FRAMES],
            })
            .unwrap();
            assert_eq!(bus.pop_audio().unwrap().timestamp_us, 55);
        }

        std::fs::remove_file(&path).expect("cleanup");
    }

    /// The mapping must be large enough for every region — a short file would
    /// mean slot writes past the end fault with SIGBUS at runtime.
    #[test]
    fn layout_is_internally_consistent() {
        assert_eq!(BUS_TOTAL_BYTES % CACHE_LINE, 0);
        assert!(SH_DATA_OFF + SH_DATA_BYTES <= BUS_TOTAL_BYTES);
        // head and tail really are on different cache lines.
        let bus = MirandaBus::in_memory();
        let h = bus.head(BLEND_CTRL_OFF) as *const AtomicUsize as usize;
        let t = bus.tail(BLEND_CTRL_OFF) as *const AtomicUsize as usize;
        assert_eq!(t - h, CACHE_LINE, "false sharing guard");
        assert_eq!(h % std::mem::align_of::<AtomicUsize>(), 0);
        assert_eq!(t % std::mem::align_of::<AtomicUsize>(), 0);
    }
}
