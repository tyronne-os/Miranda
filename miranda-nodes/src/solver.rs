//! WO-3 T7 — SIMD blend shape solver driven by raw audio energy features.
//!
//! This is the **Pipeline 2 (local)** speech-to-mouth path. It takes raw PCM
//! and produces ARKit mouth weights directly, with no ASR, no phoneme
//! alignment, and no network round trip. It is the counterpart to
//! [`crate::viseme`], which is the **Pipeline 1 (cloud)** path driven by
//! Amazon Polly viseme events. Both feed the same compositor.
//!
//! # What this is, and what it is not
//!
//! It is an **acoustic-to-articulator heuristic**: a small bandpass filter
//! bank measuring energy in eight speech-relevant frequency bands, followed
//! by a fixed linear basis mapping that spectral shape onto mouth channels.
//! The mapping is grounded in formant phonetics — jaw opening tracks F1,
//! lip rounding versus spreading tracks F2, sibilance lives above 3.5 kHz —
//! but the coefficients are hand-authored, not fitted to data.
//!
//! It is **not** a trained audio-to-face regressor. A learned model
//! (Audio2Face-class) will produce better coarticulation and better
//! consonant detail than any hand-authored basis can, because much of what
//! the mouth does is not recoverable from band energies at all: /p/, /b/ and
//! /m/ are acoustically near-identical during closure and differ only in
//! what follows, and tongue position behind closed lips has no acoustic
//! signature whatsoever. `TONGUE_OUT` is therefore never driven here — a
//! plausible-looking guess would be fabrication.
//!
//! What this path does buy, and why it exists: it is deterministic, it runs
//! offline, it adds no latency beyond the analysis window, and its cost is
//! bounded and measurable. That makes it the honest floor of the system —
//! the mouth still moves correctly in amplitude and broad shape when the
//! network is gone.
//!
//! # Why the SIMD axis is *bands*, not samples
//!
//! The filter bank is recursive: each output sample depends on the previous
//! two, so the sample axis cannot be vectorised without restructuring the
//! filters. The band axis is perfectly parallel — eight independent filters
//! see the identical input sample. So the layout is eight bands across two
//! [`f32x4`] vectors, and the inner loop advances all eight filters per
//! sample with a handful of vector operations.
//!
//! Four-wide (`f32x4`, 128-bit SSE) is deliberate, not a default. The
//! development machine (Celeron N4500) reports `sse sse2 sse3 sse4_1 sse4_2`
//! and **no AVX, AVX2, or FMA**. `f32x8` on such a target is emulated as two
//! 128-bit halves by `wide`, so widening the vectors would not widen the
//! hardware — it would only make the band count harder to change. Eight
//! bands over two `f32x4` also happens to be the right *acoustic* resolution
//! for the formant structure being measured, so the two constraints agree.

use miranda_core::{arkit, AUDIO_SAMPLE_RATE_HZ, BLENDSHAPE_COUNT};
use wide::f32x4;

/// Number of analysis bands. Two [`f32x4`] vectors' worth.
pub const BAND_COUNT: usize = 8;

/// Number of SIMD vectors the band state occupies.
const VEC_COUNT: usize = BAND_COUNT / 4;

/// Band edges in Hz, chosen for speech structure at a 16 kHz sample rate
/// (Nyquist 8 kHz).
///
/// | band | range Hz | what it measures |
/// |------|----------|------------------|
/// | 0 | 100–300 | voicing / F0 — vocal fold activity |
/// | 1 | 300–600 | F1 low — close vowels /i/ /u/ |
/// | 2 | 600–1000 | F1 high — open vowels /a/ /ɑ/ |
/// | 3 | 1000–1600 | F2 low — back, rounded vowels /o/ /u/ |
/// | 4 | 1600–2400 | F2 high — front, spread vowels /i/ /e/ |
/// | 5 | 2400–3600 | F3 — /r/ colouring, vowel brightness |
/// | 6 | 3600–5500 | sibilant onset /ʃ/ /tʃ/ |
/// | 7 | 5500–7800 | high fricative /s/ /z/ /f/ |
///
/// The top edge stops at 7800 Hz rather than 8000: a resonator centred at
/// Nyquist is unstable, and the bilinear transform's frequency warping is
/// worst there. Leaving 200 Hz of headroom costs no meaningful sibilant
/// energy.
pub const BAND_EDGES_HZ: [(f32, f32); BAND_COUNT] = [
    (100.0, 300.0),
    (300.0, 600.0),
    (600.0, 1000.0),
    (1000.0, 1600.0),
    (1600.0, 2400.0),
    (2400.0, 3600.0),
    (3600.0, 5500.0),
    (5500.0, 7800.0),
];

// Named band indices. Used by the basis table so the phonetic reasoning is
// readable at the point of use rather than encoded in positional literals.
const B_VOICING: usize = 0;
const B_F1_LOW: usize = 1;
const B_F1_HIGH: usize = 2;
const B_F2_LOW: usize = 3;
const B_F2_HIGH: usize = 4;
const B_F3: usize = 5;
const B_SIB_LOW: usize = 6;
const B_SIB_HIGH: usize = 7;

/// Level below which the input is treated as silence, in dBFS of RMS.
pub const SILENCE_FLOOR_DBFS: f32 = -60.0;

/// Level at which the mouth is considered fully driven, in dBFS of RMS.
/// Above this, extra loudness does not open the mouth further — shouting
/// does not detach the jaw.
pub const FULL_DRIVE_DBFS: f32 = -14.0;

/// Attack smoothing coefficient, per 16.67 ms frame. Mouth shapes are
/// allowed to form quickly.
pub const ATTACK_ALPHA: f32 = 0.55;

/// Release smoothing coefficient, per 16.67 ms frame. Deliberately slower
/// than attack: articulators are pulled into position by muscle and fall
/// back passively, so closing lags opening. Symmetric smoothing is the
/// single most recognisable tell of a cheap lip-sync — the mouth snaps shut
/// between syllables and reads as a chattering puppet.
pub const RELEASE_ALPHA: f32 = 0.25;

