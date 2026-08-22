import type { AceEdgeDef, AceNodeDef, StageContract } from "@/lib/stageMachine/types";

/**
 * WO-5 T1 — Miranda-Engine topology, rewired from the old eve-ecc/NVIDIA-ACE
 * node set to the real architecture.
 *
 * # What changed and why
 *
 * The original topology (mic/presence/syncer/riva-asr/nemotron/riva-tts/
 * a2f/animgraph/omniverse) described a single hypothetical all-NVIDIA-ACE
 * pipeline that was never built. This repo's real signal path is TWO
 * pipelines sharing one harness:
 *
 * - **Pipeline 1 (cloud, live today)**: browser mic → WebSocket PCM →
 *   `ace-controller` (Node.js) → OpenAI Whisper ASR → NVIDIA NIM chat →
 *   phoneme-direct viseme timeline → browser presence layer. See
 *   `client-services/ace-controller/run.mjs` and `.mjs` siblings.
 * - **Pipeline 2 (local, native, WO-1 through WO-4)**: `miranda-audio`
 *   (cpal capture + parakeet.cpp FFI) → `miranda-ipc` (lock-free SHM ring)
 *   → `miranda-supervisor` (turn-taking) → `miranda-nodes` (oscillators +
 *   acoustic solver + compositor + 60 FPS dispatcher) → `miranda-ipc`
 *   again → `miranda-transport` (WebRTC DataChannel + Axum telemetry) →
 *   browser WebGPU renderer (WO-5, this Work Order, not yet built).
 *
 * Every node below maps to something that actually exists in this repo —
 * a real crate, a real running Node.js service, or a component this exact
 * Work Order is building. There is no node here standing in for a service
 * this project doesn't have (no Riva, no Omniverse, no generic "AnimGraph"
 * — those were placeholders for an architecture this codebase does not
 * implement).
 *
 * `miranda-core` (shared types/constants, no runtime behavior) is
 * deliberately NOT a node here — it's a compile-time dependency every
 * other crate links against, not a step in a live signal path. A node
 * graph exists to show *flow*; a node with no inputs, no outputs, and no
 * runtime presence would just be visual noise.
 */
