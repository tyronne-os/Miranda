/**
 * Live indicator science for The Cerebral Project.
 *
 * Each meter maps raw ACE runtime signals → 0..1 with EMA smoothing
 * so the UI reacts like the warm path meter (stable fill, live label).
 *
 * Rubrics (research-facing, not decorative):
 * - WARM: data-plane readiness along the stage contract
 * - LAG: multi-source latency pressure (control, nodes, clock drift, slip)
 * - CONTINUITY: cognitive + conversational + cultural + cohesive awareness
 * - PRESENCE: Instant Presence control-plane quality / greetability
 * - GPU: weighted NIM/OV cost model vs full L2 envelope
 */

import { ACE_NODES } from "@/data/aceTopology";
import type {
  BlendshapeFrame,
  MediaClockSnapshot,
  NodeRuntime,
  PresenceStage,
} from "@/lib/stageMachine/types";
import type {
  ContinuityBreakdown,
  GpuCostBreakdown,
  LiveMeterId,
  LiveMeterReading,
  LiveMetricsSnapshot,
  LiveMetricsState,
  MeterTone,
} from "./types";

const METER_IDS: LiveMeterId[] = ["warm", "lag", "continuity", "presence", "gpu"];

/** Relative GPU weight per ACE node (share of full cinematic envelope). */
const NODE_GPU_WEIGHT: Record<string, number> = {
  mic: 0.02,
  presence: 0.04,
  syncer: 0.03,
  "riva-asr": 0.11,
  nemotron: 0.16,
  "riva-tts": 0.1,
  a2f: 0.27,
  animgraph: 0.08,
  omniverse: 0.42,
};

const FULL_ENVELOPE = Object.values(NODE_GPU_WEIGHT).reduce((a, b) => a + b, 0);

function clamp01(n: number) {
  return Math.min(1, Math.max(0, n));
}

function ema(prev: number, next: number, alpha: number) {
  return prev + (next - prev) * alpha;
}

function healthScore(h: NodeRuntime["health"] | undefined): number {
  switch (h) {
    case "hot":
      return 1;
    case "ready":
      return 0.82;
    case "warming":
      return 0.48;
    case "degraded":
      return 0.35;
    case "error":
      return 0.12;
    default:
      return 0.08;
  }
}

function healthGpuFactor(h: NodeRuntime["health"] | undefined): number {
  switch (h) {
    case "hot":
      return 1;
    case "ready":
      return 0.72;
    case "warming":
      return 0.5;
    case "degraded":
      return 0.88; // retries / thrash cost more
    case "error":
      return 0.25;
    default:
      return 0.05;
  }
}

function toneFor(id: LiveMeterId, v: number): MeterTone {
  if (id === "lag" || id === "gpu") {
    if (v < 0.28) return "good";
    if (v < 0.55) return "neutral";
    if (v < 0.78) return "warn";
    return "bad";
  }
  // higher-is-better meters
  if (v >= 0.72) return "good";
  if (v >= 0.48) return "neutral";
  if (v >= 0.28) return "warn";
  return "bad";
}

function emptyMeters(): Record<LiveMeterId, LiveMeterReading> {
  const base = (id: LiveMeterId, label: string): LiveMeterReading => ({
    id,
    label,
    value01: 0,
    displayPct: 0,
    unit: "%",
    tone: "neutral",
    detail: "",
  });
  return {
    warm: base("warm", "warm"),
    lag: base("lag", "lag"),
    continuity: base("continuity", "cont"),
    presence: base("presence", "pres"),
    gpu: base("gpu", "gpu"),
  };
}

export function createLiveMetricsState(): LiveMetricsState {
  return {
    meters: emptyMeters(),
    continuity: { cognitive: 0, conversational: 0, cultural: 0, cohesive: 0 },
    gpu: {
      index01: 0,
      wattsEst: 12,
      sessionCost: 0,
      byPlane: { control: 0, data: 0 },
      heavyNodes: [],
    },
    lagSources: {
      controlRatio: 0,
      nodeLatencyRatio: 0,
      driftRatio: 0,
      slipRatio: 0,
    },
    _smooth: { warm: 0.35, lag: 0.08, continuity: 0.7, presence: 0.85, gpu: 0.12 },
    _blendBaseline: null,
    _sessionGpuIntegral: 0,
    _talkSeconds: 0,
    _interactionSeconds: 0,
    _lastReportAt: 0,
  };
}

