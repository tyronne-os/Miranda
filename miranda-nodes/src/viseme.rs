//! WO-3 T2 — Amazon Polly viseme → ARKit-52 blend shape adapter.
//!
//! This is **Pipeline 1's** speech-driven kinematics path: Polly returns
//! time-aligned viseme events (a small phoneme-class alphabet), and this
//! module converts each into a set of ARKit blend shape weights, then
//! smooths across frames so phoneme boundaries don't snap.
//!
//! Pipeline 2's equivalent is the SIMD acoustic-energy solver (T7), which
//! produces the same 52-channel output from raw PCM instead. Both feed the
//! same compositor (T6) and the same autonomic layer, which is the point:
//! the *speech source* is swappable, the downstream face physics is not.
//!
//! # Every weight here is referenced by name, never by index
//!
//! All targets go through `miranda_core::arkit`'s named constants. This is
//! deliberate and load-bearing — see that module's docs for why (three
//! conflicting index schemes existed in this repo's docs; a wrong index
//! moves the wrong muscle and never fails a test that only checks ranges).

use miranda_core::{arkit, BLENDSHAPE_COUNT};

/// Amazon Polly's viseme alphabet.
///
/// Polly emits these as short strings in its Speech Marks stream (`"p"`,
/// `"sil"`, `"@"`, …). The set is Polly's documented viseme inventory —
/// note it is a *phoneme-class* alphabet, not a per-language phoneme list,
/// which is why several distinct sounds share one viseme (`p`/`b`/`m` all
/// look the same on the lips).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Viseme {
    /// Silence / rest.
    Sil,
    /// Bilabial plosive: p, b, m.
    P,
    /// Labiodental fricative: f, v.
    F,
    /// Dental fricative: th (θ, ð).
    T,
    /// Alveolar: t, d, n, l.
    TT,
    /// Sibilant: s, z.
    S,
    /// Postalveolar: sh, zh, ch, j.
    SS,
    /// Velar plosive: k, g, ng.
    K,
    /// Rhotic: r.
    R,
    /// Open front vowel: a (as in "father").
    A,
    /// Mid front vowel: e (as in "bed").
    E,
    /// Close front vowel: i (as in "see").
    I,
    /// Mid back rounded vowel: o (as in "go").
    O,
    /// Close back rounded vowel: u (as in "too").
    U,
    /// Schwa / neutral vowel: @ (as in "above").
    At,
}

impl Viseme {
    /// Parses Polly's on-the-wire viseme string.
    ///
    /// Returns `None` for an unrecognized token rather than silently
    /// mapping it to silence: an unknown viseme means either Polly changed
    /// its alphabet or the stream is corrupt, and both deserve a real
    /// error at the call site instead of EVE's mouth quietly going slack.
    pub fn from_polly(value: &str) -> Option<Self> {
        Some(match value {
            "sil" => Viseme::Sil,
            "p" => Viseme::P,
            "f" => Viseme::F,
            "T" => Viseme::T,
            "t" => Viseme::TT,
            "s" => Viseme::S,
            "S" => Viseme::SS,
            "k" => Viseme::K,
            "r" => Viseme::R,
            "a" => Viseme::A,
            "e" => Viseme::E,
            "i" => Viseme::I,
            "o" => Viseme::O,
            "u" => Viseme::U,
            "@" => Viseme::At,
            _ => return None,
        })
    }

