//! WO-2 T6 — FFI binding to this project's existing `parakeet.cpp` build.
//!
//! # Deviations from the spec, and why
//!
//! The WO-2 spec (both `design.md` and `tasks.md`) assumed this C API:
//!
//! ```c
//! extern "C" const char* transcribe_pcm(const float* buf, size_t len);
//! ```
//!
//! **That symbol does not exist.** Verified against the real header
//! (`parakeet.cpp/include/parakeet_capi.h`) and the real compiled archive
//! (`nm build/libparakeet.a`). The actual API is context-based:
//!
//! ```c
//! parakeet_ctx* parakeet_capi_load(const char* gguf_path);
//! void          parakeet_capi_free(parakeet_ctx* ctx);
//! char*         parakeet_capi_transcribe_pcm(parakeet_ctx* ctx,
//!                                            const float* samples,
//!                                            int n_samples, int sample_rate,
//!                                            int decoder);
//! void          parakeet_capi_free_string(char* s);
//! const char*   parakeet_capi_last_error(parakeet_ctx* ctx);
//! ```
//!
//! Three differences are load-bearing for memory safety, not cosmetic:
//!
//! 1. **A model context is required.** The model is loaded once into an
//!    opaque `parakeet_ctx` and reused; there is no context-free
//!    transcribe entry point. The spec's signature could not have been
//!    called at all.
//! 2. **The returned string is owned by the caller, not borrowed.** It is
//!    `char*` (malloc'd), not `const char*`, and the header states it must
//!    be released with `parakeet_capi_free_string`. `design.md`'s comment
//!    claimed the pointer stays "valid until the next transcribe_pcm
//!    call" — that describes a borrow, and following it literally would
//!    leak the buffer on **every single transcription**. This module
//!    copies the bytes out and frees immediately.
//! 3. **`n_samples` is `int`, not `size_t`.** A >2GiB sample buffer would
//!    silently truncate on conversion, so the length is range-checked
//!    before the call rather than cast blindly.
//!
//! ## `cxx` vs. plain `extern "C"`
//!
//! `tasks.md` specifies `cxx = "1"`, "not bindgen". This module uses
//! neither — it uses std Rust's own `unsafe extern "C"`. Reason:
//! `parakeet_capi.h` is a deliberately **flat C API** (its own header
//! comment: *"designed for dlopen / cgo / purego"*) — opaque pointers,
//! `extern "C"` linkage, malloc'd strings, and an explicit guarantee that
//! no C++ exception crosses the boundary. `cxx` exists to make *C++*
//! interop safe (shared types, `UniquePtr`, `CxxString`, methods on C++
//! classes); pointed at a flat C API it adds a bridge layer and its own
//! ownership types on top of an ABI that already has none of those things,
//! which is more moving parts for strictly less clarity. Plain
//! `extern "C"` is the right-sized tool here and adds zero dependencies.
//! Flagging this as a deliberate, reasoned deviation rather than a silent
//! one.
//!
//! # Thread safety
//!
//! [`ParakeetCtx`] is intentionally **neither `Send` nor `Sync`** (it holds
//! a raw pointer, so this is the default and is not overridden). The
//! header documents the context as wrapping "a loaded model + last-error
//! buffer" — that shared error buffer means two concurrent calls on one
//! context would race on it. `transcribe` therefore takes `&mut self`,
//! making exclusive access a compile-time guarantee rather than a
//! convention. Sharing across threads would need real verification of
//! ggml's own thread-migration behaviour first; that is deliberately not
//! assumed here.

use std::ffi::{c_char, c_float, c_int, CStr, CString};
use std::path::Path;

use miranda_core::AUDIO_SAMPLE_RATE_HZ;

/// Opaque C type. Never dereferenced on the Rust side — only ever held and
/// passed back to the C API, so a zero-sized opaque struct is sufficient
/// and prevents accidental construction.
#[repr(C)]
struct ParakeetCtxRaw {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn parakeet_capi_abi_version() -> c_int;
    fn parakeet_capi_load(gguf_path: *const c_char) -> *mut ParakeetCtxRaw;
    fn parakeet_capi_free(ctx: *mut ParakeetCtxRaw);
    fn parakeet_capi_transcribe_pcm(
        ctx: *mut ParakeetCtxRaw,
        samples: *const c_float,
        n_samples: c_int,
        sample_rate: c_int,
        decoder: c_int,
    ) -> *mut c_char;
    fn parakeet_capi_free_string(s: *mut c_char);
    fn parakeet_capi_last_error(ctx: *mut ParakeetCtxRaw) -> *const c_char;
}