// ---------------------------------------------------------------------
// Filter bank
// ---------------------------------------------------------------------

/// Eight constant-skirt bandpass resonators evaluated in SIMD across bands.
///
/// Each band is an RBJ biquad bandpass in transposed direct form II, which
/// needs only two state values per band (rather than direct form I's four)
/// and keeps the per-sample work to five vector multiply/adds.
#[derive(Clone)]
pub struct BandBank {
    /// Numerator gain. For a constant-skirt bandpass `b1 = 0` and
    /// `b2 = -b0`, so one coefficient covers the whole numerator.
    b0: [f32x4; VEC_COUNT],
    a1: [f32x4; VEC_COUNT],
    a2: [f32x4; VEC_COUNT],
    s1: [f32x4; VEC_COUNT],
    s2: [f32x4; VEC_COUNT],
}

impl BandBank {
    /// Builds the bank for [`AUDIO_SAMPLE_RATE_HZ`].
    pub fn new() -> Self {
        Self::with_sample_rate(AUDIO_SAMPLE_RATE_HZ as f32)
    }

    /// Builds the bank for an arbitrary sample rate.
    ///
    /// Coefficients are computed once here, never in the hot loop. The
    /// trigonometry and division below are exactly what must not appear in
    /// the per-sample path.
    pub fn with_sample_rate(sample_rate_hz: f32) -> Self {
        let mut b0 = [0.0f32; BAND_COUNT];
        let mut a1 = [0.0f32; BAND_COUNT];
        let mut a2 = [0.0f32; BAND_COUNT];

        let nyquist = sample_rate_hz * 0.5;

        for (i, &(lo, hi)) in BAND_EDGES_HZ.iter().enumerate() {
            // Geometric centre, not arithmetic: the bilinear-transform
            // bandpass is symmetric on a log frequency axis, so the
            // geometric centre is the frequency actually sitting mid-band.
            let centre = (lo * hi).sqrt();
            // Keep the resonator away from Nyquist, where the biquad
            // degenerates. Bands are authored inside range for 16 kHz; this
            // guard is what keeps `with_sample_rate` honest if someone
            // instantiates the bank at 8 kHz.
            let centre = centre.min(nyquist * 0.95);
            let bandwidth = (hi - lo).min(nyquist * 0.9);
            let q = (centre / bandwidth).max(0.3);

            let w0 = std::f32::consts::TAU * centre / sample_rate_hz;
            let (sin_w0, cos_w0) = w0.sin_cos();
            let alpha = sin_w0 / (2.0 * q);

            // RBJ constant-skirt bandpass, peak gain = Q.
            let norm = 1.0 + alpha;
            b0[i] = alpha / norm;
            a1[i] = (-2.0 * cos_w0) / norm;
            a2[i] = (1.0 - alpha) / norm;
        }

        Self {
            b0: pack(&b0),
            a1: pack(&a1),
            a2: pack(&a2),
            s1: [f32x4::ZERO; VEC_COUNT],
            s2: [f32x4::ZERO; VEC_COUNT],
        }
    }

    /// Clears filter state. Used between utterances so one turn's tail does
    /// not ring into the next.
    pub fn reset(&mut self) {
        self.s1 = [f32x4::ZERO; VEC_COUNT];
        self.s2 = [f32x4::ZERO; VEC_COUNT];
    }

    /// Runs `samples` through the bank and returns per-band RMS.
    ///
    /// Allocation-free: state lives in `self`, the accumulators are on the
    /// stack, and the return value is a fixed array.
    pub fn analyse(&mut self, samples: &[f32]) -> [f32; BAND_COUNT] {
        if samples.is_empty() {
            return [0.0; BAND_COUNT];
        }

        let mut energy = [f32x4::ZERO; VEC_COUNT];

        for &x in samples {
            // One scalar sample broadcast to every band — this is why the
            // band axis is the vectorisable one.
            let xv = f32x4::splat(x);

            for v in 0..VEC_COUNT {
                // Transposed direct form II:
                //   y  = b0*x + s1
                //   s1 = -a1*y + s2          (b1 == 0)
                //   s2 = -b0*x - a2*y        (b2 == -b0)
                let bx = self.b0[v] * xv;
                let y = bx + self.s1[v];
                self.s1[v] = self.s2[v] - self.a1[v] * y;
                self.s2[v] = -bx - self.a2[v] * y;
                energy[v] += y * y;
            }
        }

        let inv_n = 1.0 / samples.len() as f32;
        let mut out = [0.0f32; BAND_COUNT];
        for v in 0..VEC_COUNT {
            let e = (energy[v] * f32x4::splat(inv_n)).sqrt().to_array();
            out[v * 4..v * 4 + 4].copy_from_slice(&e);
        }
        // A NaN or infinity here would propagate into every blend shape and
        // then into the ring buffer, where it becomes a permanently broken
        // face rather than one bad frame. Denormalised or absurd input is
        // cheaper to neutralise than to trace later.
        for e in out.iter_mut() {
            if !e.is_finite() {
                *e = 0.0;
            }
        }
        out
    }
}

impl Default for BandBank {
    fn default() -> Self {
        Self::new()
    }
}

fn pack(values: &[f32; BAND_COUNT]) -> [f32x4; VEC_COUNT] {
    let mut out = [f32x4::ZERO; VEC_COUNT];
    for v in 0..VEC_COUNT {
        out[v] = f32x4::new([
            values[v * 4],
            values[v * 4 + 1],
            values[v * 4 + 2],
            values[v * 4 + 3],
        ]);
    }
    out
}

/// Broadband RMS of a sample block, vectorised over samples.
///
/// Unlike the filter bank this *is* parallel over samples, since squaring
/// and summing carries no state.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let chunks = samples.chunks_exact(4);
    let tail = chunks.remainder();

    let mut acc = f32x4::ZERO;
    for c in chunks {
        let v = f32x4::new([c[0], c[1], c[2], c[3]]);
        acc += v * v;
    }
    let mut sum = acc.reduce_add();
    for &x in tail {
        sum += x * x;
    }
    let value = (sum / samples.len() as f32).sqrt();
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

