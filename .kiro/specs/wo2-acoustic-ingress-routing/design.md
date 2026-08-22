# WO-2: Acoustic Ingress, VAD & Supervisory Routing — Design

**Read requirements.md first.** This document expands the *how*, building on the *what* in requirements.md. If you skipped the pre-flight in requirements.md, go back now.

---

## Where WO-2 sits in Miranda's crate graph

```
miranda-engine (workspace root)
├── miranda-ipc        ← WO-1 ✓ COMPLETE — the ring bus everything writes into
├── miranda-audio      ← WO-2 PIPELINE 2 target: mic capture + VAD + parakeet.cpp FFI
│   └── deps: miranda-ipc (bus writes), cpal (mic), ort (Silero VAD), cxx (FFI)
├── miranda-supervisor ← WO-2 PIPELINE 2 target: Nemotron-Flash turn-taking state machine
│   └── deps: miranda-ipc (bus reads), tokio, reqwest
├── miranda-core       ← not WO-2 — shared types and config, read-only from WO-2
├── miranda-nodes      ← future WO-3/4 — expression + rendering nodes
└── miranda-transport  ← future WO-4 — WebRTC transport
```

**Pipeline 1 does not add Rust crates for audio/ASR.** It uses the browser (`getUserMedia()` + AWS SDK in TypeScript) and makes HTTP/WebSocket calls to AWS-managed services. The `miranda-audio` crate is Pipeline 2 only. The `miranda-supervisor` crate handles routing for both pipelines (Bedrock Converse API in Pipeline 1, Nemotron-Flash in Pipeline 2) because the routing logic is shared — only the model endpoint URL changes.

**Crate dependency direction is locked.** `miranda-audio` → `miranda-ipc` (audio writes to bus). `miranda-supervisor` → `miranda-ipc` (supervisor reads from bus). Neither audio nor supervisor imports from each other — they communicate only through the ring bus. Do not invert these dependencies.

---

## Data flow diagrams

### Pipeline 1 (AWS-native — build first)

```
Browser (client-apps/web/)
    │
    ├─── getUserMedia() ──→ PCM audio stream (16 kHz, mono, f32)
    │
    ├─── VAD (energy threshold or WebRTC VAD wasm) ──→ SpeechStart/SpeechEnd events
    │         │
    │         └─── on SpeechStart → begin sending PCM chunks via WebSocket
    │
    ├─── WebSocket to miranda-supervisor (Axum) ──→ sends AudioChunk structs
    │
[miranda-supervisor receives AudioChunk, forwards to Transcribe]
    │
    ├─── Amazon Transcribe Streaming (WebSocket) ──→ partial + final transcripts
    │         │
    │         ├─── partial result → VadEvent::PartialTranscript → EVE "processing" expression
    │         │
    │         └─── final result → VadEvent::FinalTranscript → Bedrock Converse API call
    │
    ├─── Amazon Bedrock Converse API (amazon.nova-pro-v1:0 default)
    │         │
    │         └─── response → TurnComplete signal → downstream (WO-3 TTS, WO-5 render)
    │
[IPC bus writes — Pipeline 1 uses in-process ring rather than /dev/shm for browser context]
    │
    └─── audio_bus: AudioChunk written for logging/debug telemetry (WO-4 will consume)
```

### Pipeline 2 (parakeet.cpp + Nemotron-Flash)

```
cpal (miranda-audio) ──→ PCM audio stream (16 kHz, mono, f32)
    │
    ├─── Silero VAD (ort ONNX runtime) ──→ SpeechStart/SpeechEnd events
    │         │
    │         ├─── rising edge: write VadEvent::SpeechStart to audio_bus
    │         └─── falling edge: write VadEvent::SpeechEnd to audio_bus
    │
    ├─── on SpeechStart: write AudioChunk stream to audio_bus (WO-1 ring)
    │
    ├─── parakeet.cpp FFI (cxx binding in miranda-audio)
    │         │
    │         ├─── reads AudioChunk from audio_bus ring
    │         ├─── calls transcribe_pcm(buf, len) → token string
    │         └─── writes TranscriptChunk to miranda-ipc channel
    │
    └─── miranda-supervisor
              │
              ├─── reads TranscriptChunk from bus
              ├─── Nemotron-Flash turn-taking: partial → continue, final → dispatch
              └─── on TurnComplete → downstream TTS node (WO-3)
```

