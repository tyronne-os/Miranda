import type { AceEdgeDef, AceNodeDef, StageContract } from "@/lib/stageMachine/types";

/**
 * Canonical All-NVIDIA ACE cortex topology for Instant Presence.
 * Control plane nodes stay hot at L0; data plane warms without blocking presence.
 */
export const ACE_NODES: AceNodeDef[] = [
  {
    id: "mic",
    label: "Mic Ingress",
    kind: "ingress",
    plane: "control",
    requiredFrom: "L0",
    description: "Local capture + VAD gate. Control-plane entry.",
    intro:
      "Captures local audio and runs voice-activity detection so the cortex knows when a guest starts speaking — without waiting on the full speech stack.",
    roleInPipeline: "Control-plane entry — ears of Instant Presence",
    tags: ["VAD", "PCM", "L0-hot"],
    latencyBudgetMs: 20,
  },
  {
    id: "presence",
    label: "Instant Presence",
    kind: "presence",
    plane: "control",
    requiredFrom: "L0",
    description: "L0 idle avatar + gaze/breath loop. Must answer <1s.",
    intro:
      "The always-on face of EVE. Holds gaze, breath, and micro-expression so the guest is never staring at a cold boot — Instant Presence answers under one second.",
    roleInPipeline: "L0 mirror — guest-facing presence contract",
    tags: ["L0", "<1s", "gaze"],
    latencyBudgetMs: 80,
  },
  {
    id: "syncer",
    label: "Spatial Syncer",
    kind: "bus",
    plane: "control",
    requiredFrom: "L0",
    description: "Stage bus + media clock. Couples cortex ↔ studio.",
    intro:
      "Stage bus and media clock that couples ACE Cortex to Live Studio. Keeps blendshapes, intent, and pixel takeover on one timeline.",
    roleInPipeline: "Clock + stage bus across cortex and studio",
    tags: ["clock", "bus", "sync"],
    latencyBudgetMs: 8,
  },
  {
    id: "riva-asr",
    label: "Riva ASR",
    kind: "asr",
    plane: "data",
    requiredFrom: "L1",
    description: "Streaming speech recognition (NIM).",
    intro:
      "NVIDIA Riva turns the live mic stream into tokens. This is the first data-plane hop once L1 warms — speech becomes text the agent can reason over.",
    roleInPipeline: "Speech → text (NIM ASR)",
    tags: ["Riva", "NIM", "ASR"],
    latencyBudgetMs: 180,
  },
  {
    id: "nemotron",
    label: "Nemotron Agent",
    kind: "agent",
    plane: "data",
    requiredFrom: "L1",
    description: "Reasoning / dialogue policy on NVIDIA stack.",
    intro:
      "Nemotron holds dialogue policy and intent. It decides what EVE says next and signals the syncer so face, voice, and stage stay aligned.",
    roleInPipeline: "Reasoning + dialogue policy",
    tags: ["Nemotron", "intent", "LLM"],
    latencyBudgetMs: 350,
  },
  {
    id: "riva-tts",
    label: "Riva TTS",
    kind: "tts",
    plane: "data",
    requiredFrom: "L1",
    description: "Neural TTS stream into Audio2Face.",
    intro:
      "Neural text-to-speech on the NVIDIA path. Streams audio into Audio2Face so lip motion is driven by the same waveform the guest hears.",
    roleInPipeline: "Text → neural speech stream",
    tags: ["Riva", "TTS", "wav"],
    latencyBudgetMs: 160,
  },
  {
    id: "a2f",
    label: "Audio2Face-3D",
    kind: "a2f",
    plane: "data",
    requiredFrom: "L1",
    description: "ARKit 52 blendshapes — not generic mesh deform.",
    intro:
      "Audio2Face-3D emits true ARKit 52-channel blendshapes — not a generic mesh warp. This is the face truth that L1 Live Studio consumes.",
    roleInPipeline: "Audio → ARKit 52 blendshapes",
    tags: ["A2F", "ARKit-52", "face"],
    latencyBudgetMs: 40,
  },
  {
    id: "animgraph",
    label: "AnimGraph",
    kind: "animgraph",
    plane: "data",
    requiredFrom: "L1",
    description: "Body / gesture graph driven by intent + prosody.",
    intro:
      "Body and gesture graph driven by agent intent and prosody. Complements A2F face channels so presence feels embodied, not just lip-synced.",
    roleInPipeline: "Intent + prosody → body / gesture",
    tags: ["gesture", "body", "prosody"],
    latencyBudgetMs: 33,
  },
  {
    id: "omniverse",
    label: "Omniverse Stream",
    kind: "omniverse",
    plane: "data",
    requiredFrom: "L2",
    description: "L2 cinematic pixel takeover. Never a boot blocker.",
    intro:
      "Omniverse pixel stream for L2 cinematic takeover. Deliberately optional at boot — Instant Presence never waits on full render to greet the guest.",
    roleInPipeline: "L2 cinematic pixel path (non-blocking)",
    tags: ["Omniverse", "L2", "pixels"],
    latencyBudgetMs: 50,
  },
];

