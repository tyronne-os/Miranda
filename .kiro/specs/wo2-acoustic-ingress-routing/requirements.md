# WO-2: Acoustic Ingress, VAD & Supervisory Routing — Requirements

**Role**: Real-Time Audio Systems Specialist.  
**Depends on**: WO-1 complete — the IPC ring buffer at `/dev/shm/miranda_bus` must exist and pass its round-trip test before any WO-2 code runs. Verify with `cargo test -p miranda-ipc` before starting.  
**Target**: ≤30 ms from mic input to transcript token — measured on real audio, not claimed.  
**Pipeline scope**: WO-2 is built in two passes. **Pipeline 1 (do first)** uses Amazon Transcribe Streaming — batteries-included, no GPU, no FFI. **Pipeline 2 (do second)** replaces Transcribe with parakeet.cpp via a Rust FFI binding for lower latency and local operation. The IPC bus, VAD logic, and supervisor routing structure are shared between both.

---

## Read these first — mandatory pre-flight

You are entering a fully pre-loaded Kiro workspace. These documents contain decisions already made. Do not re-derive them from scratch; read them and build on them.

### 1. WO-1 is a prerequisite — verify before starting

Run from the repo root:
```
cargo test -p miranda-ipc
```
If any test fails, do not proceed. WO-2 writes into the ring buffers WO-1 built. A broken bus means every WO-2 integration test is measuring noise, not the audio pipeline.

### 2. Project overview

Read `PROJECT_OVERVIEW.md` at the repo root. It establishes:
- Miranda-Engine as the harness (not a fixed pipeline)
- Pipeline 1 (AWS-native, batteries-included) as the first workflow to test
- EVE as the permanent 2D control reference in THE VANITY's right pane
- The CAT-5 model routing table with all five current models

### 3. Pipeline 1 node mapping for WO-2

Read `.kiro/steering/pipeline-1-aws-native.md` before writing a single line of WO-2 code. For Pipeline 1, the WO-2 acoustic ingress node maps as follows:

| WO-2 component | Pipeline 1 implementation | Pipeline 2 implementation |
|---|---|---|
| Microphone capture | Browser `MediaDevices.getUserMedia()` API in `client-apps/web/` | `cpal` crate in `miranda-audio` |
| VAD (voice activity detection) | Browser WebRTC VAD or simple energy threshold | Silero VAD (ONNX runtime in Rust) |
| ASR (speech → text) | **Amazon Transcribe Streaming** (WebSocket, managed) | **parakeet.cpp** via `cxx` FFI binding |
| Transcript routing | **Amazon Bedrock Converse API** (Nova Pro or Claude Haiku) | Nemotron-Flash in `miranda-supervisor` |
| IPC bus writes | `AudioChunk` to `audio_bus` | `AudioChunk` to `audio_bus` (identical) |

The IPC bus contract (`AudioChunk` struct, `audio_bus` ring) is **identical in both pipelines**. Build Pipeline 1 first to prove the bus integration. Pipeline 2 swaps the implementations without touching the bus or the downstream consumers.

### 4. The four global Kiro skills

All four are globally available at `~/.kiro/skills/`. They answer specific questions that will arise during WO-2:

- **`nobility-posh-framework`** — the full Sequence Matrix performance targets (ASR ≤120 ms in the full pipeline), the Node Warden concept (small local GGUF model monitoring each node's own throughput), the 25 Vanguard Innovations, the Instant Presence Standard. Read this to understand *why* the 30 ms latency target exists in the context of the full glass-to-glass budget.
- **`aws-pipeline-architect`** — the Pipeline 1 AWS service details: Transcribe Streaming API, Bedrock Converse API, Polly viseme output. Includes SDK names, cost model, and the Podman hybrid placement rule (WO-2 can be containerized; WO-1's IPC bus cannot).
- **`llamacpp-huggingface-expert`** — critical for Pipeline 2: the exact parakeet.cpp build flags (`-march=native` is mandatory on Celeron N4500 which lacks AVX2 — see the ISA mismatch section), the GGUF quantization table, and the `llama-server` OpenAI-compatible endpoint format. Also contains the Node Warden micro-LLM guidance for future self-monitoring.
- **`live-avatar-expert`** — the 5-stage universal live avatar pattern; WO-2 implements stage 1 (ingress/acoustic) and stage 2 (understanding/routing). Read the ingress section to understand what the downstream stages (WO-3 expression, WO-5 rendering) expect to receive from this Work Order.

### 5. What EVE needs from WO-2

EVE is the permanent 2D control reference in THE VANITY's right pane. She is scored against the Instant Presence Standard (see `eve-ecc-docs/INSTANT-PRESENCE-STANDARD.md`). WO-2's job in that context is:

- Detect when the human starts speaking (VAD rising edge) → trigger EVE's listening micro-expression (Vanguard Innovation #21)
- Detect when the human finishes speaking (VAD falling edge + turn-taking signal) → trigger EVE's processing state, then dispatch to TTS/motion
- Feed transcript tokens to the cognitive core (Bedrock in Pipeline 1, Nemotron in Pipeline 2) fast enough that EVE's response starts visibly within 150 ms of the human's last word

The rolling lookahead buffer (requirement 6 below) exists specifically because EVE's mouth coarticulation on the *next* phoneme must be pre-computed — without lookahead, the lip sync lags behind the audio by one chunk.

---

## Requirements (EARS notation)

**REQ-1** — WHEN the microphone produces PCM audio  
THE SYSTEM SHALL capture it and write `AudioChunk` structs into the WO-1 `audio_bus` ring buffer without blocking the audio capture thread.

> *Pipeline 1 note: mic capture happens in the browser via `getUserMedia()`, but the browser has no filesystem/mmap access and cannot write to `/dev/shm/miranda_bus` directly. The browser sends raw PCM over the existing WebSocket to `ace-controller` (`client-services/ace-controller/run.mjs`, port 8100), which owns the Transcribe Streaming and Bedrock Converse calls. `AudioChunk` construction and the actual ring-bus write are a Node.js-side (ace-controller) concern in Pipeline 1, not a browser-side one. Pipeline 2 uses `cpal` in `miranda-audio` for native mic capture with direct bus writes. The `AudioChunk` struct and the bus write contract are identical; only the capture/transport side changes.*

**REQ-2** — WHEN speech begins or ends in the incoming audio stream  
THE SYSTEM SHALL detect the transition (voice activity) within 10 ms of the actual transition and emit a typed event (`VadEvent::SpeechStart` or `VadEvent::SpeechEnd`) that downstream consumers can subscribe to.

> *10 ms is not arbitrary — it maps to one `AudioChunk` frame (160 samples at 16 kHz). A VAD that takes 30+ ms to detect speech start has already missed a phoneme, and the lookahead buffer cannot compensate for detection latency.*

**REQ-3** — WHEN a VAD `SpeechStart` event fires  
THE SYSTEM SHALL begin forwarding `AudioChunk`s from the ring buffer to the active ASR engine (Transcribe Streaming in Pipeline 1, parakeet.cpp in Pipeline 2) and continue until `SpeechEnd`.

**REQ-4** — WHEN the ASR engine returns a transcript token or segment  
THE SYSTEM SHALL forward it to the supervisory routing layer (`miranda-supervisor`) for intent parsing and turn-taking evaluation — with no intermediate copy through a non-realtime path (no file, no Python subprocess, no HTTP round-trip between ASR and supervisor in the hot path).

> *Pipeline 1 note: Transcribe Streaming returns partial and final results over the WebSocket connection. Partial results trigger EVE's "processing" micro-expression. Final results trigger the full cognitive core call. This two-stage distinction is architectural — do not collapse it to "wait for final result" only.*

**REQ-5** — WHEN the supervisory routing layer determines the user has finished their turn  
THE SYSTEM SHALL dispatch a `TurnComplete` signal with the full transcript to the downstream TTS node (Amazon Polly in Pipeline 1, Hive TTS stub in Pipeline 2) and simultaneously signal the motion nodes to begin EVE's response posture.

**REQ-6** — WHEN audio is actively streaming to the ASR engine  
THE SYSTEM SHALL maintain a rolling lookahead buffer of at least 2 future `AudioChunk` frames so the expression/rendering layer has upcoming audio context for mouth coarticulation pre-computation.

> *This requirement exists because EVE's lip sync (WO-3, WO-5) must animate the *next* phoneme's blend shape weight before the audio for that phoneme actually plays. Without a lookahead, lip sync always lags by one chunk.*

**REQ-7** — WHEN Pipeline 2 is being built  
THE SYSTEM SHALL reuse the existing native `parakeet.cpp` build already deployed in this project (exact path and build flags documented in the `llamacpp-huggingface-expert` Kiro skill) rather than rebuilding from scratch. The `cxx` FFI binding must bind the C API surface that parakeet.cpp already exposes (`transcribe_pcm(buf: *const f32, len: usize) -> *const char`), not invent a new one.

---

## Acceptance criteria — what "WO-2 done" looks like

WO-2 has two completion levels:

**Pipeline 1 complete (minimum for WO-3 to start):**
1. Real audio from the browser microphone flows through `getUserMedia()` → VAD → Transcribe Streaming → Bedrock Converse → `TurnComplete` signal, with the transcript visible in THE VANITY's node graph.
2. End-to-end latency from speech-end to `TurnComplete` signal is measured and recorded (not the target — the actual number from a real test sentence).
3. The `audio_bus` ring in `/dev/shm/miranda_bus` shows non-zero write activity during speech (`cargo test -p miranda-ipc` still passes after WO-2 code is added).
4. VAD rising/falling edges are logged with timestamps during a real speech test.

**Pipeline 2 complete (full WO-2):**
5. All Pipeline 1 criteria above still pass.
6. `cargo build -p miranda-audio` exits 0 with the `cxx` FFI binding compiled.
7. `cargo test -p miranda-audio` includes a round-trip test: write a 10 ms PCM sine-wave chunk, pass through the FFI to parakeet.cpp, confirm a non-empty token is returned.
8. The Nemotron-Flash turn-taking state machine in `miranda-supervisor` handles at least: mid-sentence partial result, turn-complete final result, and interruption (new `SpeechStart` while Nemotron is still processing the previous turn).
9. Real end-to-end latency (mic → transcript via parakeet.cpp) is measured and compared against Pipeline 1's Transcribe Streaming baseline.