---

## Pipeline 1: technical implementation details

### Browser mic capture (`client-apps/web/`)

The existing Vite/React/TS code in `client-apps/web/` is from the `eve-ecc` carry-forward. Extend it, do not rewrite it. The mic capture goes in a new file `client-apps/web/src/audio/MicCapture.ts`:

```typescript
// MediaDevices.getUserMedia with 16 kHz mono PCM — exact params for Transcribe
const stream = await navigator.mediaDevices.getUserMedia({
  audio: { sampleRate: 16000, channelCount: 1, echoCancellation: true }
});
const ctx = new AudioContext({ sampleRate: 16000 });
const source = ctx.createMediaStreamSource(stream);
const processor = ctx.createScriptProcessor(1024, 1, 1);
source.connect(processor);
processor.connect(ctx.destination);
processor.onaudioprocess = (e) => {
  const pcm = e.inputBuffer.getChannelData(0); // Float32Array, 1024 samples
  onAudioChunk(pcm); // write to Transcribe WebSocket
};
```

**Why 1024 samples / 16 kHz = 64 ms per chunk.** Transcribe Streaming recommends 100–250 ms audio chunks. 2× 1024 = 2048 samples = 128 ms — send two processor buffers per Transcribe write, or configure `bufferSize: 2048` directly. Do not send single 64 ms chunks — Transcribe's internal buffer alignment penalizes very small inputs.

### Amazon Transcribe Streaming setup

SDK: `@aws-sdk/client-transcribe-streaming` — this should already be in the workspace from the Pipeline 1 steering doc. If not, add it to `client-apps/web/package.json`.

```typescript
import { TranscribeStreamingClient, StartStreamTranscriptionCommand } from '@aws-sdk/client-transcribe-streaming';

const client = new TranscribeStreamingClient({ region: 'us-east-1' });
const command = new StartStreamTranscriptionCommand({
  LanguageCode: 'en-US',
  MediaSampleRateHertz: 16000,
  MediaEncoding: 'pcm',
  AudioStream: audioGenerator(), // async generator yielding { AudioEvent: { AudioChunk: pcmBuffer } }
  EnablePartialResultsStabilization: true,
  PartialResultsStability: 'medium',
});
const response = await client.send(command);
for await (const event of response.TranscriptResultStream) {
  if (event.TranscriptEvent?.Transcript?.Results) {
    for (const result of event.TranscriptEvent.Transcript.Results) {
      const text = result.Alternatives?.[0]?.Transcript ?? '';
      if (result.IsPartial) onPartialTranscript(text);
      else onFinalTranscript(text);
    }
  }
}
```

**Credentials**: access AMANDA vault MCP with `get_key("aws")` — never hardcode keys, never put them in environment files that get committed. Credentials flow in at runtime through the MCP vault call. See `.kiro/steering/pipeline-1-aws-native.md` for the exact vault call pattern.

**Two-stage partial/final distinction is mandatory** (REQ-4). When `result.IsPartial === true`, emit a `PartialTranscript` event that triggers EVE's "thinking" micro-expression. When `result.IsPartial === false`, that's the stable final result — dispatch to Bedrock.

### Amazon Bedrock Converse API (`miranda-supervisor`)

Model: `amazon.nova-pro-v1:0` (CAT-3 workhorse, Bedrock-native, full tool-calling). Only escalate to `anthropic.claude-sonnet-5` if Nova Pro fails on a specific turn-taking edge case after two attempts.

