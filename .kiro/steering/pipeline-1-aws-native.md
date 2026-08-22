# Pipeline 1: AWS-Native Digital Human — Miranda's First Live Test Workflow

## Why this is Pipeline 1

Miranda-Engine is a workflow-agnostic harness — she can run any pipeline that speaks her node contracts. The first pipeline we drop in must be **batteries-included**: code already exists, models are already accessible, and nothing requires a custom GPU training run before first light. The AWS-native digital human stack meets all three criteria. Every service listed here is billable-on-demand with no standing infra cost, every SDK is open-source and documented, and Kiro (running on AWS Bedrock credits) already has direct access to every model in the routing chain.

**The strategic reason this is Pipeline 1**: it lets us prove Miranda's IPC bus, node contracts, and THE VANITY GUI work end-to-end with real signals — not mocked data, not hardcoded frames — before we introduce research-grade components (GaussianAvatars, parakeet.cpp, SIMD kinematics) in later pipelines. Pipeline 1 is the integration test for the harness, not the final product.

---

## The node mapping — Pipeline 1 vs. the full topology labels

The node graph in THE VANITY (left pane, `client-apps/web/`) shows labels like "Riva ASR," "Nemotron Agent," "Hive TTS," "Audio2Face-3D," "Omniverse Stream." Those are **role labels** — they describe the class of work at each node position. For Pipeline 1, each role maps to an AWS-managed service:

| Node label (role) | Pipeline 1 implementation | SDK / Access |
|---|---|---|
| **Riva ASR** → Acoustic Ingress | **Amazon Transcribe Streaming** | AWS SDK for JavaScript v3: `@aws-sdk/client-transcribe-streaming` |
| **Nemotron Agent** → Cognitive Core / Routing | **Amazon Bedrock** (Claude Sonnet 5 or Amazon Nova Pro) | AWS SDK: `@aws-sdk/client-bedrock-runtime` |
| **Hive TTS** → Vocal Synthesis | **Amazon Polly Neural TTS** | AWS SDK: `@aws-sdk/client-polly` |
| **Audio2Face-3D** → Viseme / Blendshape Source | **Amazon Polly Speech Marks** (viseme output) | Same Polly SDK, `SpeechMarkTypes: ['viseme']` |
| **AnimGraph** → Character Animation Rig | **Amazon Sumerian Hosts SDK** (Three.js / Babylon.js) | `amazon-sumerian-hosts` npm package, open-source: `aws-samples/amazon-sumerian-hosts` |
| **Omniverse Stream** → Transport / Delivery | **Amazon Kinesis Video Streams WebRTC** or **Amazon IVS** | `amazon-kinesis-video-streams-webrtc` npm package |

---

## What "batteries included" means concretely

### Amazon Transcribe Streaming
- Real-time WebSocket-based STT — audio in, transcript out, word-by-word as you speak
- No model to deploy, no GPU needed
- SDK: `TranscribeStreamingClient` with `StartStreamTranscriptionCommand`
- Output JSON: `{ Transcript: { Results: [{ Alternatives: [{ Transcript: "what you said", Items: [...] }] }] } }`
- Replaces parakeet.cpp for Pipeline 1 — same function (audio → text), managed instead of self-hosted

### Amazon Polly (TTS + Visemes — the key bridge)
- Neural TTS voices: `Joanna`, `Matthew`, `Aria`, `Daniel`, and 60+ others — all billable per character, no standing inference cost
- **Speech Marks API** is the critical feature: when you request synthesis with `SpeechMarkTypes: ['viseme']`, Polly returns a stream of JSON events timed to the audio:
  ```json
  {"time":0,"type":"viseme","value":"sil"}
  {"time":85,"type":"viseme","value":"p"}
  {"time":154,"type":"viseme","value":"@"}
  {"time":220,"type":"viseme","value":"t"}
  ```
- These viseme events map to **BlendshapeFrame weights** on Miranda's IPC bus — this is how Pipeline 1 populates the `blendshape_bus` without Audio2Face-3D or SIMD kinematics
- 22 Polly viseme phoneme types: `p`, `t`, `S`, `T`, `f`, `k`, `i`, `r`, `s`, `u`, `@`, `a`, `e`, `E`, `o`, `O`, `u`, `sil`, and a few others — each maps to a subset of ARKit's 52 blend shape channels

### Polly viseme → BlendshapeFrame mapping for Pipeline 1

