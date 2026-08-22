/**
 * ECC-5 Realness Profile — client mirror of the tunable human-feel surface.
 *
 * Defaults here match persona/realness.profile.json so the front is never
 * blocked on the controller. When the controller answers, its values win —
 * the lab bench is the source of truth for how human she feels.
 */

export interface RealnessProfile {
  name: string;
  breath: {
    rateHz: number;
    amplitudeModHz: number;
    amplitudeModDepth: number;
    travelPx: number;
    scaleDepth: number;
    luminanceDepth: number;
  };
  sway: { rateHz: number; rotationDeg: number; driftRateHz: number; driftPx: number };
  /** Eyelid gesture. Mirrors NVIDIA Maxine A2F-2D blink_frequency/duration. */
  blink: { implemented: boolean; frequencyBpm: number; durationFrames: number };
  attention: {
    minIntervalMs: number;
    maxIntervalMs: number;
    durationMs: number;
    travelPx: number;
    rotationDeg: number;
  };
  speech: {
    jawGain: number;
    energyGain: number;
    attack: number;
    liftPx: number;
    scaleDepth: number;
    luminanceDepth: number;
    /** Crossfade between adjacent visemes. 0 = puppet snap. */
    coarticulationMs: number;
    visemeIntensity: number;
  };
  thresholds: { backToLab: number; shippable: number; uncannyValleyTarget: number };
}

export const DEFAULT_REALNESS: RealnessProfile = {
  name: "Phase One · Baseline Human",
  breath: {
    rateHz: 0.14,
    amplitudeModHz: 0.031,
    amplitudeModDepth: 0.34,
    travelPx: 2.3,
    scaleDepth: 0.0042,
    luminanceDepth: 0.006,
  },
  sway: { rateHz: 0.053, rotationDeg: 0.38, driftRateHz: 0.023, driftPx: 1.5 },
  blink: { implemented: true, frequencyBpm: 14, durationFrames: 10 },
  attention: {
    minIntervalMs: 4200,
    maxIntervalMs: 11000,
    durationMs: 300,
    travelPx: 3.2,
    rotationDeg: 0.5,
  },
  speech: {
    jawGain: 1.4,
    energyGain: 0.4,
    attack: 0.22,
    liftPx: 1.6,
    scaleDepth: 0.011,
    luminanceDepth: 0.028,
    coarticulationMs: 55,
    visemeIntensity: 1.0,
  },
  thresholds: { backToLab: 70, shippable: 85, uncannyValleyTarget: 2.0 },
};

/** Live profile — read synchronously by the 60Hz motion loop. */
export let realness: RealnessProfile = DEFAULT_REALNESS;

const CONTROLLER = import.meta.env.VITE_ACE_HTTP_URL || "http://127.0.0.1:8100";

function merge(base: RealnessProfile, patch: Partial<RealnessProfile>): RealnessProfile {
  const out = { ...base } as RealnessProfile;
  for (const [k, v] of Object.entries(patch)) {
    if (v && typeof v === "object" && !Array.isArray(v)) {
      // @ts-expect-error — one-level structural merge over known sections
      out[k] = { ...base[k], ...v };
    } else if (v != null) {
      // @ts-expect-error — scalar passthrough
      out[k] = v;
    }
  }
  return out;
}

/** Pull the lab profile. Silent on failure — defaults already work. */
export async function loadRealnessProfile(): Promise<RealnessProfile> {
  try {
    const res = await fetch(`${CONTROLLER}/v1/realness`, { cache: "no-store" });
    if (res.ok) {
      const data = await res.json();
      if (data && !data.error) realness = merge(DEFAULT_REALNESS, data);
    }
  } catch {
    /* defaults stand */
  }
  return realness;
}

if (typeof window !== "undefined") {
  (window as unknown as { __EVE_REALNESS__?: () => RealnessProfile }).__EVE_REALNESS__ = () =>
    realness;
}