export const ACE_NODES: AceNodeDef[] = [
  {
    id: "mic",
    label: "Mic Ingress",
    kind: "ingress",
    plane: "control",
    requiredFrom: "L0",
    description: "Browser getUserMedia capture, streamed as PCM over WebSocket.",
    intro:
      "Captures the browser's microphone via getUserMedia and streams 16 kHz mono " +
      "Float32 PCM frames over the existing THE VANITY WebSocket to ace-controller. " +
      "This is Pipeline 1's ear — the cloud-bridge path that's live today.",
    roleInPipeline: "Browser audio capture — Pipeline 1 entry",
    tags: ["getUserMedia", "PCM", "WebSocket", "L0-hot"],
    latencyBudgetMs: 20,
  },
  {
    id: "presence",
    label: "Instant Presence",
    kind: "presence",
    plane: "control",
    requiredFrom: "L0",
    description: "L0 idle avatar layer — gaze/breath/micro-expression on the 2D portrait. Must answer <1s.",
    intro:
      "The always-on face of EVE, running today as a compositor-only transform/filter " +
      "layer on the 2D reference photo (client-apps/web/src/components/eve/). Holds " +
      "gaze, breath, and micro-expression so the guest is never staring at a cold boot. " +
      "This is what the WO-5 WebGPU renderer eventually replaces for the splat viewport, " +
      "but stays the fallback presence layer regardless.",
    roleInPipeline: "L0 mirror — guest-facing presence contract",
    tags: ["L0", "<1s", "gaze", "CSS-compositor"],
    latencyBudgetMs: 80,
  },
  {
    id: "cloud-bridge",
    label: "ace-controller",
    kind: "cloud-bridge",
    plane: "data",
    requiredFrom: "L1",
    description: "Node.js: Whisper ASR + NVIDIA NIM chat + phoneme-direct viseme timeline (Pipeline 1).",
    intro:
      "The live Pipeline 1 cognitive core. Buffers browser PCM in-process, transcribes " +
      "on speech-end via OpenAI Whisper REST, routes the transcript to NVIDIA NIM " +
      "(nemotron-mini-4b-instruct), and derives a zero-drift viseme timeline straight " +
      "from the reply text — before any TTS audio exists. Originally spec'd for AWS " +
      "Bedrock + Transcribe Streaming; both legs are account-locked " +
      "(UnrecognizedClientException, reproduced outside this server), so this pivoted " +
      "to OpenAI Whisper + NVIDIA NIM. bedrockRouter.mjs / transcribeBridge.mjs are left " +
      "intact, unused, for reactivation once AWS access is cleared.",
    roleInPipeline: "Pipeline 1 — cloud ASR + LLM + phoneme timeline",
    tags: ["Node.js", "Whisper", "NVIDIA-NIM", "phoneme-direct"],
    latencyBudgetMs: 350,
  },
  {
    id: "native-capture",
    label: "miranda-audio",
    kind: "native-capture",
    plane: "data",
    requiredFrom: "L1",
    description: "Native cpal mic capture + parakeet.cpp FFI local ASR (Pipeline 2).",
    intro:
      "Pipeline 2's ear — native Rust audio capture via cpal, writing directly into the " +
      "miranda-ipc audio ring, plus a parakeet.cpp FFI binding for fully local, " +
      "offline ASR. Measured at 1.76s to transcribe 1.0s of audio on the dev machine " +
      "(1.76x slower than realtime) — flagged as a known gap, not silently hidden. " +
      "The user has deprioritized parakeet dictation accuracy in favor of shipping the " +
      "harness; this node stays wired for when that's revisited.",
    roleInPipeline: "Pipeline 2 — native audio capture + local ASR",
    tags: ["cpal", "parakeet.cpp", "FFI", "offline"],
    latencyBudgetMs: 1760,
  },
  {
    id: "ipc-bus",
    label: "miranda-ipc",
    kind: "ipc-bus",
    plane: "data",
    requiredFrom: "L1",
    description: "Lock-free SHM ring buffer at /dev/shm/miranda_bus — audio, blendshape, SH lighting, kinematic rings.",
    intro:
      "The WO-1 backbone: four independent single-producer/single-consumer lock-free " +
      "ring buffers over one shared-memory mapping (audio chunks, ARKit-52 blendshape " +
      "frames, spherical-harmonic lighting, and WO-4's kinematic joint-quaternion " +
      "frames). Measured ~72ns round-trip against a <=50us target. MIRI-clean under " +
      "isolation-disabled concurrent SPSC stress. This is the one component every other " +
      "native node reads or writes through.",
    roleInPipeline: "Pipeline-agnostic shared-memory transport backbone",
    tags: ["lock-free", "SPSC", "/dev/shm", "WO-1"],
    latencyBudgetMs: 1,
  },
  {
    id: "supervisor",
    label: "miranda-supervisor",
    kind: "supervisor",
    plane: "data",
    requiredFrom: "L1",
    description: "Turn-taking state machine + Nemotron-Flash routing.",
    intro:
      "Owns the conversational turn state machine (Idle/Listening/Thinking/Speaking) " +
      "and interruption handling — a new speech-start cancels an in-flight NIM call " +
      "promptly rather than letting a stale reply win the turn. Routes finalized " +
      "transcripts to the Nemotron-Flash reasoning endpoint. This is also the intended " +
      "home for the Node Warden concept — localized llama.cpp micro-LLMs monitoring " +
      "their own node's throughput — not yet built.",
    roleInPipeline: "Turn-taking + interruption + reasoning routing",
    tags: ["turn-taking", "Nemotron-Flash", "interruption"],
    latencyBudgetMs: 350,
  },
  {
    id: "kinematics",
    label: "miranda-nodes",
    kind: "kinematics",
    plane: "data",
    requiredFrom: "L1",
    description: "ARKit-52 oscillators (blink/gaze/breath) + SIMD acoustic solver + compositor/damper + 60 FPS dispatcher.",
    intro:
      "The WO-3 kinematics engine — three autonomic oscillators (Weibull-timed " +
      "asymmetric blink, Perlin-noise fixational gaze, sine-wave respiration with " +
      "quaternion head/clavicle routing), a hand-authored SIMD formant-heuristic " +
      "acoustic-to-mouth solver (f32x4, measured 2.7us/frame against a 200us budget, " +
      "not a trained regressor — TONGUE_OUT is deliberately never driven), and the " +
      "compositor/motion-damper that layers all sources with velocity+acceleration " +
      "clamping. Verified end-to-end on a real /dev/shm bus: 60.03 fps, zero dropped " +
      "frames, zero repeated frames over a 30s run.",
    roleInPipeline: "Face truth — audio/oscillators → ARKit-52 + joint quaternions",
    tags: ["SIMD", "ARKit-52", "60fps", "No-Loop"],
    latencyBudgetMs: 17,
  },
  {
    id: "transport",
    label: "miranda-transport",
    kind: "transport",
    plane: "data",
    requiredFrom: "L1",
    description: "WebRTC DataChannel binary frame hub + Axum WebSocket telemetry + circuit breaker.",
    intro:
      "The WO-4 transport layer. Broadcasts 312-byte binary MRD1 packets (ARKit-52 " +
      "blendshape frame + kinematic joint-quaternion frame) to every connected browser " +
      "over a WebSocket-backed DataChannel-equivalent, plus a separate Axum telemetry " +
      "WebSocket streaming JSON dispatcher stats and a three-state circuit breaker " +
      "(Closed/HalfOpen/Open) so a stalled render surfaces as a signal, never a silent " +
      "freeze. A full webrtc-rs DTLS/ICE stack is declared behind an optional feature " +
      "flag for production NAT traversal; not compiled by default in this environment.",
    roleInPipeline: "Binary frame broadcast + telemetry — Pipeline 2's exit to the browser",
    tags: ["WebRTC", "DataChannel", "Axum", "circuit-breaker"],
    latencyBudgetMs: 15,
  },
  {
    id: "renderer",
    label: "WebGPU Viewport",
    kind: "renderer",
    plane: "data",
    requiredFrom: "L2",
    description: "WGSL Gaussian-splat viewport — this Work Order's net-new piece. Not yet built.",
    intro:
      "The genuinely new component WO-5 exists to build: a WebGPU/WGSL viewport that " +
      "ingests the 312-byte MRD1 binary packets from miranda-transport and deforms a " +
      "3D Gaussian-splat representation of EVE in real time. Runs against a " +
      "placeholder/test splat asset while the real GaussianAvatars/FLAME/TetGS " +
      "pipeline is researched separately (live-avatar-expert skill) — this Work Order " +
      "does not block on that research finishing. Deliberately optional at boot: " +
      "Instant Presence never waits on this to greet the guest through the L0 " +
      "CSS-compositor presence layer.",
    roleInPipeline: "L2 — WebGPU splat rendering (net-new, in progress)",
    tags: ["WebGPU", "WGSL", "Gaussian-splat", "L2", "not-yet-built"],
    latencyBudgetMs: 16,
  },
];