export const ACE_EDGES: AceEdgeDef[] = [
  { id: "e-mic-asr", source: "mic", target: "riva-asr", kind: "audio", label: "PCM" },
  { id: "e-mic-presence", source: "mic", target: "presence", kind: "control", label: "VAD" },
  { id: "e-asr-agent", source: "riva-asr", target: "nemotron", kind: "text", label: "tokens" },
  { id: "e-agent-tts", source: "nemotron", target: "riva-tts", kind: "text", label: "reply" },
  { id: "e-tts-a2f", source: "riva-tts", target: "a2f", kind: "audio", label: "wav" },
  { id: "e-a2f-anim", source: "a2f", target: "animgraph", kind: "blendshape", label: "ARKit" },
  { id: "e-anim-ov", source: "animgraph", target: "omniverse", kind: "anim", label: "pose" },
  { id: "e-a2f-presence", source: "a2f", target: "presence", kind: "blendshape", label: "face" },
  { id: "e-ov-presence", source: "omniverse", target: "presence", kind: "pixel", label: "L2" },
  { id: "e-sync-all", source: "syncer", target: "presence", kind: "clock", label: "clock" },
  { id: "e-sync-a2f", source: "syncer", target: "a2f", kind: "clock", label: "clock" },
  { id: "e-sync-ov", source: "syncer", target: "omniverse", kind: "clock", label: "clock" },
  { id: "e-agent-sync", source: "nemotron", target: "syncer", kind: "control", label: "intent" },
];

export const STAGE_CONTRACTS: Record<"L0" | "L1" | "L2", StageContract> = {
  L0: {
    stage: "L0",
    title: "Idle Presence",
    subtitle: "Control plane only — gaze, breath, micro-expression",
    controlBudgetMs: 1000,
    hotNodes: ["mic", "presence", "syncer"],
    warmNodes: ["riva-asr", "nemotron", "riva-tts", "a2f", "animgraph", "omniverse"],
    requiresPixelStream: false,
    requiresBlendshapes: false,
  },
  L1: {
    stage: "L1",
    title: "Audio2Face Live",
    subtitle: "ASR → Agent → TTS → ARKit blendshapes + AnimGraph",
    controlBudgetMs: 1000,
    hotNodes: ["mic", "presence", "syncer", "riva-asr", "nemotron", "riva-tts", "a2f", "animgraph"],
    warmNodes: ["omniverse"],
    requiresPixelStream: false,
    requiresBlendshapes: true,
  },
  L2: {
    stage: "L2",
    title: "Omniverse Cinematic",
    subtitle: "Pixel stream takeover — full ACE data plane",
    controlBudgetMs: 1000,
    hotNodes: [
      "mic",
      "presence",
      "syncer",
      "riva-asr",
      "nemotron",
      "riva-tts",
      "a2f",
      "animgraph",
      "omniverse",
    ],
    warmNodes: [],
    requiresPixelStream: true,
    requiresBlendshapes: true,
  },
};

/** Visual size of AceNodeCard — used to center nodes on the oval path */
const NODE_W = 168;
const NODE_H = 112;

/**
 * Pipeline order around the oval (counter-clockwise from west).
 * Keeps the speech path readable while fitting every node on first paint.
 */
const OVAL_ORDER = [
  "mic",
  "riva-asr",
  "nemotron",
  "riva-tts",
  "a2f",
  "animgraph",
  "omniverse",
  "presence",
  "syncer",
] as const;

/**
 * Place nodes on an ellipse so the full ACE cortex is visible on load
 * (no off-stage nodes from the old linear left→right strip).
 */
export function layoutOval(
  ids: readonly string[],
  opts: {
    cx?: number;
    cy?: number;
    rx?: number;
    ry?: number;
    /** Radians; default = west (−π) so mic starts at the left */
    startAngle?: number;
    nodeW?: number;
    nodeH?: number;
  } = {},
): Record<string, { x: number; y: number }> {
  const {
    cx = 520,
    cy = 360,
    rx = 420,
    ry = 260,
    startAngle = -Math.PI,
    nodeW = NODE_W,
    nodeH = NODE_H,
  } = opts;

  const n = ids.length || 1;
  const out: Record<string, { x: number; y: number }> = {};

  ids.forEach((id, i) => {
    const a = startAngle + (i / n) * Math.PI * 2;
    out[id] = {
      x: cx + rx * Math.cos(a) - nodeW / 2,
      y: cy + ry * Math.sin(a) - nodeH / 2,
    };
  });

  return out;
}

/** Default oval graph layout — all nine ACE nodes visible on page load */
export const NODE_POSITIONS: Record<string, { x: number; y: number }> = layoutOval(OVAL_ORDER);