function rmsBlendDrift(
  frame: BlendshapeFrame,
  baseline: Record<string, number>,
): number {
  let acc = 0;
  let n = 0;
  for (const [k, v] of Object.entries(frame.weights)) {
    const b = baseline[k] ?? 0;
    const d = v - b;
    acc += d * d;
    n += 1;
  }
  return n ? Math.sqrt(acc / n) : 0;
}

function updateBaseline(
  prev: Record<string, number> | null,
  frame: BlendshapeFrame,
  talking: boolean,
  dtSec: number,
): Record<string, number> {
  // Slow baseline while idle; freeze more while talking so intentional motion ≠ drift
  const alpha = talking ? 0.02 * Math.min(1, dtSec * 30) : 0.08 * Math.min(1, dtSec * 30);
  const next: Record<string, number> = prev ? { ...prev } : {};
  for (const [k, v] of Object.entries(frame.weights)) {
    const p = prev?.[k];
    next[k] = p === undefined ? v : p + (v - p) * alpha;
  }
  return next;
}

export interface MetricsTickInput {
  dtMs: number;
  stage: PresenceStage;
  targetStage: PresenceStage;
  warmProgress: number;
  nodes: Record<string, NodeRuntime>;
  clock: MediaClockSnapshot;
  blend: BlendshapeFrame;
  controlMs: number;
  controlBudgetMs: number;
  talking: boolean;
  micArmed: boolean;
  presenceEnergy: number;
  /** Optional media slip injected this tick (ms) */
  slipMs?: number;
}

function computeLagRaw(input: MetricsTickInput) {
  const { nodes, clock, controlMs, controlBudgetMs, slipMs = 0 } = input;

  const controlRatio = controlMs / Math.max(1, controlBudgetMs);

  let latAcc = 0;
  let latN = 0;
  for (const def of ACE_NODES) {
    const rt = nodes[def.id];
    if (!rt) continue;
    if (rt.health === "cold") continue;
    latAcc += rt.latencyMs / Math.max(1, def.latencyBudgetMs);
    latN += 1;
  }
  const nodeLatencyRatio = latN ? latAcc / latN : 0;

  // 33.3ms ≈ one 30fps frame; drift beyond a frame is user-visible lag
  const driftRatio = Math.abs(clock.driftMs) / 33.3;
  const slipRatio = Math.abs(slipMs) / 16;

  // Weighted lag pressure (can exceed 1 before clamp — captures spikes)
  const pressure =
    controlRatio * 0.34 + nodeLatencyRatio * 0.36 + driftRatio * 0.18 + slipRatio * 0.12;

  // Soft knee: small pressure stays low; overload climbs fast
  const raw = clamp01(Math.pow(Math.max(0, pressure - 0.35), 1.15) * 1.35 + pressure * 0.22);

  return {
    raw,
    sources: {
      controlRatio: clamp01(controlRatio),
      nodeLatencyRatio: clamp01(nodeLatencyRatio / 2),
      driftRatio: clamp01(driftRatio),
      slipRatio: clamp01(slipRatio),
    },
  };
}