// ---------------------------------------------------------------------
// Acoustic features
// ---------------------------------------------------------------------

/// Intermediate features extracted from one analysis window. Exposed
/// because the solver's behaviour is only auditable if the features feeding
/// it are inspectable — a wrong weight and a wrong band energy look
/// identical from the outside.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioFeatures {
    /// Per-band RMS, unnormalised.
    pub band_rms: [f32; BAND_COUNT],
    /// Per-band share of total band energy, summing to 1.0 (or all zero for
    /// silence). This is the *shape* of the spectrum with loudness removed,
    /// which is what determines mouth shape.
    pub band_shape: [f32; BAND_COUNT],
    /// Broadband RMS.
    pub rms: f32,
    /// Loudness mapped to `[0, 1]` between [`SILENCE_FLOOR_DBFS`] and
    /// [`FULL_DRIVE_DBFS`]. This is what scales mouth motion.
    pub drive: f32,
}

impl AudioFeatures {
    /// Extracts features from one window. `bank` carries filter state
    /// across calls, so windows must be fed in order.
    pub fn extract(bank: &mut BandBank, samples: &[f32]) -> Self {
        let band_rms = bank.analyse(samples);
        let level = rms(samples);

        let total: f32 = band_rms.iter().sum();
        let mut band_shape = [0.0f32; BAND_COUNT];
        if total > 1e-9 {
            let inv = 1.0 / total;
            for i in 0..BAND_COUNT {
                band_shape[i] = band_rms[i] * inv;
            }
        }

        Self {
            band_rms,
            band_shape,
            rms: level,
            drive: drive_from_rms(level),
        }
    }
}

