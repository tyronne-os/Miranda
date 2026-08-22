# Instant Presence Standard (IPS)

The canonical behavioral and latency spec for how EVE (and any avatar built on the NOBILITY POSH FRAMEWORK) comes alive. This document supersedes the abbreviated IPS table that previously lived only in `README.md` — that table is preserved below as the latency/staging rules; the **No Loop Video Protocol** section is new, added to close a real gap: the prior spec never wrote down the actual *behavioral* requirement, only the timing budget.

## The No Loop Video Protocol

**The anti-pattern this explicitly rejects**: a pre-rendered idle-loop video clip playing on repeat while the "real" system warms up behind it. That's the cheap, common approach across most avatar products — and it's disqualifying under this standard, because a loop is legible as a loop within a few seconds (it repeats), which breaks presence the moment a viewer notices the pattern. **No loop, ever, at any stage** — every frame EVE renders must come from live generation, not playback of a fixed clip.

**Instead — instantly alive, from frame one**:

- **First frame, not first few seconds**: the moment the avatar is visible, it must already read as alive — not "loading, about to become alive." No fade-in-then-animate; the very first rendered frame carries motion signal.
- **Engaging and waving as the opening gesture**: the default entry behavior is a wave — a real, physically choreographed greeting motion, not a static pose. This is the canonical "hello" moment and should be the first thing a first-time viewer sees.
- **Mid-frame, waist-up composition**: the camera/crop framing is fixed at waist-up, centered mid-frame — not a full-body shot, not a tight face-only close-up. This framing is deliberate: it's tight enough to read facial micro-expression clearly, wide enough to carry shoulder/arm choreography (the wave, hand gestures) without cropping them out.
- **Choreography, not idle animation**: motion during any "waiting" state is not a canned idle-loop but genuine choreographed movement — weight shifts, head tilts, hand micro-gestures — sourced the same way conversational motion is (Vanguard Innovation #21, VAD-triggered micro-expressions; #25, Ambient Perlin Noise Generation for eyelid/lip movement during silence). "Idle" and "alive" are the same rendering path, not two different modes with a seam between them.
- **Micromovement is mandatory, not decorative**: per Innovation #22 (Uncanny Valley Gradient Penalties), perfectly still or perfectly smooth motion reads as dead — there must always be some infinitesimal, non-repeating movement (breathing, blink, weight shift) even at total rest. Zero motion for more than one frame interval is a defect, not a stable/idle state.

**Verification standard, not aspiration**: per the honesty discipline this whole project runs on (see the `ORCHESTRATION-PIVOT.md` lesson — "a high score on the wrong metric is worse than no score at all"), a build only satisfies the No Loop Video Protocol when it's been observed rendering distinct, non-repeating frames over a real time window — not when the code merely intends to. The prior ECC-5 certification's MOTION pillar failure (scored 96/100 on transform values that were animating behind an opaque, unchanging portrait) is the exact failure mode to guard against here: verify what's actually *painted*, not just what the code claims to be doing.

## The original IPS latency/staging rules (carried forward, unchanged)

| Rule | Meaning |
|------|---------|
| **Control plane < 1s** | Chat, graph, idle presence, stage bus always answer immediately |
| **Data plane warms cold** | Riva / Nemotron / A2F-3D / AnimGraph / Omniverse never block L0 |
| **L0 → L1 → L2** | Idle presence → ARKit Audio2Face → Omniverse cinematic takeover |
| **ARKit, not mesh soup** | A2F emits 52-channel blendshapes |
| **OV is optional glory** | Pixel stream is L2 only — never a boot blocker |
| **Spatial Syncer** | Shared media clock + stage bus couples cortex ↔ studio |

**How the No Loop Video Protocol fits the L0→L1→L2 staging**: the protocol applies starting at L0 — even before A2F-3D (L1) or the Omniverse pixel stream (L2) are warm, whatever is on-screen at L0 must already satisfy "instantly alive, waist-up, waving, choreographed" using whatever rendering is available at that stage (e.g. the CSS/transform-driven presence layer). The mistake documented in `ORCHESTRATION-PIVOT.md` — cosmetics mistaken for science, a CSS layer scored as if it were the real neural renderer — must not repeat here: L0's presence layer can satisfy the *behavioral* half of this standard (instantly alive, no loop, waving, choreographed) without yet satisfying photorealism, and the certification process must label which half it's actually measuring.

## Cross-references

- Miranda-Engine's Work Order 3 (`miranda-nodes` crate) is where the real kinematics for this protocol eventually live — ARKit-52 blendshape SIMD, Perlin-noise micro-saccades, breathing oscillators — see the `nobility-posh-framework` Kiro skill's `references/work-orders.md`.
- Vanguard Innovations #21, #22, #25 (`references/vanguard-innovations.md`) are the named techniques this protocol depends on.