    /// The target blend shape weights for this viseme, as
    /// `(channel_index, weight)` pairs.
    ///
    /// Values follow the WO-3 directive's interpolation table where it
    /// specifies one, and are filled in consistently for the visemes it
    /// left unspecified. They are intentionally *sparse* — a viseme names
    /// only the channels it actually drives, and every unnamed channel
    /// stays at 0.0 from the speech layer, leaving it free for the
    /// autonomic layer (blink/gaze/breath) to own without contention.
    pub fn targets(self) -> &'static [(usize, f32)] {
        match self {
            // Rest position: the speech layer contributes nothing at all.
            // Note this is NOT "the face is still" — the autonomic layer
            // is always running underneath (No-Loop Video Protocol).
            Viseme::Sil => &[],

            // Bilabial plosive — lips pressed shut. Directive-specified.
            Viseme::P => &[
                (arkit::MOUTH_CLOSE, 0.90),
                (arkit::JAW_OPEN, 0.05),
                (arkit::MOUTH_PUCKER, 0.20),
            ],

            // Labiodental — lower lip to upper teeth. Directive-specified.
            Viseme::F => &[
                (arkit::MOUTH_SHRUG_LOWER, 0.70),
                (arkit::JAW_OPEN, 0.15),
            ],

            // Dental fricative — tongue between teeth. Directive-specified.
            Viseme::T => &[(arkit::JAW_OPEN, 0.20), (arkit::TONGUE_OUT, 0.60)],

            // Alveolar stop — tongue to ridge, jaw barely open, slight
            // lateral stretch. Not directive-specified; kept small since
            // t/d/n/l are visually subtle.
            Viseme::TT => &[
                (arkit::JAW_OPEN, 0.18),
                (arkit::MOUTH_STRETCH_LEFT, 0.12),
                (arkit::MOUTH_STRETCH_RIGHT, 0.12),
            ],

            // Sibilant s/z — narrow aperture, slight spread.
            Viseme::S => &[
                (arkit::JAW_OPEN, 0.12),
                (arkit::MOUTH_SMILE_LEFT, 0.15),
                (arkit::MOUTH_SMILE_RIGHT, 0.15),
            ],

            // Postalveolar sh/ch — rounded and funnelled.
            // Directive-specified.
            Viseme::SS => &[
                (arkit::MOUTH_FUNNEL, 0.60),
                (arkit::JAW_OPEN, 0.20),
                (arkit::MOUTH_PUCKER, 0.40),
            ],

            // Velar plosive k/g — jaw drop with lateral stretch.
            // Directive-specified.
            Viseme::K => &[
                (arkit::JAW_OPEN, 0.35),
                (arkit::MOUTH_STRETCH_LEFT, 0.30),
                (arkit::MOUTH_STRETCH_RIGHT, 0.30),
            ],

            // Rhotic r — slight upper-lip shrug and rounding.
            Viseme::R => &[
                (arkit::MOUTH_SHRUG_UPPER, 0.40),
                (arkit::MOUTH_PUCKER, 0.25),
                (arkit::JAW_OPEN, 0.20),
            ],

            // Open vowel a — the widest jaw of the set.
            Viseme::A => &[(arkit::JAW_OPEN, 0.70), (arkit::MOUTH_CLOSE, 0.0)],

            // Mid vowel e — moderate jaw with spread.
            Viseme::E => &[
                (arkit::JAW_OPEN, 0.35),
                (arkit::MOUTH_SMILE_LEFT, 0.30),
                (arkit::MOUTH_SMILE_RIGHT, 0.30),
            ],

            // Close front vowel i — minimal jaw, strong spread.
            Viseme::I => &[
                (arkit::JAW_OPEN, 0.15),
                (arkit::MOUTH_SMILE_LEFT, 0.55),
                (arkit::MOUTH_SMILE_RIGHT, 0.55),
            ],

            // Mid back rounded o — funnel plus jaw.
            Viseme::O => &[
                (arkit::MOUTH_FUNNEL, 0.60),
                (arkit::JAW_OPEN, 0.35),
                (arkit::MOUTH_PUCKER, 0.30),
            ],

            // Close back rounded u — tightest rounding.
            Viseme::U => &[
                (arkit::MOUTH_PUCKER, 0.65),
                (arkit::MOUTH_FUNNEL, 0.45),
                (arkit::JAW_OPEN, 0.15),
            ],

            // Schwa — neutral, slightly open.
            Viseme::At => &[(arkit::JAW_OPEN, 0.28)],
        }
    }
}