function computeContinuityRaw(input: MetricsTickInput, baseline: Record<string, number> | null) {
  const { nodes, blend, talking, micArmed, presenceEnergy, stage, warmProgress } = input;
  const h = (id: string) => healthScore(nodes[id]?.health);

  // Cognitive: agent path + reasoning readiness
  const agentPath = (h("riva-asr") + h("nemotron") + h("syncer")) / 3;
  const stageCog =
    stage === "L0" ? 0.55 + warmProgress * 0.25 : stage === "L1" ? 0.82 : 0.9;
  const cognitive = clamp01(agentPath * 0.7 + stageCog * 0.3);

  // Conversational: ears/voice + talk alignment
  const speechPath = (h("mic") + h("riva-asr") + h("riva-tts") + h("a2f")) / 4;
  const jaw = blend.weights.jawOpen ?? 0;
  const visemeLive = blend.viseme !== "sil" ? 1 : 0;
  const talkAlign = talking
    ? clamp01(0.35 + jaw * 0.4 + visemeLive * 0.2 + presenceEnergy * 0.25)
    : clamp01(0.75 + (1 - jaw) * 0.15);
  const micFactor = micArmed ? 1 : 0.45;
  const conversational = clamp01(speechPath * 0.55 + talkAlign * 0.35 + micFactor * 0.1);

  // Cultural: expression symmetry + calm brow (tone stability proxy)
  const smileL = blend.weights.mouthSmileLeft ?? 0;
  const smileR = blend.weights.mouthSmileRight ?? 0;
  const symmetry = 1 - Math.min(1, Math.abs(smileL - smileR) * 8);
  const brow = blend.weights.browInnerUp ?? 0;
  const browCalm = 1 - clamp01((brow - 0.12) * 3.5);
  const smileTone = clamp01(1 - Math.abs(smileL - (talking ? 0.18 : 0.1)) * 2.2);
  const cultural = clamp01(symmetry * 0.4 + browCalm * 0.35 + smileTone * 0.25);

  // Cohesive: anti facial-drift over long interactions
  const drift = baseline ? rmsBlendDrift(blend, baseline) : 0;
  // Drift of ~0.08 RMS is mild; 0.2+ is visible identity slip
  const cohesive = clamp01(Math.exp(-drift / 0.09) * (0.85 + (1 - Math.min(1, drift * 4)) * 0.15));

  const breakdown: ContinuityBreakdown = {
    cognitive: clamp01(cognitive),
    conversational: clamp01(conversational),
    cultural: clamp01(cultural),
    cohesive: clamp01(cohesive),
  };

  // Research weights — conversational + cognitive dominate live sessions
  const raw = clamp01(
    breakdown.cognitive * 0.3 +
      breakdown.conversational * 0.3 +
      breakdown.cultural * 0.15 +
      breakdown.cohesive * 0.25,
  );

  return { raw, breakdown, drift };
}

function computePresenceRaw(input: MetricsTickInput) {
  const { nodes, controlMs, controlBudgetMs, stage, talking, presenceEnergy } = input;
  const controlOk = clamp01(1 - controlMs / Math.max(1, controlBudgetMs));
  const presenceNode = healthScore(nodes.presence?.health);
  const syncNode = healthScore(nodes.syncer?.health);
  const micNode = healthScore(nodes.mic?.health);
  // Instant Presence hard rule: greetability never waits on data plane
  const greet =
    stage === "L0"
      ? 0.9
      : stage === "L1"
        ? 0.84
        : 0.8;
  const energyFit = talking
    ? clamp01(presenceEnergy / 0.7)
    : clamp01(1 - Math.abs(presenceEnergy - 0.12) * 3);
  return clamp01(controlOk * 0.4 + presenceNode * 0.25 + syncNode * 0.15 + micNode * 0.1 + greet * 0.05 + energyFit * 0.05);
}

function computeGpu(input: MetricsTickInput, sessionIntegral: number, dtSec: number): GpuCostBreakdown {
  const { nodes, stage, warmProgress, talking, blend, targetStage } = input;

  let control = 0;
  let data = 0;
  const shares: Array<{ id: string; share: number }> = [];

  const stageDuty =
    stage === "L2" ? 1 : stage === "L1" ? 0.72 : 0.22 + warmProgress * 0.35;
  const targetBoost = targetStage > stage ? 0.08 : 0;
  const talkBoost = talking ? 1.12 : 1;
  const blendBoost = 1 + (blend.energy ?? 0) * 0.15;

  for (const def of ACE_NODES) {
    const w = NODE_GPU_WEIGHT[def.id] ?? 0.05;
    const rt = nodes[def.id];
    const util = healthGpuFactor(rt?.health) * (0.35 + (rt?.load ?? 0) * 0.65);
    let nodeDuty = def.plane === "control" ? 0.85 : stageDuty;

    // Omniverse only bites hard on L2 / L2 target
    if (def.id === "omniverse") {
      nodeDuty = stage === "L2" ? 1 : targetStage === "L2" ? 0.35 + warmProgress * 0.4 : warmProgress * 0.12;
    }
    // A2F / anim scale with L1+
    if (def.id === "a2f" || def.id === "animgraph") {
      nodeDuty = stage === "L0" ? 0.15 + warmProgress * 0.35 : stageDuty;
    }

    const share = w * util * nodeDuty * talkBoost * blendBoost * (1 + targetBoost);
    shares.push({ id: def.id, share });
    if (def.plane === "control") control += share;
    else data += share;
  }

  const total = control + data;
  const index01 = clamp01(total / FULL_ENVELOPE);

  // Idle shell ~18W display path; full L2 envelope ~240W class relative unit
  const wattsEst = Math.round(18 + index01 * 222);

  const sessionCost = sessionIntegral + index01 * dtSec;

  shares.sort((a, b) => b.share - a.share);

  return {
    index01,
    wattsEst,
    sessionCost,
    byPlane: { control, data },
    heavyNodes: shares.slice(0, 4).map((s) => ({
      id: s.id,
      share: Math.round((s.share / Math.max(1e-6, total)) * 1000) / 1000,
    })),
  };
}

