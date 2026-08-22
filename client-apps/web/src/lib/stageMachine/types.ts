/** Instant Presence Standard — stage & plane contracts */

export type PresenceStage = "L0" | "L1" | "L2";

export type PlaneMode = "control" | "data";

// WO-5 T1: rewired from the old eve-ecc/NVIDIA-ACE node set (ingress/asr/
// agent/tts/a2f/animgraph/omniverse/bus/presence) to the real Miranda-Engine
// architecture. Every value below corresponds to an actual crate, an actual
// still-live Node.js service, or an actual not-yet-built browser component —
// no role labels for pipeline stages that don't exist in this codebase.
export type NodeKind =
  | "ingress" // browser mic capture (getUserMedia) — client-apps/web/src/audio/MicCapture.ts
  | "native-capture" // miranda-audio: cpal mic capture + parakeet.cpp FFI (Pipeline 2)
  | "ipc-bus" // miranda-ipc: lock-free SHM ring buffer, /dev/shm/miranda_bus
  | "kinematics" // miranda-nodes: oscillators + acoustic solver + compositor + 60fps dispatcher
  | "supervisor" // miranda-supervisor: turn-taking state machine + Nemotron-Flash routing
  | "transport" // miranda-transport: WebRTC DataChannel hub + Axum telemetry (WO-4)
  | "cloud-bridge" // client-services/ace-controller (Node.js): Pipeline 1 — Whisper ASR + NVIDIA NIM
  | "renderer" // WO-5: WebGPU/WGSL Gaussian-splat viewport (browser)
  | "presence"; // browser-side L0 idle presence layer (client-apps/web/src/components/eve)

export type NodeHealth = "cold" | "warming" | "ready" | "hot" | "degraded" | "error";

// WO-5 T1: `pixel` and `anim` were ACE/Omniverse-specific (cinematic
// pixel takeover, gesture-graph pose) and have no real-crate equivalent in
// this codebase, so they're dropped rather than reused for something they
// don't describe. `ipc` (miranda-ipc SHM ring traffic) and `datachannel`
// (miranda-transport's binary MRD1 broadcast) are added because the real
// signal path has legs neither audio/text/blendshape/control/clock covers.
export type EdgeKind = "audio" | "text" | "blendshape" | "ipc" | "datachannel" | "control" | "clock";

export interface AceNodeDef {
  id: string;
  label: string;
  kind: NodeKind;
  plane: PlaneMode;
  /** Minimum stage where this node must be hot for Instant Presence */
  requiredFrom: PresenceStage;
  description: string;
  /** Hover education — short role intro (Understand-Anything DNA) */
  intro: string;
  /** One-line place in the Instant Presence pipeline */
  roleInPipeline: string;
  /** Compact education tags for tooltip chips */
  tags: string[];
  latencyBudgetMs: number;
}

export interface AceEdgeDef {
  id: string;
  source: string;
  target: string;
  kind: EdgeKind;
  label: string;
}

export interface StageContract {
  stage: PresenceStage;
  title: string;
  subtitle: string;
  /** Control-plane response budget (ms) — Instant Presence hard rule */
  controlBudgetMs: number;
  /** Nodes that must be ready at this stage */
  hotNodes: string[];
  /** Nodes allowed to still warm */
  warmNodes: string[];
  /** Whether Omniverse pixel stream is required */
  requiresPixelStream: boolean;
  /** Whether ARKit blendshapes are live */
  requiresBlendshapes: boolean;
}

export interface MediaClockSnapshot {
  /** Monotonic media time in ms since session start */
  tMediaMs: number;
  /** Wall clock ISO */
  wallIso: string;
  /** Drift between control ticks and media (ms) */
  driftMs: number;
  /** Frames / events processed this second */
  pps: number;
}

export interface BlendshapeFrame {
  tMediaMs: number;
  /** ARKit 52-channel weights 0..1 */
  weights: Record<string, number>;
  energy: number;
  viseme: string;
}

export interface BusEvent {
  id: string;
  t: number;
  kind: "stage" | "node" | "sync" | "presence" | "user" | "system";
  level: "info" | "ok" | "warn" | "error";
  message: string;
  meta?: Record<string, string | number | boolean>;
}

export interface NodeRuntime {
  id: string;
  health: NodeHealth;
  latencyMs: number;
  load: number;
  lastBeatMs: number;
  message: string;
}

export const STAGE_ORDER: PresenceStage[] = ["L0", "L1", "L2"];

export const ARKIT_CHANNELS = [
  "jawOpen",
  "mouthClose",
  "mouthFunnel",
  "mouthPucker",
  "mouthSmileLeft",
  "mouthSmileRight",
  "mouthFrownLeft",
  "mouthFrownRight",
  "mouthUpperUpLeft",
  "mouthUpperUpRight",
  "mouthLowerDownLeft",
  "mouthLowerDownRight",
  "mouthLeft",
  "mouthRight",
  "mouthRollUpper",
  "mouthRollLower",
  "mouthShrugUpper",
  "mouthShrugLower",
  "mouthPressLeft",
  "mouthPressRight",
  "mouthDimpleLeft",
  "mouthDimpleRight",
  "mouthStretchLeft",
  "mouthStretchRight",
  "tongueOut",
  "eyeBlinkLeft",
  "eyeBlinkRight",
  "eyeLookUpLeft",
  "eyeLookUpRight",
  "eyeLookDownLeft",
  "eyeLookDownRight",
  "eyeLookInLeft",
  "eyeLookInRight",
  "eyeLookOutLeft",
  "eyeLookOutRight",
  "eyeSquintLeft",
  "eyeSquintRight",
  "eyeWideLeft",
  "eyeWideRight",
  "browDownLeft",
  "browDownRight",
  "browInnerUp",
  "browOuterUpLeft",
  "browOuterUpRight",
  "cheekPuff",
  "cheekSquintLeft",
  "cheekSquintRight",
  "noseSneerLeft",
  "noseSneerRight",
  "jawForward",
  "jawLeft",
  "jawRight",
] as const;

export type ArkitChannel = (typeof ARKIT_CHANNELS)[number];