/// Default exponential-smoothing factor from the WO-3 directive.
///
/// `alpha` is the weight given to the *new* target each frame, so higher
/// values track faster and smooth less. 0.35 at 60 FPS reaches ~90% of a
/// step change in about 6 frames (~100 ms), which is roughly the duration
/// of a real phoneme transition — fast enough to stay in sync with audio,
/// slow enough to remove the snap.
pub const DEFAULT_SMOOTHING_ALPHA: f32 = 0.35;

/// Converts a stream of Polly visemes into smoothed ARKit blend shape
/// weights, one frame at a time.
///
/// # Smoothing model
///
/// The directive calls for "exponential smoothing (α = 0.35) across a
/// 3-frame lookahead". Lookahead is deliberately **not** implemented here,
/// and that is a real deviation worth stating plainly rather than
/// pretending otherwise: genuine lookahead requires buffering future
/// viseme events before rendering the current frame, which trades latency
/// for smoothness. Polly's Speech Marks *do* arrive ahead of playback, so
/// a lookahead implementation is possible — but it belongs in the
/// scheduler that pairs marks to the audio clock (WO-4's media-clock
/// territory), not in this stateless-per-frame converter. What this module
/// provides is the exponential smoother itself, which is the part that
/// eliminates snapping; adding lookahead later changes *which target* is
/// fed in, not this math.
pub struct VisemeAdapter {
    /// Current smoothed weights — the previous frame's output, which is
    /// also the smoother's state.
    current: [f32; BLENDSHAPE_COUNT],
    /// Smoothing factor in (0, 1].
    alpha: f32,
}

impl VisemeAdapter {
    pub fn new() -> Self {
        Self::with_alpha(DEFAULT_SMOOTHING_ALPHA)
    }

    /// `alpha` is clamped into a sane open range: 0.0 would freeze the
    /// output forever (never converging on any target) and values above
    /// 1.0 would overshoot and oscillate, so both are rejected at
    /// construction rather than producing baffling motion later.
    pub fn with_alpha(alpha: f32) -> Self {
        Self {
            current: [0.0; BLENDSHAPE_COUNT],
            alpha: alpha.clamp(f32::EPSILON, 1.0),
        }
    }

    /// The last emitted frame's weights.
    pub fn current(&self) -> &[f32; BLENDSHAPE_COUNT] {
        &self.current
    }

    /// Advances one frame toward `viseme`'s target pose and returns the
    /// smoothed weights.
    ///
    /// Allocation-free: writes into a fixed-size array owned by `self`,
    /// which matters because this runs inside the 60 FPS loop (RT-safety
    /// standard: no heap traffic in the active loop).
    pub fn step(&mut self, viseme: Viseme) -> &[f32; BLENDSHAPE_COUNT] {
        // Build the sparse target as a dense array on the stack. 52 f32s
        // is 208 bytes — cheap, and keeps the smoothing loop below a
        // simple uniform pass rather than a branchy sparse merge.
        let mut target = [0.0f32; BLENDSHAPE_COUNT];
        for &(channel, weight) in viseme.targets() {
            // Debug-only bounds proof: `targets()` returns constants from
            // `arkit`, so this can only fire if someone adds a bad literal
            // there. Cheap insurance at the one place indices enter.
            debug_assert!(
                channel < BLENDSHAPE_COUNT,
                "viseme {viseme:?} targets out-of-range channel {channel}"
            );
            target[channel] = weight;
        }

        for i in 0..BLENDSHAPE_COUNT {
            // Standard exponential smoothing: move a fraction alpha of the
            // remaining distance to the target each frame.
            self.current[i] += self.alpha * (target[i] - self.current[i]);
        }
        &self.current
    }