function reading(
  id: LiveMeterId,
  label: string,
  value01: number,
  detail: string,
  unit = "%",
): LiveMeterReading {
  const v = clamp01(value01);
  return {
    id,
    label,
    value01: v,
    displayPct: Math.round(v * 100),
    unit,
    tone: toneFor(id, v),
    detail,
  };
}

/**
 * Advance live metrics one presence tick. Mutates and returns next state.
 */
export function tickLiveMetrics(
  state: LiveMetricsState,
  input: MetricsTickInput,
): LiveMetricsState {
  const dtSec = Math.max(0.001, input.dtMs / 1000);
  const lag = computeLagRaw(input);
  const baseline = updateBaseline(state._blendBaseline, input.blend, input.talking, dtSec);
  const cont = computeContinuityRaw(input, baseline);
  const presenceRaw = computePresenceRaw(input);
  const gpu = computeGpu(input, state._sessionGpuIntegral, dtSec);

  // EMA alphas — warm tracks path closely; lag reacts faster to spikes
  const alphas: Record<LiveMeterId, number> = {
    warm: 0.22,
    lag: 0.35,
    continuity: 0.18,
    presence: 0.2,
    gpu: 0.28,
  };

  const targets: Record<LiveMeterId, number> = {
    warm: clamp01(input.warmProgress),
    lag: lag.raw,
    continuity: cont.raw,
    presence: presenceRaw,
    gpu: gpu.index01,
  };

  const smooth = { ...state._smooth };
  for (const id of METER_IDS) {
    smooth[id] = ema(smooth[id], targets[id], alphas[id]);
  }

  const meters: Record<LiveMeterId, LiveMeterReading> = {
    warm: reading(
      "warm",
      "warm",
      smooth.warm,
      `Warm path ${Math.round(smooth.warm * 100)}% · stage ${input.stage}→${input.targetStage}`,
    ),
    lag: reading(
      "lag",
      "lag",
      smooth.lag,
      `Lag pressure ${Math.round(smooth.lag * 100)}% · ctrl ${input.controlMs}ms · drift ${input.clock.driftMs.toFixed(1)}ms`,
    ),
    continuity: reading(
      "continuity",
      "cont",
      smooth.continuity,
      `Continuity ${Math.round(smooth.continuity * 100)}% · cog ${Math.round(cont.breakdown.cognitive * 100)} · conv ${Math.round(cont.breakdown.conversational * 100)} · cult ${Math.round(cont.breakdown.cultural * 100)} · cohere ${Math.round(cont.breakdown.cohesive * 100)}`,
    ),
    presence: reading(
      "presence",
      "pres",
      smooth.presence,
      `Instant Presence ${Math.round(smooth.presence * 100)}% · control ${input.controlMs}/${input.controlBudgetMs}ms`,
    ),
    gpu: reading(
      "gpu",
      "gpu",
      smooth.gpu,
      `GPU cost ${Math.round(smooth.gpu * 100)}% · ~${gpu.wattsEst}W · session ${gpu.sessionCost.toFixed(1)}s·u`,
    ),
  };

  return {
    meters,
    continuity: cont.breakdown,
    gpu,
    lagSources: lag.sources,
    _smooth: smooth,
    _blendBaseline: baseline,
    _sessionGpuIntegral: gpu.sessionCost,
    _talkSeconds: state._talkSeconds + (input.talking ? dtSec : 0),
    _interactionSeconds: state._interactionSeconds + dtSec,
    _lastReportAt: state._lastReportAt,
  };
}

export function toMetricsSnapshot(
  state: LiveMetricsState,
  clock: MediaClockSnapshot,
): LiveMetricsSnapshot {
  return {
    tMediaMs: clock.tMediaMs,
    wallIso: clock.wallIso,
    meters: state.meters,
    continuity: state.continuity,
    gpu: state.gpu,
    lagSources: state.lagSources,
  };
}

export const LIVE_METER_ORDER: LiveMeterId[] = ["warm", "lag", "continuity", "presence", "gpu"];