export const ACE_EDGES: AceEdgeDef[] = [
  // Pipeline 1 — cloud bridge (live today)
  { id: "e-mic-bridge", source: "mic", target: "cloud-bridge", kind: "audio", label: "PCM/WS" },
  { id: "e-mic-presence", source: "mic", target: "presence", kind: "control", label: "VAD" },
  { id: "e-bridge-presence", source: "cloud-bridge", target: "presence", kind: "blendshape", label: "phoneme-direct" },

  // Pipeline 2 — native harness (WO-1 through WO-4)
  { id: "e-capture-ipc", source: "native-capture", target: "ipc-bus", kind: "audio", label: "AudioChunk" },
  { id: "e-ipc-supervisor", source: "ipc-bus", target: "supervisor", kind: "audio", label: "drain" },
  { id: "e-supervisor-kinematics", source: "supervisor", target: "kinematics", kind: "control", label: "turn state" },
  { id: "e-ipc-kinematics", source: "ipc-bus", target: "kinematics", kind: "audio", label: "AudioChunk" },
  { id: "e-kinematics-ipc", source: "kinematics", target: "ipc-bus", kind: "ipc", label: "Blendshape+Kinematic" },
  { id: "e-ipc-transport", source: "ipc-bus", target: "transport", kind: "ipc", label: "drain 60fps" },
  { id: "e-transport-renderer", source: "transport", target: "renderer", kind: "datachannel", label: "MRD1 binary" },
  { id: "e-transport-presence", source: "transport", target: "presence", kind: "control", label: "telemetry" },
];

export const STAGE_CONTRACTS: Record<"L0" | "L1" | "L2", StageContract> = {
  L0: {
    stage: "L0",
    title: "Idle Presence",
    subtitle: "Control plane only — gaze, breath, micro-expression on the 2D portrait",
    controlBudgetMs: 1000,
    hotNodes: ["mic", "presence"],
    warmNodes: [
      "cloud-bridge",
      "native-capture",
      "ipc-bus",
      "supervisor",
      "kinematics",
      "transport",
      "renderer",
    ],
    requiresPixelStream: false,
    requiresBlendshapes: false,
  },
  L1: {
    stage: "L1",
    title: "Live Signal Path",
    subtitle: "Cloud bridge (Pipeline 1) and/or native harness (Pipeline 2) driving ARKit-52 + telemetry",
    controlBudgetMs: 1000,
    hotNodes: [
      "mic",
      "presence",
      "cloud-bridge",
      "native-capture",
      "ipc-bus",
      "supervisor",
      "kinematics",
      "transport",
    ],
    warmNodes: ["renderer"],
    requiresPixelStream: false,
    requiresBlendshapes: true,
  },
  L2: {
    stage: "L2",
    title: "WebGPU Splat Render",
    subtitle: "Full data plane — WGSL Gaussian-splat viewport takeover",
    controlBudgetMs: 1000,
    hotNodes: [
      "mic",
      "presence",
      "cloud-bridge",
      "native-capture",
      "ipc-bus",
      "supervisor",
      "kinematics",
      "transport",
      "renderer",
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
 * Groups Pipeline 1 (mic → cloud-bridge) on one arc and Pipeline 2
 * (native-capture → ... → renderer) on the other, with presence at the
 * hub both arcs feed into — the layout should read as "two pipelines,
 * one presence contract," matching the harness's actual architecture.
 */
const OVAL_ORDER = [
  "mic",
  "cloud-bridge",
  "native-capture",
  "ipc-bus",
  "supervisor",
  "kinematics",
  "transport",
  "renderer",
  "presence",
] as const;

/**
 * Place nodes on an ellipse so the full topology is visible on load
 * (no off-stage nodes from a linear left→right strip).
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

/** Default oval graph layout — all nine Miranda-Engine nodes visible on page load */
export const NODE_POSITIONS: Record<string, { x: number; y: number }> = layoutOval(OVAL_ORDER);