/// Maps RMS to a `[0, 1]` drive factor on a decibel scale.
///
/// Decibels rather than linear amplitude because loudness perception and
/// articulatory effort are both roughly logarithmic. A linear map spends
/// almost all of its range on the loudest few decibels, so normal speech
/// would sit near zero and the mouth would barely move until someone
/// shouted.
pub fn drive_from_rms(level: f32) -> f32 {
    if !(level > 0.0) || !level.is_finite() {
        return 0.0;
    }
    let dbfs = 20.0 * level.log10();
    ((dbfs - SILENCE_FLOOR_DBFS) / (FULL_DRIVE_DBFS - SILENCE_FLOOR_DBFS)).clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------
// The basis
// ---------------------------------------------------------------------

/// One row of the solver basis: a mouth channel, its output gain, and its
/// sensitivity to each band's share of the spectrum.
///
/// Negative weights are *inhibitory* and are the load-bearing part. Lip
/// rounding is not "energy at 1.2 kHz"; it is energy at 1.2 kHz **in the
/// absence of** energy at 2 kHz. Without the negative term every vowel
/// would round a little and every vowel would smile a little, and the mouth
/// would converge on one mush shape for all speech.
struct BasisRow {
    channel: usize,
    gain: f32,
    weights: [f32; BAND_COUNT],
}

/// Builds a weight array with named bands, so a row reads as phonetics
/// rather than as eight positional numbers.
const fn w(pairs: &[(usize, f32)]) -> [f32; BAND_COUNT] {
    let mut out = [0.0; BAND_COUNT];
    let mut i = 0;
    while i < pairs.len() {
        out[pairs[i].0] = pairs[i].1;
        i += 1;
    }
    out
}

/// The solver basis.
///
/// Every channel is referenced through [`arkit`] constants. No bare index
/// literals: a wrong index here moves the wrong muscle and would pass every
/// range check in this file.
fn basis() -> [BasisRow; 11] {
    [
        // Jaw opening tracks F1. High F1 (600–1000 Hz) means an open vocal
        // tract; energy concentrated at low F1 means a close vowel with the
        // jaw nearly shut. F2 contributes nothing to aperture.
        BasisRow {
            channel: arkit::JAW_OPEN,
            gain: 1.6,
            weights: w(&[(B_F1_HIGH, 1.0), (B_F1_LOW, 0.25), (B_SIB_HIGH, -0.5)]),
        },
        // Bilabial closure /p/ /b/ /m/: voicing present, nothing above it.
        // The negative terms are the whole test — during closure the lips
        // block the high frequencies, so their absence *is* the signal.
        BasisRow {
            channel: arkit::MOUTH_CLOSE,
            gain: 1.3,
            weights: w(&[
                (B_VOICING, 1.0),
                (B_F1_LOW, 0.2),
                (B_F2_HIGH, -0.7),
                (B_SIB_LOW, -0.7),
                (B_SIB_HIGH, -0.7),
            ]),
        },
        // Rounding /o/ /u/ /w/: F2 pulled down. Inhibited by high F2, which
        // is the spread-lip signature and the acoustic opposite.
        BasisRow {
            channel: arkit::MOUTH_FUNNEL,
            gain: 1.7,
            weights: w(&[(B_F2_LOW, 1.0), (B_F2_HIGH, -0.8), (B_SIB_HIGH, -0.4)]),
        },
        BasisRow {
            channel: arkit::MOUTH_PUCKER,
            gain: 1.5,
            weights: w(&[
                (B_F2_LOW, 0.9),
                (B_F1_LOW, 0.3),
                (B_F2_HIGH, -0.7),
                (B_F1_HIGH, -0.4),
            ]),
        },
        // Spreading /i/ /e/: F2 high, and high F3 brightness with it.
        BasisRow {
            channel: arkit::MOUTH_SMILE_LEFT,
            gain: 1.4,
            weights: w(&[(B_F2_HIGH, 1.0), (B_F3, 0.3), (B_F2_LOW, -0.6)]),
        },
        BasisRow {
            channel: arkit::MOUTH_SMILE_RIGHT,
            gain: 1.4,
            weights: w(&[(B_F2_HIGH, 1.0), (B_F3, 0.3), (B_F2_LOW, -0.6)]),
        },
        BasisRow {
            channel: arkit::MOUTH_STRETCH_LEFT,
            gain: 1.1,
            weights: w(&[(B_F2_HIGH, 0.8), (B_F3, 0.5), (B_F1_HIGH, -0.3)]),
        },
        BasisRow {
            channel: arkit::MOUTH_STRETCH_RIGHT,
            gain: 1.1,
            weights: w(&[(B_F2_HIGH, 0.8), (B_F3, 0.5), (B_F1_HIGH, -0.3)]),
        },
        // Labiodental /f/ /v/: high-frequency friction with little voicing.
        BasisRow {
            channel: arkit::MOUTH_SHRUG_LOWER,
            gain: 1.5,
            weights: w(&[(B_SIB_LOW, 0.8), (B_SIB_HIGH, 0.5), (B_VOICING, -0.8)]),
        },
        BasisRow {
            channel: arkit::MOUTH_SHRUG_UPPER,
            gain: 1.2,
            weights: w(&[(B_SIB_HIGH, 0.7), (B_VOICING, -0.6)]),
        },
        // Alveolar sibilants /s/ /z/ draw the lips slightly back over the
        // teeth; the roll is small and driven only by the top band.
        BasisRow {
            channel: arkit::MOUTH_ROLL_LOWER,
            gain: 0.7,
            weights: w(&[(B_SIB_HIGH, 0.8), (B_VOICING, -0.5)]),
        },
    ]
}

// ---------------------------------------------------------------------
// The solver
// ---------------------------------------------------------------------

/// Raw-audio-driven mouth blend shape solver.
///
/// Feed it one window of PCM per rendered frame; it returns the full
/// 52-channel weight array with only its own mouth channels non-zero. Like
/// the oscillators, it never touches the autonomic channels — the
/// compositor owns combining the two layers.
pub struct AudioBlendshapeSolver {
    bank: BandBank,
    basis: [BasisRow; 11],
    /// Smoothed output, held across frames.
    current: [f32; BLENDSHAPE_COUNT],
    /// Untouched scratch for the pre-smoothing solve. Owned so the hot path
    /// allocates nothing.
    target: [f32; BLENDSHAPE_COUNT],
    attack_alpha: f32,
    release_alpha: f32,
    last_features: AudioFeatures,
}

impl AudioBlendshapeSolver {
    pub fn new() -> Self {
        Self::with_sample_rate(AUDIO_SAMPLE_RATE_HZ as f32)
    }

    pub fn with_sample_rate(sample_rate_hz: f32) -> Self {
        Self {
            bank: BandBank::with_sample_rate(sample_rate_hz),
            basis: basis(),
            current: [0.0; BLENDSHAPE_COUNT],
            target: [0.0; BLENDSHAPE_COUNT],
            // Both coefficients are clamped into (0, 1]. Zero would freeze
            // the mouth forever and values above 1 would overshoot and
            // oscillate — both are silent visual failures, so they are
            // rejected here rather than trusted.
            attack_alpha: ATTACK_ALPHA.clamp(f32::EPSILON, 1.0),
            release_alpha: RELEASE_ALPHA.clamp(f32::EPSILON, 1.0),
            last_features: AudioFeatures {
                band_rms: [0.0; BAND_COUNT],
                band_shape: [0.0; BAND_COUNT],
                rms: 0.0,
                drive: 0.0,
            },
        }
    }

    /// Features from the most recent [`Self::solve`]. For telemetry and for
    /// the tests that need to distinguish a filter-bank error from a basis
    /// error.
    pub fn features(&self) -> &AudioFeatures {
        &self.last_features
    }

    /// Clears filter and smoothing state.
    pub fn reset(&mut self) {
        self.bank.reset();
        self.current = [0.0; BLENDSHAPE_COUNT];
        self.last_features = AudioFeatures {
            band_rms: [0.0; BAND_COUNT],
            band_shape: [0.0; BAND_COUNT],
            rms: 0.0,
            drive: 0.0,
        };
    }

    /// Solves one frame from `samples` and returns the smoothed weights.
    ///
    /// Allocation-free, branch-light, and free of I/O and locking, so it is
    /// safe to call from the 60 FPS thread.
    pub fn solve(&mut self, samples: &[f32]) -> &[f32; BLENDSHAPE_COUNT] {
        let features = AudioFeatures::extract(&mut self.bank, samples);
        self.last_features = features;

        self.target = [0.0; BLENDSHAPE_COUNT];

        // Silence is an explicit case, not an emergent one. With drive == 0
        // every row would multiply out to zero anyway, but relying on that
        // means the *shape* is still computed from filter ringing during
        // silence, and any later change to the drive term would let that
        // ringing leak onto the face.
        if features.drive > 0.0 {
            for row in self.basis.iter() {
                let mut acc = 0.0f32;
                for b in 0..BAND_COUNT {
                    acc += row.weights[b] * features.band_shape[b];
                }
                // Rectify before scaling: a negative activation means "this
                // articulator is not indicated", which is zero, not a
                // negative weight.
                let activation = (acc * row.gain).max(0.0);
                self.target[row.channel] = (activation * features.drive).clamp(0.0, 1.0);
            }
        }

        // Asymmetric smoothing, applied per channel on its own direction of
        // travel.
        for i in 0..BLENDSHAPE_COUNT {
            let t = self.target[i];
            let c = self.current[i];
            let alpha = if t > c {
                self.attack_alpha
            } else {
                self.release_alpha
            };
            self.current[i] = c + (t - c) * alpha;
        }

        &self.current
    }

    /// Current smoothed weights without advancing the solver.
    pub fn weights(&self) -> &[f32; BLENDSHAPE_COUNT] {
        &self.current
    }

    /// Adds the solved weights into `out`, additively, matching the
    /// oscillators' contract so the compositor can layer all sources
    /// uniformly and own the final clamp.
    pub fn add_into(&self, out: &mut [f32; BLENDSHAPE_COUNT]) {
        for i in 0..BLENDSHAPE_COUNT {
            out[i] += self.current[i];
        }
    }
}

impl Default for AudioBlendshapeSolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const SR: f32 = AUDIO_SAMPLE_RATE_HZ as f32;
    /// Samples per rendered frame at 60 FPS and 16 kHz: 16.67 ms.
    const FRAME_SAMPLES: usize = 267;

    fn sine(freq: f32, amp: f32, n: usize, phase_offset: f32) -> Vec<f32> {
        (0..n)
            .map(|i| {
                (std::f32::consts::TAU * freq * i as f32 / SR + phase_offset).sin() * amp
            })
            .collect()
    }

    /// Two summed sinusoids at F1 and F2. Not a real vocal tract — a real
    /// one has a harmonic source and a full formant envelope — but the
    /// solver reads band energy, and a formant pair puts energy in the same
    /// bands a real one would.
    fn vowel(f1: f32, f2: f32, amp: f32, n: usize) -> Vec<f32> {
        let a = sine(f1, amp * 0.6, n, 0.0);
        let b = sine(f2, amp * 0.4, n, 1.0);
        a.iter().zip(b.iter()).map(|(x, y)| x + y).collect()
    }

    /// Deterministic pseudo-noise band-limited by summing many sines across
    /// a range. Used for fricatives.
    fn noise_band(lo: f32, hi: f32, amp: f32, n: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; n];
        let lines = 24;
        for k in 0..lines {
            let f = lo + (hi - lo) * (k as f32 / (lines - 1) as f32);
            let phase = k as f32 * 1.7;
            for (i, o) in out.iter_mut().enumerate() {
                *o += (std::f32::consts::TAU * f * i as f32 / SR + phase).sin();
            }
        }
        let scale = amp / (lines as f32).sqrt();
        for o in out.iter_mut() {
            *o *= scale;
        }
        out
    }

    /// Runs enough frames for the smoother to settle, then returns the
    /// weights. A single frame only shows `attack_alpha` of the target, so
    /// asserting on one frame would test the smoother, not the basis.
    fn settle(solver: &mut AudioBlendshapeSolver, block: &[f32], frames: usize) -> [f32; BLENDSHAPE_COUNT] {
        let mut last = [0.0; BLENDSHAPE_COUNT];
        for _ in 0..frames {
            last = *solver.solve(block);
        }
        last
    }

    // -----------------------------------------------------------------
    // Filter bank correctness — the foundation everything else rests on
    // -----------------------------------------------------------------

    /// **The load-bearing test of this module.** Each band must respond
    /// most strongly to its own centre frequency.
    ///
    /// If the biquad coefficients are wrong, or the geometric centre is
    /// miscomputed, or the bands are packed into the SIMD lanes in the
    /// wrong order, every downstream weight is driven by the wrong part of
    /// the spectrum. Nothing about that is visible from range checks: the
    /// weights stay in `[0, 1]`, they move with the audio, and the mouth is
    /// simply wrong. Testing tone-to-band correspondence directly is the
    /// only way to catch it.
    #[test]
    fn each_band_responds_most_to_its_own_centre_frequency() {
        for (band, &(lo, hi)) in BAND_EDGES_HZ.iter().enumerate() {
            let centre = (lo * hi).sqrt();
            let mut bank = BandBank::new();
            // Long enough for the resonators to reach steady state.
            let tone = sine(centre, 0.5, 4096, 0.0);
            let energies = bank.analyse(&tone);

            let winner = energies
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                .map(|(i, _)| i)
                .unwrap();

            assert_eq!(
                winner, band,
                "a {centre:.0} Hz tone (centre of band {band}, {lo}-{hi} Hz) \
                 produced peak energy in band {winner} instead. Band \
                 energies: {energies:?}"
            );
        }
    }

    /// Bands must also *reject* distant frequencies, not merely prefer
    /// their own. A bank where every band passes everything would still
    /// win the test above while carrying no spectral information at all.
    #[test]
    fn bands_reject_distant_frequencies() {
        let mut bank = BandBank::new();
        let tone = sine(200.0, 0.5, 4096, 0.0);
        let e = bank.analyse(&tone);
        assert!(
            e[B_SIB_HIGH] < e[B_VOICING] * 0.05,
            "a 200 Hz tone leaked {} into the 5.5-7.8 kHz band against {} \
             in its own band — the bank is not selective",
            e[B_SIB_HIGH],
            e[B_VOICING]
        );

        let mut bank = BandBank::new();
        let tone = sine(6500.0, 0.5, 4096, 0.0);
        let e = bank.analyse(&tone);
        assert!(
            e[B_VOICING] < e[B_SIB_HIGH] * 0.05,
            "a 6.5 kHz tone leaked {} into the 100-300 Hz band",
            e[B_VOICING]
        );
    }

    /// Pathological input must not destabilise the recursive filters. A
    /// biquad that blows up produces infinities that, once written to the
    /// ring buffer, break the face permanently rather than for one frame.
    #[test]
    fn bank_is_stable_under_pathological_input() {
        let cases: Vec<(&str, Vec<f32>)> = vec![
            ("dc", vec![1.0; 4096]),
            (
                "nyquist square",
                (0..4096).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }).collect(),
            ),
            ("full scale step", {
                let mut v = vec![0.0; 4096];
                v[2048..].fill(1.0);
                v
            }),
            ("loud", vec![100.0; 4096]),
        ];

        for (name, input) in cases {
            let mut bank = BandBank::new();
            let e = bank.analyse(&input);
            for (i, v) in e.iter().enumerate() {
                assert!(
                    v.is_finite(),
                    "{name}: band {i} energy is not finite ({v}) — filter diverged"
                );
            }
        }
    }

    #[test]
    fn empty_input_is_handled_without_panicking() {
        let mut bank = BandBank::new();
        assert_eq!(bank.analyse(&[]), [0.0; BAND_COUNT]);
        assert_eq!(rms(&[]), 0.0);
    }

    /// A block whose length is not a multiple of four must be fully
    /// accounted for. `chunks_exact` silently drops the remainder, so a
    /// missing tail loop is easy to write and hard to notice — the error is
    /// a small level offset, not a crash.
    #[test]
    fn rms_includes_the_non_multiple_of_four_tail() {
        // Nine samples: two full vectors and a one-sample remainder.
        let mut v = vec![0.0f32; 9];
        v[8] = 3.0;
        let expected = (9.0f32 / 9.0).sqrt();
        assert!(
            (rms(&v) - expected).abs() < 1e-6,
            "tail sample was dropped: got {}, expected {expected}",
            rms(&v)
        );
    }

    #[test]
    fn rms_matches_the_scalar_definition() {
        let block = vowel(730.0, 1090.0, 0.4, 267);
        let scalar =
            (block.iter().map(|x| x * x).sum::<f32>() / block.len() as f32).sqrt();
        assert!(
            (rms(&block) - scalar).abs() < 1e-5,
            "SIMD rms {} disagrees with scalar {}",
            rms(&block),
            scalar
        );
    }

    // -----------------------------------------------------------------
    // Drive curve
    // -----------------------------------------------------------------

    #[test]
    fn silence_produces_zero_drive_and_loud_speech_produces_full_drive() {
        assert_eq!(drive_from_rms(0.0), 0.0);
        // -80 dBFS, below the floor.
        assert_eq!(drive_from_rms(0.0001), 0.0);
        // 0 dBFS, above full drive.
        assert_eq!(drive_from_rms(1.0), 1.0);
        // Conversational speech around -30 dBFS should land mid-range, not
        // pinned at either end. A linear (non-dB) map would put this at
        // about 0.03 and the mouth would barely move.
        let d = drive_from_rms(10f32.powf(-30.0 / 20.0));
        assert!(
            (0.3..0.8).contains(&d),
            "-30 dBFS mapped to drive {d}, outside a usable mid-range"
        );
    }

    #[test]
    fn drive_is_monotonic_in_level() {
        let mut prev = -1.0;
        for i in 0..=100 {
            let dbfs = -70.0 + i as f32 * 0.7;
            let d = drive_from_rms(10f32.powf(dbfs / 20.0));
            assert!(d >= prev, "drive fell from {prev} to {d} at {dbfs} dBFS");
            prev = d;
        }
    }

    // -----------------------------------------------------------------
    // Solver behaviour
    // -----------------------------------------------------------------

    #[test]
    fn silence_produces_a_rest_pose() {
        let mut solver = AudioBlendshapeSolver::new();
        let out = settle(&mut solver, &vec![0.0; FRAME_SAMPLES], 60);
        for (i, w) in out.iter().enumerate() {
            assert_eq!(*w, 0.0, "channel {i} = {w} during silence");
        }
    }

    /// Layer ownership, enforced. Speech owns the mouth; the autonomic
    /// oscillators own eyes, brows, cheeks and nose. If the solver ever
    /// wrote an eye channel it would fight the blink and gaze generators
    /// through the compositor's additive sum, and the result would look
    /// like a rendering glitch rather than a layering bug.
    #[test]
    fn solver_never_touches_autonomic_channels() {
        let forbidden = [
            arkit::EYE_BLINK_LEFT,
            arkit::EYE_BLINK_RIGHT,
            arkit::EYE_SQUINT_LEFT,
            arkit::EYE_SQUINT_RIGHT,
            arkit::EYE_LOOK_IN_LEFT,
            arkit::EYE_LOOK_OUT_LEFT,
            arkit::EYE_LOOK_UP_LEFT,
            arkit::EYE_LOOK_DOWN_LEFT,
            arkit::EYE_LOOK_IN_RIGHT,
            arkit::EYE_LOOK_OUT_RIGHT,
            arkit::EYE_LOOK_UP_RIGHT,
            arkit::EYE_LOOK_DOWN_RIGHT,
            arkit::EYE_WIDE_LEFT,
            arkit::EYE_WIDE_RIGHT,
            arkit::BROW_DOWN_LEFT,
            arkit::BROW_DOWN_RIGHT,
            arkit::BROW_INNER_UP,
            arkit::BROW_OUTER_UP_LEFT,
            arkit::BROW_OUTER_UP_RIGHT,
            arkit::CHEEK_PUFF,
            arkit::CHEEK_SQUINT_LEFT,
            arkit::CHEEK_SQUINT_RIGHT,
            arkit::NOSE_SNEER_LEFT,
            arkit::NOSE_SNEER_RIGHT,
            // Never inferred from audio: nothing about band energy
            // distinguishes tongue position behind closed lips.
            arkit::TONGUE_OUT,
        ];

        let inputs: Vec<Vec<f32>> = vec![
            vowel(730.0, 1090.0, 0.5, FRAME_SAMPLES),
            vowel(270.0, 2290.0, 0.5, FRAME_SAMPLES),
            noise_band(5000.0, 7500.0, 0.5, FRAME_SAMPLES),
            noise_band(80.0, 8000.0, 0.5, FRAME_SAMPLES),
            vec![0.0; FRAME_SAMPLES],
        ];

        let mut solver = AudioBlendshapeSolver::new();
        for input in &inputs {
            for _ in 0..30 {
                let out = solver.solve(input);
                for &c in forbidden.iter() {
                    assert_eq!(
                        out[c], 0.0,
                        "solver wrote {} to autonomic channel {c}",
                        out[c]
                    );
                }
            }
        }
    }

    #[test]
    fn weights_stay_in_unit_range_for_every_input() {
        let inputs: Vec<Vec<f32>> = vec![
            vowel(730.0, 1090.0, 1.0, FRAME_SAMPLES),
            vowel(270.0, 2290.0, 1.0, FRAME_SAMPLES),
            vowel(300.0, 870.0, 1.0, FRAME_SAMPLES),
            noise_band(3600.0, 7800.0, 1.0, FRAME_SAMPLES),
            noise_band(80.0, 8000.0, 1.0, FRAME_SAMPLES),
            // Deliberately over unity — a clipped capture is a real thing
            // that happens, and it must not produce out-of-range weights.
            vec![50.0; FRAME_SAMPLES],
        ];

        let mut solver = AudioBlendshapeSolver::new();
        for (n, input) in inputs.iter().enumerate() {
            for _ in 0..40 {
                let out = solver.solve(input);
                for (i, w) in out.iter().enumerate() {
                    assert!(
                        (0.0..=1.0).contains(w),
                        "input {n}: channel {i} = {w} out of range"
                    );
                }
            }
        }
    }

    /// Loud input must not drive every channel to 1.0. A solver that
    /// saturates is producing an expressionless rictus, which passes every
    /// range check while carrying zero information about the speech.
    #[test]
    fn loud_input_does_not_saturate_every_channel() {
        let mut solver = AudioBlendshapeSolver::new();
        let out = settle(
            &mut solver,
            &noise_band(80.0, 8000.0, 0.9, FRAME_SAMPLES),
            60,
        );
        let saturated = out.iter().filter(|w| **w > 0.99).count();
        assert!(
            saturated <= 2,
            "{saturated} channels saturated at 1.0 on loud broadband input — \
             the basis is not discriminating"
        );
    }

    /// **Open versus close vowel.** /a/ (F1 730) must open the jaw more
    /// than /i/ (F1 270). This is the single most visible property of
    /// audio-driven lip sync, and it is what a band-order or F1/F2 mix-up
    /// would break.
    #[test]
    fn open_vowel_opens_the_jaw_more_than_a_close_vowel() {
        let mut s = AudioBlendshapeSolver::new();
        let ah = settle(&mut s, &vowel(730.0, 1090.0, 0.5, FRAME_SAMPLES), 60)
            [arkit::JAW_OPEN];

        let mut s = AudioBlendshapeSolver::new();
        let ee = settle(&mut s, &vowel(270.0, 2290.0, 0.5, FRAME_SAMPLES), 60)
            [arkit::JAW_OPEN];

        println!("jawOpen: /a/ = {ah:.4}, /i/ = {ee:.4}");
        assert!(
            ah > ee * 1.5,
            "/a/ (F1 730) gave jawOpen {ah} but /i/ (F1 270) gave {ee} — the \
             jaw is not tracking F1"
        );
        assert!(ah > 0.15, "/a/ barely opened the jaw ({ah}) — visually dead");
    }

    /// **Rounded versus spread.** /u/ must round the lips and /i/ must
    /// spread them, and each must suppress the other. Driving both on every
    /// vowel is the failure the inhibitory basis weights exist to prevent.
    #[test]
    fn rounded_and_spread_vowels_produce_opposite_lip_shapes() {
        let mut s = AudioBlendshapeSolver::new();
        // /u/ — F1 300, F2 870: both formants low.
        let oo = settle(&mut s, &vowel(300.0, 870.0, 0.5, FRAME_SAMPLES), 60);

        let mut s = AudioBlendshapeSolver::new();
        // /i/ — F1 270, F2 2290: F2 far up.
        let ee = settle(&mut s, &vowel(270.0, 2290.0, 0.5, FRAME_SAMPLES), 60);

        println!(
            "/u/: funnel {:.4} pucker {:.4} smile {:.4}",
            oo[arkit::MOUTH_FUNNEL],
            oo[arkit::MOUTH_PUCKER],
            oo[arkit::MOUTH_SMILE_LEFT]
        );
        println!(
            "/i/: funnel {:.4} pucker {:.4} smile {:.4}",
            ee[arkit::MOUTH_FUNNEL],
            ee[arkit::MOUTH_PUCKER],
            ee[arkit::MOUTH_SMILE_LEFT]
        );

        assert!(
            oo[arkit::MOUTH_PUCKER] > oo[arkit::MOUTH_SMILE_LEFT],
            "/u/ smiled ({}) more than it puckered ({})",
            oo[arkit::MOUTH_SMILE_LEFT],
            oo[arkit::MOUTH_PUCKER]
        );
        assert!(
            ee[arkit::MOUTH_SMILE_LEFT] > ee[arkit::MOUTH_PUCKER],
            "/i/ puckered ({}) more than it smiled ({})",
            ee[arkit::MOUTH_PUCKER],
            ee[arkit::MOUTH_SMILE_LEFT]
        );
        assert!(
            ee[arkit::MOUTH_SMILE_LEFT] > 0.1,
            "/i/ barely spread the lips ({}) — visually dead",
            ee[arkit::MOUTH_SMILE_LEFT]
        );
    }

    /// Unvoiced friction must engage the lip/teeth channels rather than
    /// dropping the jaw. A solver keyed on loudness alone gapes at every
    /// /s/.
    #[test]
    fn sibilance_engages_lips_rather_than_opening_the_jaw() {
        let mut s = AudioBlendshapeSolver::new();
        let ess = settle(
            &mut s,
            &noise_band(5000.0, 7500.0, 0.5, FRAME_SAMPLES),
            60,
        );
        println!(
            "/s/: jawOpen {:.4} shrugLower {:.4} rollLower {:.4}",
            ess[arkit::JAW_OPEN],
            ess[arkit::MOUTH_SHRUG_LOWER],
            ess[arkit::MOUTH_ROLL_LOWER]
        );
        assert!(
            ess[arkit::MOUTH_SHRUG_LOWER] > ess[arkit::JAW_OPEN],
            "/s/ opened the jaw ({}) more than it engaged the lips ({})",
            ess[arkit::JAW_OPEN],
            ess[arkit::MOUTH_SHRUG_LOWER]
        );
    }

    /// Voiced low-frequency energy with nothing above it is bilabial
    /// closure, and must close rather than open the mouth.
    #[test]
    fn voiced_low_energy_reads_as_closure_not_aperture() {
        let mut s = AudioBlendshapeSolver::new();
        let mm = settle(&mut s, &sine(150.0, 0.5, FRAME_SAMPLES, 0.0), 60);
        println!(
            "/m/: mouthClose {:.4} jawOpen {:.4}",
            mm[arkit::MOUTH_CLOSE],
            mm[arkit::JAW_OPEN]
        );
        assert!(
            mm[arkit::MOUTH_CLOSE] > mm[arkit::JAW_OPEN],
            "150 Hz voicing gave mouthClose {} vs jawOpen {} — closure is not \
             being detected",
            mm[arkit::MOUTH_CLOSE],
            mm[arkit::JAW_OPEN]
        );
    }

    // -----------------------------------------------------------------
    // Temporal behaviour
    // -----------------------------------------------------------------

    /// Attack must be faster than release. Symmetric smoothing makes the
    /// mouth snap shut between syllables — the classic cheap-lip-sync
    /// chatter.
    #[test]
    fn attack_is_faster_than_release() {
        let loud = vowel(730.0, 1090.0, 0.5, FRAME_SAMPLES);
        let quiet = vec![0.0f32; FRAME_SAMPLES];

        let mut s = AudioBlendshapeSolver::new();
        // Reach steady state, note it, then measure how many frames it took
        // to get most of the way there from rest.
        let settled = settle(&mut s, &loud, 80)[arkit::JAW_OPEN];
        assert!(settled > 0.1, "no signal to measure ({settled})");

        let mut s = AudioBlendshapeSolver::new();
        let mut rise_frames = 0;
        for f in 1..=200 {
            if s.solve(&loud)[arkit::JAW_OPEN] >= settled * 0.9 {
                rise_frames = f;
                break;
            }
        }

        let mut fall_frames = 0;
        for f in 1..=200 {
            if s.solve(&quiet)[arkit::JAW_OPEN] <= settled * 0.1 {
                fall_frames = f;
                break;
            }
        }

        println!("jawOpen rise {rise_frames} frames, fall {fall_frames} frames");
        assert!(rise_frames > 0 && fall_frames > 0, "never settled either way");
        assert!(
            fall_frames > rise_frames,
            "release ({fall_frames} frames) is not slower than attack \
             ({rise_frames} frames)"
        );
    }

    /// A steady input must reach a steady output. A solver that never
    /// settles has replaced silence with jitter.
    #[test]
    fn steady_input_converges_to_a_steady_output() {
        let mut s = AudioBlendshapeSolver::new();
        let block = vowel(730.0, 1090.0, 0.5, FRAME_SAMPLES);
        let settled = settle(&mut s, &block, 120);
        for _ in 0..30 {
            let now = *s.solve(&block);
            for i in 0..BLENDSHAPE_COUNT {
                assert!(
                    (now[i] - settled[i]).abs() < 1e-3,
                    "channel {i} drifted from {} to {}",
                    settled[i],
                    now[i]
                );
            }
        }
    }

    /// Reproducibility is required by T9's capture workflow: the same audio
    /// must yield the same weights.
    #[test]
    fn solving_is_deterministic_after_reset() {
        let block = vowel(500.0, 1500.0, 0.4, FRAME_SAMPLES);
        let mut s = AudioBlendshapeSolver::new();
        let first = settle(&mut s, &block, 50);
        s.reset();
        let second = settle(&mut s, &block, 50);
        assert_eq!(first, second, "solver output is not reproducible after reset");
    }

    #[test]
    fn reset_clears_filter_ringing_between_utterances() {
        let mut s = AudioBlendshapeSolver::new();
        settle(&mut s, &vowel(730.0, 1090.0, 0.8, FRAME_SAMPLES), 40);
        s.reset();
        for (i, w) in s.weights().iter().enumerate() {
            assert_eq!(*w, 0.0, "channel {i} survived reset with {w}");
        }
        assert_eq!(s.features().drive, 0.0);
    }

    #[test]
    fn add_into_is_additive_not_assigning() {
        let mut s = AudioBlendshapeSolver::new();
        settle(&mut s, &vowel(730.0, 1090.0, 0.5, FRAME_SAMPLES), 60);

        let mut out = [0.0f32; BLENDSHAPE_COUNT];
        out[arkit::JAW_OPEN] = 0.04; // as the breath oscillator would leave it
        let before = out[arkit::JAW_OPEN];
        s.add_into(&mut out);
        assert!(
            out[arkit::JAW_OPEN] > before,
            "add_into clobbered an existing contribution ({before} -> {})",
            out[arkit::JAW_OPEN]
        );
        assert!(
            (out[arkit::JAW_OPEN] - (before + s.weights()[arkit::JAW_OPEN])).abs() < 1e-6
        );
    }

    #[test]
    fn solver_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<AudioBlendshapeSolver>();
        assert_send::<BandBank>();
    }

    // -----------------------------------------------------------------
    // Performance budget
    // -----------------------------------------------------------------

    /// **Acceptance criterion: solve time under 0.2 ms.**
    ///
    /// Measured on a real block of one frame's worth of audio (267 samples
    /// at 16 kHz), averaged over many iterations to get above timer
    /// resolution.
    ///
    /// The assertion is gated on `debug_assertions` because an unoptimised
    /// build is 10-40x slower and the number would say nothing about the
    /// shipped code. In a debug build the measurement is printed and the
    /// budget is not enforced; in release it is enforced. Run
    /// `cargo test --release -p miranda-nodes solve_time` for the number
    /// that counts.
    #[test]
    fn solve_time_is_within_the_frame_budget() {
        const BUDGET_US: f64 = 200.0;
        const ITERS: usize = 20_000;

        let block = vowel(730.0, 1090.0, 0.5, FRAME_SAMPLES);
        let mut s = AudioBlendshapeSolver::new();

        // Warm up so the measurement is not dominated by first-touch page
        // faults and cold caches.
        for _ in 0..2_000 {
            s.solve(&block);
        }

        let start = Instant::now();
        for _ in 0..ITERS {
            let out = s.solve(&block);
            // Consume the result so the loop cannot be optimised away.
            std::hint::black_box(out[arkit::JAW_OPEN]);
        }
        let per_solve_us = start.elapsed().as_secs_f64() * 1e6 / ITERS as f64;

        println!(
            "solve: {per_solve_us:.3} us/frame for {FRAME_SAMPLES} samples \
             ({BAND_COUNT} bands, f32x4) — budget {BUDGET_US} us, \
             {}% of a 16.67 ms frame",
            (per_solve_us / 16_670.0 * 100.0).round()
        );

        if cfg!(debug_assertions) {
            println!(
                "NOTE: debug build — budget NOT enforced. Run with --release \
                 for the number that counts."
            );
        } else {
            assert!(
                per_solve_us < BUDGET_US,
                "solve took {per_solve_us:.3} us, over the {BUDGET_US} us budget"
            );
        }
    }
}
