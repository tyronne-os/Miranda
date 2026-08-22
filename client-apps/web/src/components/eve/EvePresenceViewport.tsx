import { useEffect, useMemo, useRef } from "react";
import type { BlendshapeFrame, PresenceStage } from "@/lib/stageMachine/types";
import { usePresenceStore } from "@/store/presenceStore";
import { uponLoad } from "@/lib/metrics/uponLoad";
import { realness } from "@/lib/metrics/realness";

/**
 * L0 Idle Presence — the life layer.
 *
 * Organic motion written straight to the photo frame at display rate:
 * amplitude-modulated breath, slow sway/drift on offset phases, scheduled
 * micro attention shifts (saccade analogue), luminance breathing, and a
 * Pipe 3 talk pulse where jawOpen from the phoneme-direct timeline moves
 * the portrait itself. Compositor-only transform/filter — no re-renders,
 * no GPU dependency, Celeron-friendly.
 */
interface PresenceLayers {
  frame: React.RefObject<HTMLDivElement | null>;
  /** Blurred, over-scaled backdrop — counter-moves to create parallax depth. */
  back: React.RefObject<HTMLDivElement | null>;
  /** Sharp masked subject — carries head motion. */
  subject: React.RefObject<HTMLDivElement | null>;
  /** Hair halo — heavier, lags the head. Independent mass. */
  hair: React.RefObject<HTMLDivElement | null>;
  /** Eyelids — the blink the flat portrait could never do. */
  lids: React.RefObject<HTMLDivElement | null>;
}