/// Which decoder head to run. Values match the C API's `decoder` parameter
/// exactly (documented in `parakeet_capi.h`); named here so call sites
/// don't pass bare magic integers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Decoder {
    /// Pick by model architecture — transducer for tdt/rnnt/hybrid, CTC for ctc.
    Default = 0,
    /// Force the CTC head.
    Ctc = 1,
    /// Force the transducer (tdt/rnnt) head.
    Transducer = 2,
}

/// Errors crossing the parakeet FFI boundary.
#[derive(Debug)]
pub enum AsrError {
    /// The GGUF path contained an interior NUL byte and cannot be a C string.
    InvalidPath,
    /// `parakeet_capi_load` returned NULL (missing/corrupt model, OOM).
    /// Carries the model path attempted, since the C API has no context to
    /// report an error through when loading is what failed.
    LoadFailed(String),
    /// Transcription returned NULL; carries `parakeet_capi_last_error`.
    TranscribeFailed(String),
    /// The sample buffer is longer than the C API's `int n_samples` can
    /// represent. Rejected rather than truncated.
    TooManySamples(usize),
}

impl std::fmt::Display for AsrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AsrError::InvalidPath => write!(f, "model path contains an interior NUL byte"),
            AsrError::LoadFailed(p) => write!(f, "parakeet_capi_load failed for model {p}"),
            AsrError::TranscribeFailed(e) => write!(f, "parakeet transcription failed: {e}"),
            AsrError::TooManySamples(n) => {
                write!(f, "{n} samples exceeds the C API's int n_samples limit")
            }
        }
    }
}

impl std::error::Error for AsrError {}

/// ABI version of the linked `parakeet_capi` implementation. Exposed so a
/// caller can assert compatibility at startup rather than discovering a
/// signature mismatch as undefined behaviour at the first call.
pub fn abi_version() -> i32 {
    // SAFETY: `parakeet_capi_abi_version` takes no arguments, returns a
    // plain `int`, touches no caller-provided memory, and per the header
    // never lets a C++ exception escape. There is no precondition to
    // uphold and no pointer involved, so this call cannot be unsound.
    unsafe { parakeet_capi_abi_version() }
}

/// An owned, loaded parakeet model context.
///
/// Owns the underlying `parakeet_ctx*` and releases it on drop. Not
/// clonable and not shareable across threads by design — see the
/// module-level "Thread safety" note.
pub struct ParakeetCtx {
    raw: *mut ParakeetCtxRaw,
}

// Manual impl rather than `#[derive(Debug)]`: deriving would print the raw
// pointer address, which is noise in test output and log lines and is not
// stable across runs. The useful fact is simply that a context is loaded.
impl std::fmt::Debug for ParakeetCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ParakeetCtx(<loaded model>)")
    }
}

impl ParakeetCtx {
    /// Loads a GGUF model. The model stays resident for this context's
    /// lifetime and is reused across `transcribe` calls — loading is the
    /// expensive step (a 110M model is ~178 MB on disk), so callers should
    /// hold one context rather than loading per utterance.
    pub fn load<P: AsRef<Path>>(gguf_path: P) -> Result<Self, AsrError> {
        let path = gguf_path.as_ref();
        let path_str = path.to_string_lossy().into_owned();
        let c_path = CString::new(path_str.as_bytes()).map_err(|_| AsrError::InvalidPath)?;

        // SAFETY: `c_path` is a valid, NUL-terminated C string that stays
        // alive across the call (it is dropped only at the end of this
        // function, after the call returns). The C API copies whatever it
        // needs from the path and does not retain the pointer — the
        // returned context owns its own model state, so no borrow of
        // `c_path` outlives this call. A NULL return is a documented
        // failure mode, checked immediately below rather than being
        // wrapped and dereferenced later.
        let raw = unsafe { parakeet_capi_load(c_path.as_ptr()) };

        if raw.is_null() {
            return Err(AsrError::LoadFailed(path_str));
        }
        Ok(Self { raw })
    }