The `BlendshapeFrame` struct (52 f32 weights, defined in WO-1) does not need all 52 channels populated in Pipeline 1. Polly provides lip-sync data; the rest of the face (eye blink, brow raise, micro-expressions) stays at 0.0 until WO-3 ships. This is not a limitation — it proves the bus and the renderer work with real time-synchronized data before adding complexity.

Approximate mapping (implement this in the Pipeline 1 node adapter in `miranda-nodes`):

| Polly viseme | ARKit blend shape channels (indices) | Weight |
|---|---|---|
| `sil` | all mouth-related | 0.0 |
| `p`, `b`, `m` | `mouthClose` (#19), `jawOpen` (#0) | 0.7, 0.1 |
| `f`, `v` | `mouthFunnel` (#22), `mouthDimpleLeft` (#32), `mouthDimpleRight` (#33) | 0.5, 0.2, 0.2 |
| `T`, `D` | `tongueOut` (#51), `jawOpen` (#0) | 0.6, 0.3 |
| `s`, `z` | `mouthSmileLeft` (#44), `mouthSmileRight` (#45) | 0.15, 0.15 |
| `k`, `g` | `jawOpen` (#0) | 0.4 |
| `i` | `mouthSmileLeft` (#44), `mouthSmileRight` (#45) | 0.6, 0.6 |
| `r` | `mouthShrugUpper` (#48) | 0.4 |
| `@` | `jawOpen` (#0), `mouthOpen` (#21) | 0.5, 0.4 |
| `a`, `A` | `jawOpen` (#0), `mouthOpen` (#21) | 0.7, 0.6 |
| `e`, `E` | `mouthSmileLeft` (#44), `mouthSmileRight` (#45), `jawOpen` (#0) | 0.3, 0.3, 0.3 |
| `o`, `O` | `mouthFunnel` (#22), `jawOpen` (#0) | 0.6, 0.35 |
| `u` | `mouthFunnel` (#22), `mouthPucker` (#23) | 0.5, 0.5 |

These are starting weights — they can be tuned visually against EVE's reference image once rendering is live.

### Amazon Sumerian Hosts SDK — the animation rig
- Open-source AWS project: https://github.com/aws-samples/amazon-sumerian-hosts
- npm package: `npm install amazon-sumerian-hosts`
- Works with **Three.js** (which `client-apps/web/` already uses or can add) and Babylon.js
- What it provides out of the box:
  - Pre-built character GLB model with a full facial blend shape rig (including all ARKit-equivalent channels)
  - Built-in Polly TTS integration (you give it text, it calls Polly, animates the character automatically)
  - **`LipsyncFeature`** — maps Polly visemes to blend shape weights automatically (we can override with our own Polly viseme → BlendshapeFrame mapping for tighter harness control)
  - `GestureFeature` for hand/body gestures
  - `PointOfInterestFeature` for eye gaze tracking
- For Pipeline 1, use Sumerian Hosts as the renderer in WO-5 **in place of** the custom WebGPU/Gaussian-splat renderer — the Gaussian-splat renderer is the long-term target but requires a GPU training pipeline. Sumerian Hosts gives us a working animated EVE-equivalent **right now, in the browser, on zero GPU**.

### Amazon Bedrock — Cognitive Core
- Invoke from `miranda-nodes` via `@aws-sdk/client-bedrock-runtime`
- Pipeline 1 model: **Amazon Nova Pro** (fastest, cheapest) or **Claude 3.5 Haiku** (best reasoning per dollar for turn-taking)
- Use **Bedrock Converse API** (model-agnostic, works with Nova, Claude, Titan, Llama — no model-specific code)
- This is where Nemotron's role lives — it routes conversation turns and generates the response text that gets sent to Polly

### Amazon Kinesis Video Streams WebRTC — Transport
- npm package: `amazon-kinesis-video-streams-webrtc`
- Manages WebRTC signaling without a hand-rolled signaling server (replaces the custom `webrtc-rs` + Axum signaling for Pipeline 1)
- The Rust transport crate (`miranda-transport`, WO-4) will implement `webrtc-rs` for later pipelines — for Pipeline 1, the Node.js/TypeScript client in `client-apps/web/` handles the WebRTC transport via this SDK directly

---

## How Pipeline 1 flows through Miranda's IPC bus

The full data flow for a single conversation turn in Pipeline 1:

```
[Browser microphone]
    │
    ▼
[Amazon Transcribe Streaming] ── (WebSocket over HTTPS) ──▶ transcript JSON
    │
    ▼ (miranda-audio writes AudioChunk to audio_bus)
[/dev/shm/miranda_bus → audio_bus]
    │
    ▼ (miranda-nodes reads transcript, calls Bedrock)
[Amazon Bedrock Converse API] ── response text ──▶
    │
    ▼ (miranda-nodes calls Polly with SpeechMarkTypes=['viseme','sentence'])
[Amazon Polly Neural TTS]
    │ ├── [audio stream] → played in browser via Web Audio API
    └── [viseme Speech Marks] ──▶ BlendshapeFrame per viseme event
                                     │
                                     ▼
[/dev/shm/miranda_bus → blendshape_bus]
                                     │
                                     ▼
[miranda-nodes reads BlendshapeFrame] ──▶ [Sumerian Hosts LipsyncFeature.setViseme()]
                                               │
                                               ▼
                          [EVE rendered live in THE VANITY's right-side canvas]
```

---

## What Miranda's harness adds on top of a raw AWS call chain

A raw call chain (Transcribe → Bedrock → Polly → browser) takes about 2 hours to wire up with direct SDK calls in a single Node.js script. Miranda adds:

1. **The IPC bus** — decouples each service call into an independent node with its own throughput measurement; the harness can see *exactly* how long each segment takes and where the latency is
2. **Session isolation** — each test run is a fresh Podman container; no state bleeds between pipeline variants
3. **Swap points** — any node can be replaced by pointing the harness at a different implementation (swap Transcribe for parakeet.cpp by changing one node definition; the rest of the pipeline doesn't know or care)
4. **THE VANITY** — live node graph with latency readouts per node, warm-path status, circuit breaker state; you can see exactly what Pipeline 1 looks like while it runs and compare it side-by-side with Pipeline 2 when it exists
5. **Scoring against EVE** — the Instant Presence Standard checker (from `eve-ecc-docs/INSTANT-PRESENCE-STANDARD.md`) is applied to the live render; Pipeline 1's Sumerian Hosts render is scored against the same criteria that Pipeline 2's Gaussian-splat render will be scored against

---

## AWS credentials and access for Pipeline 1

All four services (Transcribe, Polly, Bedrock, Kinesis Video) require AWS credentials. They are:
- Already vaulted in AMANDA's Access panel (or can be added via the Access UI — provider name `aws`)
- Exposed to Kiro via the MCP vault server: `node /home/hunt/Downloads/THECODE/amanda/kiro-vault-mcp.mjs` (registered globally in `~/.kiro/settings/mcp.json` as `amanda-access-vault`)
- Kiro can call `get_key("aws")` to retrieve the AWS access key / secret for SDK initialization — no hardcoding, no manual paste

For Bedrock specifically: the model access must be enabled in the AWS console per region for Nova Pro and any Anthropic models. Verify in the AWS console under Bedrock → Model access before running Pipeline 1 nodes.

---

## Work Order dependencies for Pipeline 1

Pipeline 1 uses the same WO structure but with lighter implementations for each node:

| Work Order | Pipeline 1 implementation | Status |
|---|---|---|
| WO-1 (IPC bus) | **Same** — the ring buffer serves all pipelines | Build now (spec in this repo) |
| WO-2 (ASR / audio) | Amazon Transcribe Streaming instead of parakeet.cpp | Simpler than the full WO-2; no FFI |
| WO-3 (kinematics) | Polly viseme → BlendshapeFrame adapter instead of SIMD solver | Simpler than full WO-3; arithmetic only |
| WO-4 (transport) | KVS WebRTC SDK (JS) instead of webrtc-rs | Simpler; no Rust WebRTC |
| WO-5 (renderer) | Amazon Sumerian Hosts + Three.js instead of WebGPU Gaussian-splat | Working right now; no GPU |

WO-1 is the same for every pipeline — the IPC bus is pipeline-agnostic. Build it once; it serves Pipeline 1, Pipeline 2, and every future variant without change.

---

## The point Miranda is proving with Pipeline 1

Miranda is not a digital human application. Miranda is **the engineering harness that proves any workflow can be brought to life — including a theoretical research paper**. Pipeline 1 is the first proof: take a well-understood AWS stack, drop it into the harness, measure it against EVE and the Instant Presence Standard, then use those measurements as the baseline to beat when Pipeline 2 (with parakeet.cpp, SIMD kinematics, and Gaussian-splat rendering) is tested alongside it in a quad-test session.

The claim Miranda makes is not "our AWS pipeline is the best digital human." The claim is: **Miranda can take the description of any pipeline — from an AWS reference architecture to a theoretical research paper — and engineer it into a running, measured, scored implementation without rebuilding the harness.** Pipeline 1 is the first evidence for that claim.