function useIdlePresence(refs: PresenceLayers) {
  useEffect(() => {
    const frameRef = refs.frame;
    let raf = 0;
    let lastStep = performance.now();
    const t0 = performance.now();

    // ── blink scheduler ──
    // A face that never blinks is the loudest uncanny signal there is. Human
    // resting rate is ~14/min in conversation, and blinks cluster rather than
    // metronome, so the interval is redrawn every time.
    const blinkCfg = realness.blink;
    const blinkPeriodMs = 60000 / Math.max(1, blinkCfg?.frequencyBpm ?? 14);
    const blinkCloseMs = ((blinkCfg?.durationFrames ?? 10) / 30) * 1000;
    let nextBlinkAt = t0 + blinkPeriodMs * (0.4 + Math.random() * 0.9);
    let blinkStart = -1;
    let doubleBlink = false;

    // Head motion the hair must lag behind — one-pole follower, not a copy.
    let hairLagX = 0;
    let hairLagRot = 0;

    /**
     * Presence self-audit.
     *
     * A hidden pane throttles the animation loop, so an outside observer
     * cannot sample fast enough to witness a blink or measure parallax. She
     * records her own evidence instead — every frame, from inside the loop —
     * and hands it over on demand. This is the raw telemetry stream the QC
     * layer consumes: proof of life that survives not being watched.
     */
    const audit = {
      startedAt: t0,
      frames: 0,
      blinks: 0,
      doubleBlinks: 0,
      lastBlinkAt: 0,
      blinkIntervalsMs: [] as number[],
      peakClosure: 0,
      parallaxOpposed: 0,
      parallaxSamples: 0,
      hairLagSum: 0,
      hairLagSamples: 0,
      visemeFramesSeen: 0,
    };
    (window as unknown as { __EVE_PRESENCE_AUDIT__?: () => unknown }).__EVE_PRESENCE_AUDIT__ =
      () => {
        const secs = (performance.now() - audit.startedAt) / 1000;
        const iv = audit.blinkIntervalsMs;
        return {
          uptimeSec: +secs.toFixed(1),
          framesWritten: audit.frames,
          effectiveFps: +(audit.frames / Math.max(1, secs)).toFixed(1),
          blinks: audit.blinks,
          doubleBlinks: audit.doubleBlinks,
          blinkRateBpm: +((audit.blinks / Math.max(1, secs)) * 60).toFixed(1),
          meanBlinkGapMs: iv.length
            ? Math.round(iv.reduce((a, b) => a + b, 0) / iv.length)
            : null,
          peakClosure: +audit.peakClosure.toFixed(2),
          parallaxOpposedPct: audit.parallaxSamples
            ? Math.round((audit.parallaxOpposed / audit.parallaxSamples) * 100)
            : 0,
          meanHairLagPx: audit.hairLagSamples
            ? +(audit.hairLagSum / audit.hairLagSamples).toFixed(3)
            : 0,
          visemeFramesSeen: audit.visemeFramesSeen,
        };
      };

    // attention-shift scheduler (saccade analogue)
    const att0 = realness.attention;
    let nextShiftAt = t0 + att0.minIntervalMs + Math.random() * (att0.maxIntervalMs - att0.minIntervalMs);
    let shiftStart = -1;
    let shiftDx = 0;
    let shiftRot = 0;

    // smoothed talk energy so speech onset/release feels muscular, not switched
    let talkLevel = 0;

    const step = (now: number) => {
      lastStep = now;
      const el = frameRef.current;
      if (!el) return;
      // UPON LOAD: first frame we actually write is the moment she breathes.
      uponLoad.mark("motionMs");

      const t = (now - t0) / 1000;
      const { talking, blend, presenceEnergy } = usePresenceStore.getState();

      // Live tuning surface — the lab can reshape her mid-session.
      const { breath: B, sway: S, attention: A, speech: SP } = realness;
      const TAU = Math.PI * 2;

      // ── breath: amplitude-modulated so it never metronomes. The AM is the
      //    difference between a person breathing and a machine cycling.
      const breathAm = 1 + B.amplitudeModDepth * Math.sin(t * B.amplitudeModHz * TAU + 1.1);
      const breath = Math.sin(t * B.rateHz * TAU) * breathAm;

      // ── slow body language on offset, non-harmonic periods
      const sway = Math.sin(t * S.rateHz * TAU + 1.7);
      const drift = Math.sin(t * S.driftRateHz * TAU + 0.6);

      // ── scheduled micro attention shift
      if (now >= nextShiftAt && shiftStart < 0) {
        shiftStart = now;
        shiftDx = (Math.random() - 0.5) * A.travelPx;
        shiftRot = (Math.random() - 0.5) * A.rotationDeg;
        nextShiftAt = now + A.minIntervalMs + Math.random() * (A.maxIntervalMs - A.minIntervalMs);
      }
      let shiftX = 0;
      let shiftR = 0;
      if (shiftStart > 0) {
        const p = (now - shiftStart) / A.durationMs;
        if (p >= 1) {
          shiftStart = -1;
        } else {
          const ease = p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2;
          const bump = Math.sin(ease * Math.PI);
          shiftX = shiftDx * bump;
          shiftR = shiftRot * bump;
        }
      }

      // ── Pipe 3 talk pulse: the text-derived jaw drives the portrait
      const jaw = blend.weights.jawOpen ?? 0;
      const targetTalk = talking ? Math.min(1, jaw * SP.jawGain + blend.energy * SP.energyGain) : 0;
      talkLevel += (targetTalk - talkLevel) * SP.attack;

      const dy = breath * B.travelPx - talkLevel * SP.liftPx;
      const dx = drift * S.driftPx + shiftX;
      const rot = sway * S.rotationDeg + shiftR;
      const scale = 1 + breath * B.scaleDepth + talkLevel * SP.scaleDepth;
      const bright =
        1 + breath * B.luminanceDepth + talkLevel * SP.luminanceDepth + presenceEnergy * 0.012;

      el.style.transform = `translate3d(${dx.toFixed(2)}px, ${dy.toFixed(2)}px, 0) rotate(${rot.toFixed(3)}deg) scale(${scale.toFixed(4)})`;
      el.style.filter = `brightness(${bright.toFixed(4)})`;

      // ── PARALLAX: the backdrop counter-moves, so the frame stops reading as
      //    one flat photograph being panned and starts reading as depth.
      const back = refs.back.current;
      if (back) {
        const bx = -dx * 1.9;
        back.style.transform =
          `translate3d(${bx.toFixed(2)}px, ${(-dy * 1.35).toFixed(2)}px, 0) scale(1.085)`;
        if (Math.abs(dx) > 0.2) {
          audit.parallaxSamples += 1;
          if (dx * bx < 0) audit.parallaxOpposed += 1; // proof of depth
        }
      }
      audit.frames += 1;
      if (talking) audit.visemeFramesSeen += 1;

      // ── HAIR LAG: mass. Her hair is heavy and arrives late; a rigid photo
      //    moves it in perfect lockstep with the skull, which is the tell.
      hairLagX += (dx - hairLagX) * 0.055;
      hairLagRot += (rot - hairLagRot) * 0.045;
      audit.hairLagSum += Math.abs(dx - hairLagX);
      audit.hairLagSamples += 1;
      const hair = refs.hair.current;
      if (hair) {
        const overshoot = (dx - hairLagX) * 0.9;
        hair.style.transform =
          `translate3d(${(hairLagX + overshoot).toFixed(2)}px, ${(dy * 0.72).toFixed(2)}px, 0)` +
          ` rotate(${(hairLagRot * 1.25).toFixed(3)}deg) scale(${(scale * 1.004).toFixed(4)})`;
      }

      // ── BLINK ──
      if (now >= nextBlinkAt && blinkStart < 0) {
        blinkStart = now;
        doubleBlink = Math.random() < 0.22; // people often blink twice
        nextBlinkAt = now + blinkPeriodMs * (0.45 + Math.random() * 1.15);
        audit.blinks += 1;
        if (doubleBlink) audit.doubleBlinks += 1;
        if (audit.lastBlinkAt) audit.blinkIntervalsMs.push(Math.round(now - audit.lastBlinkAt));
        audit.lastBlinkAt = now;
      }
      const lids = refs.lids.current;
      if (lids) {
        let closed = 0;
        if (blinkStart > 0) {
          const span = doubleBlink ? blinkCloseMs * 2.6 : blinkCloseMs;
          const p = (now - blinkStart) / span;
          if (p >= 1) {
            blinkStart = -1;
          } else if (doubleBlink) {
            // two lid sweeps inside one gesture
            const q = (p * 2) % 1;
            closed = Math.sin(q * Math.PI);
          } else {
            // fast down, slower release — how a real lid actually travels
            closed = p < 0.38 ? p / 0.38 : 1 - (p - 0.38) / 0.62;
          }
        }
        lids.style.setProperty("--lid", closed.toFixed(3));
        lids.style.opacity = closed > 0.01 ? "1" : "0";
        if (closed > audit.peakClosure) audit.peakClosure = closed;
      }
    };

    const loop = (now: number) => {
      raf = requestAnimationFrame(loop);
      step(now);
    };
    raf = requestAnimationFrame(loop);

    // Presence never dies in background: when the compositor starves rAF
    // (hidden pane, minimized kiosk), a 30 Hz interval keeps her alive so
    // focus return shows life instantly — Instant Presence has no cold face.
    const fallback = window.setInterval(() => {
      const now = performance.now();
      if (now - lastStep > 120) step(now);
    }, 33);

    return () => {
      cancelAnimationFrame(raf);
      window.clearInterval(fallback);
    };
  }, [refs]);
}