```typescript
import { BedrockRuntimeClient, ConverseCommand } from '@aws-sdk/client-bedrock-runtime';

const bedrock = new BedrockRuntimeClient({ region: 'us-east-1' });
const response = await bedrock.send(new ConverseCommand({
  modelId: 'amazon.nova-pro-v1:0',
  messages: [{ role: 'user', content: [{ text: finalTranscript }] }],
  system: [{ text: SUPERVISOR_SYSTEM_PROMPT }],
}));
const reply = response.output?.message?.content?.[0]?.text ?? '';
emitTurnComplete(reply);
```

`SUPERVISOR_SYSTEM_PROMPT` is the turn-taking persona prompt. Keep it in a separate constant — it will change over time as Pipeline 2 swaps Nemotron-Flash in. The function signature (`emitTurnComplete(text: string)`) must not change between Pipeline 1 and Pipeline 2.

---

## Pipeline 2: technical implementation details

### `miranda-audio` crate — mic capture with `cpal`

`cpal` is the cross-platform audio I/O library for Rust. It provides a stream callback on a dedicated audio thread. **The audio callback must never block** — the only work allowed inside it is writing to the IPC ring buffer (which is a lock-free atomic write, same as WO-1's `audio_bus` producer side).

```toml
# miranda-audio/Cargo.toml
[dependencies]
miranda-ipc = { path = "../miranda-ipc" }
cpal = "0.15"
ort = { version = "2", features = ["load-dynamic"] }  # Silero VAD ONNX runtime
cxx = "1"                                              # parakeet.cpp FFI
```

```rust
// miranda-audio/src/capture.rs — the cpal callback skeleton
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use miranda_ipc::{AudioBus, AudioChunk};

pub fn start_capture(bus: &'static AudioBus) -> cpal::Stream {
    let host = cpal::default_host();
    let device = host.default_input_device().expect("no mic");
    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(16000),
        buffer_size: cpal::BufferSize::Fixed(160), // 10 ms @ 16 kHz
    };
    device.build_input_stream(
        &config,
        move |data: &[f32], _| {
            let chunk = AudioChunk::from_pcm(data);
            bus.push(chunk); // lock-free ring write — see WO-1 design
        },
        |err| eprintln!("audio stream error: {err}"),
        None,
    ).expect("failed to build input stream")
}
```

**Why 160-sample chunks (10 ms)?** This matches Silero VAD's internal frame size. VAD is computed per-chunk — sending larger chunks delays the VAD rising edge. See REQ-2: 10 ms detection target.

### Silero VAD (ONNX runtime via `ort`)

Silero VAD is a small (2 MB) ONNX model that takes 512 samples (32 ms @ 16 kHz) and returns a speech probability. Run it every 4 chunks (4 × 10 ms = 40 ms frame):

```rust
// miranda-audio/src/vad.rs
use ort::{Environment, Session};

pub struct SileroVad {
    session: Session,
    buffer: Vec<f32>,  // accumulates 512 samples before inference
    h: Vec<f32>,       // hidden state — must persist across frames
    c: Vec<f32>,
}

impl SileroVad {
    pub fn feed(&mut self, chunk: &[f32]) -> Option<VadEvent> {
        self.buffer.extend_from_slice(chunk);
        if self.buffer.len() < 512 { return None; }
        let prob = self.run_inference(&self.buffer[..512]); // → f32 0..1
        self.buffer.drain(..512);
        if prob > 0.5 { Some(VadEvent::SpeechStart) }
        else          { Some(VadEvent::SpeechEnd) }
    }
}
```

The ONNX model file path: check `llamacpp-huggingface-expert` skill for where it's already placed in this repo. Do not download it again. If it is not yet in the repo, download it once from HuggingFace (`snakers4/silero-vad`) and place it in `assets/models/silero_vad.onnx`. Add that path to `.gitignore` if the file exceeds 5 MB.

### parakeet.cpp FFI binding (`cxx`)

**Read the `llamacpp-huggingface-expert` Kiro skill before touching this.** It contains the exact build flags, the ISA mismatch note for Celeron N4500 (no AVX2 — use `-march=native` only, not `-mavx2`), and the existing parakeet.cpp binary path.

The FFI binding wraps a minimal C API. parakeet.cpp already exposes this function — the `cxx` binding should bind the existing API, not add new C++ symbols:

```cpp
// Declared in parakeet.cpp's header (do not add new symbols)
extern "C" const char* transcribe_pcm(const float* buf, size_t len);
```

```rust
// miranda-audio/src/asr/parakeet_ffi.rs
#[cxx::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("parakeet.h");
        fn transcribe_pcm(buf: *const f32, len: usize) -> *const c_char;
    }
}

pub fn transcribe(samples: &[f32]) -> String {
    let raw = unsafe { ffi::transcribe_pcm(samples.as_ptr(), samples.len()) };
    // SAFETY: parakeet.cpp returns a valid null-terminated UTF-8 string
    // or null on empty audio. The pointer is valid until the next transcribe_pcm call.
    if raw.is_null() { return String::new(); }
    unsafe { std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned() }
}
```

**MIRI note:** The FFI call is `unsafe` and MIRI cannot cross the FFI boundary. Wrap the integration test in `#[cfg(not(miri))]` — the same pattern used in WO-1's `test_round_trip_latency`. The unit test for the Silero VAD logic (which is pure Rust) CAN run under MIRI and should.

### `miranda-supervisor` turn-taking state machine (Pipeline 2)

Nemotron-Flash is accessed via AMANDA's vault MCP (`get_key("nemotron")`). It is NOT a Bedrock model — it runs on NVIDIA's NIM API. The URL pattern is `https://integrate.api.nvidia.com/v1` with the Nemotron-Flash model name. See `live-avatar-expert` skill for the exact endpoint pattern.

The state machine has four states:

```
IDLE ──SpeechStart──→ LISTENING
LISTENING ──PartialTranscript──→ PROCESSING_PARTIAL  (Nemotron not called yet)
PROCESSING_PARTIAL ──FinalTranscript──→ PROCESSING_FINAL  (Nemotron called here)
PROCESSING_FINAL ──NemotronResponse──→ IDLE  (TurnComplete emitted, EVE begins response)

LISTENING ──SpeechStart──→ LISTENING  (re-entry while listening: extend the utterance window)
PROCESSING_FINAL ──SpeechStart──→ LISTENING  (interruption: cancel in-flight Nemotron call, restart)
```

The interruption case (fourth transition) is required by acceptance criterion 8. A new `SpeechStart` while Nemotron is mid-call cancels the `tokio` task (via `tokio::select!` on a cancellation signal), resets to LISTENING, and begins accumulating the new utterance. Do not let the stale Nemotron response from the previous turn arrive after the interruption — the user's new input has overridden it.

### Rolling lookahead buffer

The lookahead buffer is a circular slice of the `audio_bus` ring — not a second buffer. Since WO-1's ring holds 64 audio slots and the producer (mic capture) runs ahead of the consumer (parakeet.cpp / Transcribe), the consumer can read `current + 1` and `current + 2` slots ahead by inspecting the ring's write pointer without moving the read pointer. This is safe and lock-free because:

1. The write pointer is an `AtomicUsize` (WO-1 design)
2. Reading ahead of the read pointer but behind the write pointer cannot access uninitialized slots — write always precedes read by the ring's natural invariant

Document this in code as:

```rust
// SAFETY: lookahead reads at ring.write_ptr - 2 through ring.write_ptr - 1.
// These slots were written before we advanced our read_ptr, so they are
// fully initialized and will not be overwritten until the ring wraps (64 slots
// ahead of where we are now). At 16 kHz / 10ms chunks, that is 640 ms of
// lookahead safety margin — far more than the 20 ms we actually use.
```

---

## Struct definitions from WO-1 (read-only — do not modify)

WO-2 **reads from** these structs. They are defined in `miranda-ipc/src/lib.rs`. Do not add fields or change sizes — doing so breaks alignment proofs and invalidates the mmap layout WO-1 built.

```rust
// miranda-ipc/src/lib.rs — these are WO-1 definitions, read them but do not edit
#[repr(C, align(64))]
pub struct AudioChunk {
    pub timestamp_us: u64,     // 8 bytes
    pub sample_count: u32,     // 4 bytes
    pub sample_rate:  u32,     // 4 bytes
    pub samples: [f32; 160],   // 640 bytes
    _pad: [u8; 0],             // align(64) compiler handles padding
}
// Total: 656 bytes per slot, 64 slots in audio_bus → 41,984 bytes

#[repr(C, align(64))]
pub struct BlendshapeFrame {
    pub timestamp_us: u64,
    pub weights: [f32; 52],    // ARKit-52 blend shape coefficients
    _pad: [u8; 4],
}
// Total: 216 bytes per slot, 128 slots in blendshape_bus → 27,648 bytes
```

WO-2 only writes `AudioChunk` to `audio_bus`. `BlendshapeFrame` is written by WO-3 (expression kinematics). WO-2 must not touch `blendshape_bus`.

---

## What Kiro skill to check for each implementation question

| Question | Answer is in this skill |
|---|---|
| Why is the latency target 30 ms? What does the full glass-to-glass budget look like? | `nobility-posh-framework` — Sequence Matrix, ASR slot in the pipeline |
| What's the exact Transcribe Streaming API call pattern? | `aws-pipeline-architect` — Pipeline 1 section, Transcribe Streaming |
| What's the Bedrock Converse API format? Which model ID? | `aws-pipeline-architect` — Bedrock Converse API section |
| What build flags does parakeet.cpp need? Where is the binary? | `llamacpp-huggingface-expert` — ISA mismatch + build flags section |
| What's the Nemotron-Flash NIM API endpoint format? | `live-avatar-expert` — Nemotron section |
| How does the Node Warden monitor per-node latency in the future? | `llamacpp-huggingface-expert` — Node Warden micro-LLM section |
| What does the "Instant Presence Standard" say about VAD rising-edge response? | `nobility-posh-framework` — IPS, Vanguard Innovation #21 |
| Can WO-2 be containerized? | `aws-pipeline-architect` — Podman hybrid placement rule (yes — audio nodes can run in Podman; WO-1 IPC bus cannot) |

---

## Verification command sequence (run in this order)

After all Pipeline 1 tasks are done:
```bash
# 1. WO-1 still passes (must never regress)
cargo test -p miranda-ipc

# 2. Pipeline 1: open THE VANITY in browser, speak a test sentence, confirm transcript appears
# (manual — no automated test for mic→Transcribe→Bedrock round-trip)
```

After all Pipeline 2 tasks are done:
```bash
# 1. WO-1 still passes
cargo test -p miranda-ipc

# 2. miranda-audio compiles with cxx FFI
cargo build -p miranda-audio

# 3. All unit tests pass (pure-Rust tests run under MIRI, FFI tests are cfg(not(miri)) skipped)
cargo test -p miranda-audio -- --nocapture

# 4. MIRI on Silero VAD pure-Rust logic (FFI test excluded by #[cfg(not(miri))])
cargo +nightly miri test -p miranda-audio

# 5. miranda-supervisor compiles and state machine tests pass
cargo test -p miranda-supervisor -- --nocapture

# 6. Latency benchmark: real mic → parakeet.cpp transcript, print elapsed_us
cargo test -p miranda-audio --test bench_asr -- --nocapture
```