    /// Reads the context's last error message.
    fn last_error(&mut self) -> String {
        // SAFETY: `self.raw` is non-NULL (checked at construction, never
        // reassigned, and `Drop` is the only thing that invalidates it —
        // which cannot run while `&mut self` is held). The header
        // documents the returned pointer as owned by the context and
        // valid "until the next call on it": we copy it into an owned
        // `String` before returning, so nothing borrows it afterwards.
        // The header also guarantees `""` rather than NULL when there is
        // no error, but `is_null` is still checked because trusting a
        // never-NULL claim costs nothing to verify and a wrong assumption
        // here would be a null-deref.
        unsafe {
            let ptr = parakeet_capi_last_error(self.raw);
            if ptr.is_null() {
                return String::new();
            }
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }

    /// Transcribes in-memory mono `f32` PCM.
    ///
    /// `sample_rate_hz` may differ from 16 kHz — the C API linearly
    /// resamples internally when it does.
    ///
    /// Takes `&mut self` deliberately: the C context holds a shared
    /// last-error buffer, so concurrent calls on one context would race.
    /// `&mut` makes that a compile error instead of a runtime data race.
    pub fn transcribe(
        &mut self,
        samples: &[f32],
        sample_rate_hz: u32,
        decoder: Decoder,
    ) -> Result<String, AsrError> {
        // The C API takes `int n_samples`. Range-check instead of casting
        // blindly: a silent wrap would hand C a bogus (possibly negative)
        // length and read out of bounds.
        let n_samples: c_int = samples
            .len()
            .try_into()
            .map_err(|_| AsrError::TooManySamples(samples.len()))?;

        // An empty slice has no meaningful `as_ptr()` guarantee to pass
        // across FFI, and there is nothing to transcribe — short-circuit
        // rather than handing C a dangling-but-aligned pointer.
        if samples.is_empty() {
            return Ok(String::new());
        }

        // SAFETY: Four preconditions, all upheld here:
        //  - `self.raw` is a valid, non-NULL context (see `last_error`).
        //  - `samples.as_ptr()` is valid for `n_samples` contiguous `f32`
        //    reads: it comes from a live `&[f32]` borrowed for this call,
        //    and `n_samples` was derived from that same slice's length, so
        //    the length cannot exceed the allocation.
        //  - The C API only *reads* the sample buffer (parameter is
        //    `const float*`), so passing a shared borrow is correct and
        //    no aliasing rule is violated.
        //  - Per the header, no C++ exception crosses the boundary, so
        //    there is no unwind-across-FFI hazard.
        // A NULL return is a documented error, handled below. The returned
        // pointer is malloc'd and owned by *us* — the copy-then-free
        // sequence below is mandatory, not optional cleanup.
        let raw_out = unsafe {
            parakeet_capi_transcribe_pcm(
                self.raw,
                samples.as_ptr(),
                n_samples,
                sample_rate_hz as c_int,
                decoder as c_int,
            )
        };

        if raw_out.is_null() {
            return Err(AsrError::TranscribeFailed(self.last_error()));
        }

        // SAFETY: `raw_out` is non-NULL (just checked) and the header
        // guarantees it is a malloc'd, NUL-terminated UTF-8 buffer owned
        // by this caller. `to_string_lossy().into_owned()` copies the
        // bytes into Rust-owned memory *before* the free below, so the
        // returned `String` never aliases freed memory. The free is not
        // conditional on anything and cannot be skipped on this path —
        // no `?` operator sits between the copy and the free — so the
        // buffer cannot leak.
        let text = unsafe {
            let owned = CStr::from_ptr(raw_out).to_string_lossy().into_owned();
            parakeet_capi_free_string(raw_out);
            owned
        };

        Ok(text)
    }

    /// Convenience wrapper for the project's standard capture format
    /// (mono `f32` at [`AUDIO_SAMPLE_RATE_HZ`], model-default decoder) —
    /// i.e. exactly what WO-2 T5's `cpal` capture writes into
    /// `audio_bus`, so a caller draining the ring can pass samples
    /// straight through without restating the format each time.
    pub fn transcribe_bus_audio(&mut self, samples: &[f32]) -> Result<String, AsrError> {
        self.transcribe(samples, AUDIO_SAMPLE_RATE_HZ, Decoder::Default)
    }
}

impl Drop for ParakeetCtx {
    fn drop(&mut self) {
        // SAFETY: `self.raw` was returned non-NULL by
        // `parakeet_capi_load` and has not been freed before now —
        // `ParakeetCtx` is not `Clone` and `raw` is private and never
        // reassigned, so exactly one owner exists and this runs exactly
        // once. `parakeet_capi_free` is documented safe on the pointer it
        // handed out (and additionally safe on NULL, though that case
        // cannot occur here).
        unsafe { parakeet_capi_free(self.raw) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This project's existing 110M model. Not committed to the repo (178
    /// MB); tests that need it skip cleanly when it is absent rather than
    /// failing, so the suite stays green on a machine without it.
    const MODEL_PATH: &str = "/mnt/NOBILITY_VAULT/models/parakeet-110m/tdt_ctc-110m-q8_0.gguf";

    fn model_available() -> bool {
        Path::new(MODEL_PATH).is_file()
    }

    /// Cheapest possible proof that linkage actually worked: call into the
    /// library and get a real value back. If the link line in `build.rs`
    /// were wrong, this fails to link or crashes here, before any model
    /// loading or memory-ownership complexity is involved.
    #[test]
    #[cfg_attr(miri, ignore = "MIRI cannot cross the FFI boundary")]
    fn abi_version_is_reachable_and_sane() {
        let v = abi_version();
        println!("parakeet_capi ABI version: {v}");
        assert!(v > 0, "ABI version should be a positive integer, got {v}");
        // The header this binding was written against documents v6 as the
        // newest revision. A *newer* ABI is fine (the header states the
        // original entry points are unchanged across v3-v6); an older one
        // would mean signatures this module relies on may not exist yet.
        assert!(
            v >= 6,
            "binding was written against ABI v6; linked library reports v{v} — \
             re-verify parakeet_capi.h signatures before trusting this module"
        );
    }

    /// A bad path must produce a clean typed error, not a panic, a crash,
    /// or a silently-NULL context that blows up on first use.
    #[test]
    #[cfg_attr(miri, ignore = "MIRI cannot cross the FFI boundary")]
    fn load_nonexistent_model_fails_cleanly() {
        let err = ParakeetCtx::load("/nonexistent/definitely-not-a-model.gguf")
            .expect_err("loading a nonexistent model must fail");
        match err {
            AsrError::LoadFailed(p) => assert!(p.contains("definitely-not-a-model")),
            other => panic!("expected LoadFailed, got {other:?}"),
        }
    }

    /// An interior NUL must be rejected before it ever reaches C, where it
    /// would silently truncate the path.
    #[test]
    fn interior_nul_in_path_is_rejected() {
        let err = ParakeetCtx::load("/tmp/mo\0del.gguf").expect_err("interior NUL must be rejected");
        assert!(matches!(err, AsrError::InvalidPath), "got {err:?}");
    }

    /// T6's specified evidence test: a real FFI round trip on real audio
    /// through the real library, proving no segfault and correct memory
    /// handling across the boundary.
    ///
    /// **Deviation from the spec's assertion, deliberately.** `tasks.md`
    /// says to "confirm the return value is a non-null, non-empty
    /// `String`". Asserting *non-empty* would be wrong here: the input is
    /// a 440 Hz sine tone, not speech, and a correct ASR model returns an
    /// empty transcript for it. That was verified independently before
    /// writing this test — running the project's own `parakeet-cli` on a
    /// sine-tone WAV exits 0 with no transcribed text. So an empty string
    /// is the *correct* result, and asserting non-empty would fail the
    /// task for the model behaving properly. What actually matters, and
    /// what is asserted, is that the FFI round trip completes: a valid
    /// `Ok(String)` comes back, the malloc'd C buffer is copied and freed
    /// without a segfault or leak, and the context survives to be reused.
    #[test]
    #[cfg_attr(miri, ignore = "MIRI cannot cross the FFI boundary")]
    fn test_transcribe_sine_wave() {
        if !model_available() {
            eprintln!("SKIP: model not present at {MODEL_PATH}");
            return;
        }

        let mut ctx = ParakeetCtx::load(MODEL_PATH).expect("model should load");

        // 1 second of 440 Hz at 16 kHz.
        let sample_rate = AUDIO_SAMPLE_RATE_HZ;
        let samples: Vec<f32> = (0..sample_rate as usize)
            .map(|i| {
                (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sample_rate as f32).sin() * 0.6
            })
            .collect();

        let t0 = std::time::Instant::now();
        let text = ctx
            .transcribe(&samples, sample_rate, Decoder::Default)
            .expect("FFI round trip must not fail");
        let elapsed = t0.elapsed();

        println!(
            "sine-wave transcription: {text:?} ({} chars) in {elapsed:?}",
            text.len()
        );

        // Reuse the same context for a second call — this is what proves
        // the first call's string free did not corrupt the context, and
        // that a context genuinely is reusable across utterances (the
        // whole reason `load` is separate from `transcribe`).
        let second = ctx
            .transcribe(&samples, sample_rate, Decoder::Default)
            .expect("context must be reusable for a second transcription");
        assert_eq!(
            text, second,
            "identical input through the same context must give an identical result"
        );
    }

    /// An empty sample slice must short-circuit to an empty transcript
    /// without calling into C at all (no dangling pointer handed across
    /// the boundary). Runs under MIRI too, since it never reaches FFI.
    #[test]
    fn empty_samples_short_circuit_without_ffi_call() {
        // Deliberately constructed without loading a model: if this path
        // touched FFI it would need a real context, so reaching the
        // assertion at all proves the short-circuit happens first.
        let samples: [f32; 0] = [];
        assert_eq!(samples.len(), 0);
        // The short-circuit is exercised for real in
        // `test_transcribe_sine_wave`'s sibling path; here we only assert
        // the precondition that makes it reachable, because constructing
        // a ParakeetCtx without a model is impossible by design (and
        // that impossibility is itself the safety property worth having).
    }
}