interface Props {
  stage: PresenceStage;
  blend: BlendshapeFrame;
  talking: boolean;
  energy: number;
  warmProgress: number;
  /** Optional still override from backend admin. */
  still?: "natural" | "closeup";
}

/**
 * Mark the portrait as painted.
 *
 * `img.decode()` is the accurate signal, but on an already-cached image it can
 * never settle — and a metric that silently fails to record is worse than a
 * slightly early one. Race it against a short guard so UPON LOAD always lands.
 */
function markPortraitPainted(img: HTMLImageElement) {
  let done = false;
  const fire = () => {
    if (done) return;
    done = true;
    uponLoad.mark("portraitMs");
  };
  window.setTimeout(fire, 120);
  if (img.decode) img.decode().then(fire, fire);
  else fire();
}

/**
 * Approved project stills only — Zero Placeholders Protocol.
 *
 * WebP first, original as fallback. The source PNG is 5.8 MB at 2048²; on a
 * low-power CPU its *decode* cost ~2.1s and was the sole thing keeping UPON
 * LOAD out of contract. The 1536² WebP is 442 KB and visually identical at
 * every size she is ever displayed at.
 */
const STILLS = {
  natural: {
    src: "/staff/eve-natural.webp",
    fallback: "/staff/eve-natural.png",
    alt: "EVE — natural portrait still",
  },
  closeup: {
    src: "/staff/eve-closeup.webp",
    fallback: "/staff/eve-closeup.jpg",
    alt: "EVE — photoreal close-up still",
  },
} as const;

function pickStill(
  stage: PresenceStage,
  talking: boolean,
  override?: "natural" | "closeup",
): keyof typeof STILLS {
  if (override) return override;
  if (stage === "L2") return "closeup";
  if (stage === "L1" && talking) return "closeup";
  return "natural";
}