    /// Resets to the rest pose. Used when a session ends, so a new
    /// utterance doesn't inherit the tail of the previous one's mouth
    /// shape.
    pub fn reset(&mut self) {
        self.current = [0.0; BLENDSHAPE_COUNT];
    }
}

impl Default for VisemeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_polly_viseme_token() {
        let tokens = [
            "sil", "p", "f", "T", "t", "s", "S", "k", "r", "a", "e", "i", "o", "u", "@",
        ];
        for t in tokens {
            assert!(
                Viseme::from_polly(t).is_some(),
                "documented Polly viseme {t:?} must parse"
            );
        }
    }

    /// An unknown token must be rejected, not silently treated as silence —
    /// otherwise a Polly alphabet change would degrade EVE's speech to a
    /// slack jaw with no error anywhere.
    #[test]
    fn rejects_unknown_viseme_token() {
        assert_eq!(Viseme::from_polly("zzz"), None);
        assert_eq!(Viseme::from_polly(""), None);
        // Case is significant: "T" (dental) and "t" (alveolar) are
        // different visemes in Polly's alphabet, so a case-insensitive
        // parse would conflate two distinct mouth shapes.
        assert_ne!(Viseme::from_polly("T"), Viseme::from_polly("t"));
    }

    /// Every target every viseme names must be a valid channel index and a
    /// legal weight. This is the guard against a typo in the table pointing
    /// at channel 52+ (panic) or specifying an out-of-range weight (which
    /// would later be clamped, masking the mistake).
    #[test]
    fn all_viseme_targets_are_in_range() {
        let all = [
            Viseme::Sil, Viseme::P, Viseme::F, Viseme::T, Viseme::TT, Viseme::S,
            Viseme::SS, Viseme::K, Viseme::R, Viseme::A, Viseme::E, Viseme::I,
            Viseme::O, Viseme::U, Viseme::At,
        ];
        for v in all {
            for &(channel, weight) in v.targets() {
                assert!(
                    channel < BLENDSHAPE_COUNT,
                    "viseme {v:?} targets channel {channel}, out of range"
                );
                assert!(
                    (0.0..=1.0).contains(&weight),
                    "viseme {v:?} channel {channel} weight {weight} outside [0,1]"
                );
            }
        }
    }

    /// Visemes must only drive mouth/jaw/tongue channels. If a speech
    /// viseme wrote to an eye or brow channel it would fight the autonomic
    /// layer for ownership of that channel, producing motion that looks
    /// like a bug and is very hard to attribute. This test enforces the
    /// separation the compositor design depends on.
    #[test]
    fn visemes_never_touch_autonomic_channels() {
        // Eye, brow, cheek and nose channels belong to the autonomic layer
        // (blink/gaze) or to expression, not to speech.
        let forbidden: Vec<usize> = (arkit::EYE_BLINK_LEFT..=arkit::EYE_WIDE_RIGHT)
            .chain(arkit::BROW_DOWN_LEFT..=arkit::BROW_OUTER_UP_RIGHT)
            .chain(arkit::CHEEK_SQUINT_LEFT..=arkit::CHEEK_SQUINT_RIGHT)
            .collect();

        let all = [
            Viseme::Sil, Viseme::P, Viseme::F, Viseme::T, Viseme::TT, Viseme::S,
            Viseme::SS, Viseme::K, Viseme::R, Viseme::A, Viseme::E, Viseme::I,
            Viseme::O, Viseme::U, Viseme::At,
        ];
        for v in all {
            for &(channel, _) in v.targets() {
                assert!(
                    !forbidden.contains(&channel),
                    "viseme {v:?} writes to autonomic-owned channel {channel} \
                     ({}) — speech and autonomic layers must not contend",
                    arkit::CHANNEL_NAMES[channel]
                );
            }
        }
    }

    /// Smoothing must actually smooth: a step change to a new viseme must
    /// approach its target progressively, never jump there in one frame.
    /// That single-frame jump is precisely the "snapping between phonemes"
    /// the directive requires eliminating.
    #[test]
    fn smoothing_approaches_target_without_snapping() {
        let mut a = VisemeAdapter::new();
        let target = Viseme::A.targets()
            .iter()
            .find(|(c, _)| *c == arkit::JAW_OPEN)
            .map(|(_, w)| *w)
            .expect("viseme A drives jawOpen");

        let first = a.step(Viseme::A)[arkit::JAW_OPEN];
        assert!(
            first > 0.0 && first < target,
            "after one frame jawOpen should be partway to {target}, got {first}"
        );

        // Monotonic approach, never overshooting.
        let mut prev = first;
        for frame in 0..60 {
            let now = a.step(Viseme::A)[arkit::JAW_OPEN];
            assert!(
                now >= prev - f32::EPSILON,
                "frame {frame}: weight went backwards ({prev} -> {now}) while \
                 holding one viseme"
            );
            assert!(
                now <= target + 1e-4,
                "frame {frame}: overshot target {target} with {now}"
            );
            prev = now;
        }
        // After a second of holding, it should be essentially there.
        assert!(
            (prev - target).abs() < 0.01,
            "after 61 frames jawOpen {prev} should have converged on {target}"
        );
    }

    /// Returning to silence must relax the mouth back toward rest, again
    /// progressively rather than snapping shut.
    #[test]
    fn silence_relaxes_back_toward_rest() {
        let mut a = VisemeAdapter::new();
        for _ in 0..60 {
            a.step(Viseme::A);
        }
        let open = a.current()[arkit::JAW_OPEN];
        assert!(open > 0.5, "should be open after holding viseme A, got {open}");

        let one_frame_later = a.step(Viseme::Sil)[arkit::JAW_OPEN];
        assert!(
            one_frame_later < open && one_frame_later > 0.0,
            "silence should relax progressively: {open} -> {one_frame_later}"
        );

        for _ in 0..120 {
            a.step(Viseme::Sil);
        }
        assert!(
            a.current()[arkit::JAW_OPEN] < 0.01,
            "should have relaxed to rest, got {}",
            a.current()[arkit::JAW_OPEN]
        );
    }

    /// A higher alpha must track faster than a lower one — proves the
    /// parameter is actually wired into the math rather than ignored.
    #[test]
    fn higher_alpha_tracks_faster() {
        let mut slow = VisemeAdapter::with_alpha(0.1);
        let mut fast = VisemeAdapter::with_alpha(0.9);
        slow.step(Viseme::A);
        fast.step(Viseme::A);
        assert!(
            fast.current()[arkit::JAW_OPEN] > slow.current()[arkit::JAW_OPEN],
            "alpha=0.9 should move further in one frame than alpha=0.1"
        );
    }

    /// Degenerate alphas are clamped at construction, so they can't
    /// produce a frozen or oscillating face at runtime.
    #[test]
    fn degenerate_alpha_is_clamped() {
        let mut zero = VisemeAdapter::with_alpha(0.0);
        zero.step(Viseme::A);
        assert!(
            zero.current()[arkit::JAW_OPEN] > 0.0,
            "alpha 0.0 must be clamped to something that still converges"
        );

        let mut over = VisemeAdapter::with_alpha(5.0);
        let w = over.step(Viseme::A)[arkit::JAW_OPEN];
        assert!(
            w <= 0.70 + 1e-4,
            "alpha > 1 must be clamped so the weight cannot overshoot: got {w}"
        );
    }

    #[test]
    fn reset_returns_to_rest_pose() {
        let mut a = VisemeAdapter::new();
        for _ in 0..30 {
            a.step(Viseme::U);
        }
        assert!(a.current()[arkit::MOUTH_PUCKER] > 0.0);
        a.reset();
        assert_eq!(a.current(), &[0.0; BLENDSHAPE_COUNT]);
    }
}