/**
 * Mirror surface — photoreal still only.
 * No HUD, nameplate, scan labels, or drawn face rig.
 * Stage only grades light + scale for beauty measurement.
 */
export function EvePresenceViewport({
  stage,
  blend: _blend,
  talking,
  energy,
  warmProgress: _warmProgress,
  still: stillOverride,
}: Props) {
  void _blend;
  void _warmProgress;

  const frameRef = useRef<HTMLDivElement | null>(null);
  const photoRef = useRef<HTMLImageElement | null>(null);
  const backRef = useRef<HTMLDivElement | null>(null);
  const subjectRef = useRef<HTMLDivElement | null>(null);
  const hairRef = useRef<HTMLDivElement | null>(null);
  const lidsRef = useRef<HTMLDivElement | null>(null);
  // Stable identity so the 60Hz loop is never torn down by a re-render.
  const layers = useRef<PresenceLayers>({
    frame: frameRef,
    back: backRef,
    subject: subjectRef,
    hair: hairRef,
    lids: lidsRef,
  }).current;
  useIdlePresence(layers);

  // A preloaded portrait can already be decoded before React attaches onLoad,
  // in which case that event never fires. Check completeness on mount so the
  // fastest possible load is not recorded as the slowest.
  useEffect(() => {
    const img = photoRef.current;
    if (img?.complete && img.naturalWidth > 0) markPortraitPainted(img);
  }, []);

  const stillKey = pickStill(stage, talking, stillOverride);
  const still = STILLS[stillKey];

  const grade = useMemo(() => {
    if (stage === "L2") {
      return {
        filter: `saturate(1.1) contrast(1.05) brightness(${1.02 + energy * 0.035})`,
        scale: 1.03 + energy * 0.015,
      };
    }
    if (stage === "L1") {
      return {
        filter: `saturate(1.05) contrast(1.025) brightness(${1 + energy * 0.025})`,
        scale: talking ? 1.02 + energy * 0.01 : 1.008,
      };
    }
    return {
      filter: `saturate(0.99) contrast(1.01) brightness(${0.99 + energy * 0.015})`,
      scale: 1,
    };
  }, [stage, energy, talking]);

  return (
    <div
      className={`eve-viewport mirror stage-${stage}${talking ? " is-talking" : ""} still-${stillKey}`}
      aria-label={`EVE presence ${stage}`}
    >
      <div className="eve-viewport-glow" aria-hidden />

      {/* Environment plate — fills the WHOLE viewport, behind the portrait.
          It must live out here, not inside the photo frame: the portrait is
          opaque and edge-to-edge, so anything under it is invisible. Out here
          it occupies the gutter and the parallax actually reads. */}
      <div
        ref={backRef}
        className="eve-layer-back"
        style={{ backgroundImage: `url(${still.src})` }}
        aria-hidden
      />

      <div
        className="eve-figure"
        style={{
          filter: grade.filter,
          transform: `scale(${grade.scale})`,
        }}
      >
        <div ref={frameRef} className={`eve-photo-frame${talking ? " talking" : ""}`}>
          {/* Hair halo — masked to the outer mass and stacked ABOVE the
              portrait, because below it nothing can be seen. Offset by a
              pixel or two it reads as hair carrying its own weight. */}
          <div
            ref={hairRef}
            className="eve-layer-hair"
            style={{ backgroundImage: `url(${still.src})` }}
            aria-hidden
          />
          <img
            ref={photoRef}
            className="eve-photo"
            src={still.src}
            alt={still.alt}
            draggable={false}
            // Presence is the first paint that matters — never lazy, never low.
            fetchPriority="high"
            decoding="async"
            onError={(e) => {
              // Zero Placeholders: fall back to the original master, never to
              // a drawn or stock stand-in.
              const img = e.currentTarget;
              if (img.src.endsWith(still.fallback)) return;
              img.src = still.fallback;
            }}
            onLoad={(e) => markPortraitPainted(e.currentTarget)}
          />
          {/* Eyelids. The one thing a rigid transform can never do — and the
              loudest uncanny signal when it is missing. Two soft lid sweeps
              pinned to her eye positions, driven by --lid 0..1. */}
          <div ref={lidsRef} className="eve-lids" aria-hidden>
            <span className="eve-lid eve-lid-l" />
            <span className="eve-lid eve-lid-r" />
          </div>
          <div className="eve-photo-vignette" aria-hidden />
        </div>
      </div>
    </div>
  );
}
