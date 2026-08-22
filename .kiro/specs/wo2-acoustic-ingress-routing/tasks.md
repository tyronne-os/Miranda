# WO-2 Tasks — Acoustic Ingress, VAD & Supervisory Routing

## Pre-flight before touching a single task

**You are Kiro, the agent building this.** Read the full pre-flight in `requirements.md` before starting any task in this file. Every task below assumes you have already done these three things:

1. `cargo test -p miranda-ipc` — passes clean on this machine. If it fails, stop and fix WO-1 before proceeding. WO-2 writes into WO-1's ring buffers. A broken bus poisons every WO-2 integration test.
2. Read `.kiro/steering/pipeline-1-aws-native.md` — you need to know exactly which AWS service fills the ASR role in Pipeline 1 (Amazon Transcribe Streaming) versus Pipeline 2 (parakeet.cpp).
3. Run `node scripts/cat-router-check.mjs` — confirms which CAT tiers have pending tasks and whether you'll need Claude Sonnet 5 or Opus 5 this session.

---

## Clarifications embedded — Kiro does not need to ask about these

**Q: Is Hermes running? Should I route these tasks through Hermes?**
A: No. Hermes is not integrated, not running. These Work Orders were written in a Hermes-style prompt format, but Kiro is the agent executing them. Do not wait for or reference Hermes.

**Q: Pipeline 1 uses parakeet.cpp?**
A: No. Pipeline 1 uses Amazon Transcribe Streaming for ASR. parakeet.cpp is Pipeline 2. The node label in THE VANITY says "Riva ASR" — that is a role label. Transcribe Streaming fills that role slot in Pipeline 1. parakeet.cpp fills it in Pipeline 2. Do not install or use parakeet.cpp for Pipeline 1 tasks.

**Q: Which Amazon Bedrock model ID should I use for Pipeline 1 routing?**
A: `amazon.nova-pro-v1:0` — CAT 3, Bedrock-native, full tool-calling. Only escalate to `anthropic.claude-sonnet-5` if Nova Pro fails after two real verified attempts on a specific turn-taking edge case.

**Q: Single session or multi-session for this Work Order?**
A: Multi-session. Pipeline 1 tasks (T1–T4) are one session. Pipeline 2 tasks (T5–T9) are a second session that starts after Pipeline 1 is verified complete. Before closing each session, emit the CAT handoff block for the next tier change. Re-read the tasks.md pre-flight at the start of the second session.

**Q: What do I do when I hit the CAT 4 parakeet.cpp FFI task?**
A: Stop on the CAT 3 model, emit the handoff block at the bottom of this file, switch to Claude Sonnet 5, then re-issue T7 alone on the Sonnet 5 session. Do not attempt T7 on Nova Pro.

---

## CAT routing summary for WO-2

| Task | CAT | Model | Why |
|---|---|---|---|
| T1 | CAT 1 | Qwen3 Coder Next | Add npm deps and scaffold TypeScript files — pure mechanical boilerplate |
| T2 | CAT 2 | Amazon Nova Lite | Simple energy-threshold VAD math, known pattern, no risk |
| T3 | CAT 3 | Amazon Nova Pro | Transcribe Streaming WebSocket integration — third-party SDK, behavioral tests |
| T4 | CAT 3 | Amazon Nova Pro | Bedrock Converse routing — SDK call, state dispatch, same tier as T3 |
| T5 | CAT 2 | Amazon Nova Lite | cpal mic capture scaffold — known Rust audio pattern |
| T6 | CAT 2 | Amazon Nova Lite | Silero VAD ONNX inference — standard ort pipeline |
| T7 | CAT 4 | Claude Sonnet 5 | parakeet.cpp cxx FFI — unsafe Rust ABI, cross-language memory contract, real failure risk |
| T8 | CAT 3 | Amazon Nova Pro | Nemotron-Flash state machine — complex but bounded logic, NIM API |
| T9 | CAT 1 | Qwen3 Coder Next | Latency measurement — print elapsed_us, no logic |

---

## SESSION 1 — Pipeline 1 (AWS-native, do first)

### Architectural note for Session 1 — read before touching T1

**The `ace-controller` Node.js service is the correct home for all Pipeline 1 AWS SDK calls.**

Browsers have no filesystem or mmap access — they cannot read `/dev/shm/miranda_bus`. The service that must own the Transcribe Streaming WebSocket and the Bedrock Converse call is the existing Node.js backend at `client-services/ace-controller/run.mjs`, already running on `http://127.0.0.1:8100` with a WebSocket at `/ws`.

The correct Pipeline 1 data flow is:

```
Browser (client-apps/web/)
  └─ getUserMedia() → Float32Array PCM chunks → VAD → on SpeechStart:
     WebSocket message to ace-controller: { type: "audio-chunk", pcm: [...] }

ace-controller (client-services/ace-controller/)
  └─ receives PCM chunks over /ws → accumulates → streams to Amazon Transcribe Streaming
     └─ partial transcript → broadcasts { type: "partial-transcript", text } back over /ws to browser
     └─ final transcript → calls Amazon Bedrock Converse API
        └─ response → broadcasts { type: "turn-complete", transcript, reply } over /ws to browser
```

**Existing infrastructure in ace-controller to preserve and extend (do NOT rewrite):**
- `run.mjs` — the WebSocket server at `/ws` (already handles `{ type: "talk" }` for text-in path)
- `/v1/talk` HTTP endpoint — still used for direct text-in (keep it)
- `nvidiaChatMessages()` / `nvidiaChat()` — the existing NVIDIA Nemotron path (keep it, Pipeline 2 may use it)
- `eveSystemPrompt()`, `PERSONA`, `REALNESS` — persona system (keep, do not modify)
- The `/ws` message handler already has `if (msg.type === "talk")` — extend it with `if (msg.type === "audio-chunk")`, do not replace the existing block

T1 adds AWS SDK deps to `ace-controller`. T2 adds VAD to the browser. T3 adds the PCM→Transcribe path to ace-controller. T4 wires Bedrock into ace-controller's transcript handler.

---

### T1 — [CAT 1] Add AWS SDK deps to ace-controller

**Model: Qwen3 Coder Next**

1. In `client-services/ace-controller/package.json`, add:
   ```json
   {
     "dependencies": {
       "@aws-sdk/client-transcribe-streaming": "^3",
       "@aws-sdk/client-bedrock-runtime": "^3"
     }
   }
   ```
2. Run `npm install` from `client-services/ace-controller/` — exits 0.
3. Create these empty files (scaffold only — T3/T4 will fill them):
   - `client-services/ace-controller/transcribe-bridge.mjs`
   - `client-services/ace-controller/bedrock-router.mjs`
4. In `client-apps/web/package.json`, verify there is NO `@aws-sdk` entry — the browser app does not get AWS SDKs. If any were added accidentally, remove them and run `npm install` again.

**Evidence required**: `npm install` output from ace-controller (0 errors). Show `cat client-services/ace-controller/package.json` confirming both deps present.

---

### T2 — [CAT 2] Implement browser VAD (energy threshold) + PCM WebSocket sender

**Model: Amazon Nova Lite**

The browser's job in Pipeline 1: capture mic audio, detect speech, send raw PCM chunks to ace-controller. Nothing else.

Create `client-apps/web/src/audio/VadSender.ts`:

```typescript
// Browser Pipeline 1 acoustic layer:
// getUserMedia → VAD → send PCM chunks to ace-controller /ws on SpeechStart
export type VadEvent = 'speech-start' | 'speech-end';

export class VadSender {
  private isSpeaking = false;
  private silenceFrames = 0;
  private ws: WebSocket;
  private readonly THRESHOLD = 0.01;
  private readonly SILENCE_FRAMES = 15; // 15 × ~64ms ≈ 1 s of silence → SpeechEnd

  constructor(aceControllerWsUrl: string) {
    this.ws = new WebSocket(aceControllerWsUrl); // 'ws://127.0.0.1:8100/ws'
  }

  async start(): Promise<void> {
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
      const event = this.feedVad(pcm);
      if (event === 'speech-start') {
        // Signal ace-controller that a new utterance is starting
        this.ws.send(JSON.stringify({ type: 'speech-start' }));
      }
      if (event === 'speech-end') {
        this.ws.send(JSON.stringify({ type: 'speech-end' }));
      }
      if (this.isSpeaking && this.ws.readyState === WebSocket.OPEN) {
        // Send raw PCM as a typed array message during active speech
        this.ws.send(pcm.buffer);
      }
    };
  }

  private feedVad(pcm: Float32Array): VadEvent | null {
    const rms = Math.sqrt(pcm.reduce((s, x) => s + x * x, 0) / pcm.length);
    if (rms > this.THRESHOLD) {
      this.silenceFrames = 0;
      if (!this.isSpeaking) { this.isSpeaking = true; return 'speech-start'; }
    } else {
      this.silenceFrames++;
      if (this.isSpeaking && this.silenceFrames >= this.SILENCE_FRAMES) {
        this.isSpeaking = false; this.silenceFrames = 0; return 'speech-end';
      }
    }
    return null;
  }
}
```

Write one unit test in `client-apps/web/src/audio/VadSender.test.ts` using Vitest (test `feedVad` logic only — no browser API needed for this):
- 20 frames of silence (all zeros) → no event
- 5 frames of loud audio (RMS > THRESHOLD) → `'speech-start'` on first frame
- 15 frames of silence → `'speech-end'`

```bash
npx vitest run --reporter=verbose
```
Must pass.

**Evidence required**: Vitest output showing the VadSender test passing.

---

### T3 — [CAT 3] Add PCM→Transcribe Streaming bridge to ace-controller

**Model: Amazon Nova Pro**

Read `.kiro/steering/pipeline-1-aws-native.md` and the `aws-pipeline-architect` Kiro skill section on Transcribe Streaming before writing any code.

Implement `client-services/ace-controller/transcribe-bridge.mjs`:

```javascript
// Receives raw PCM Float32 from browser WebSocket, streams to Amazon Transcribe Streaming.
// Emits partial and final transcript events back to the caller.
import { TranscribeStreamingClient, StartStreamTranscriptionCommand } from '@aws-sdk/client-transcribe-streaming';

export class TranscribeBridge {
  #client;
  #controller = null; // AbortController for in-flight stream

  constructor() {
    // Credentials from environment — injected by AMANDA vault at startup via aws-setup.sh
    this.#client = new TranscribeStreamingClient({ region: process.env.AWS_REGION || 'us-east-1' });
  }

  // Call on SpeechStart. audioGen is an AsyncGenerator<Uint8Array> of 16kHz PCM chunks.
  async startStream(audioGen, { onPartial, onFinal }) {
    if (this.#controller) this.#controller.abort(); // cancel previous stream on interruption
    this.#controller = new AbortController();

    const command = new StartStreamTranscriptionCommand({
      LanguageCode: 'en-US',
      MediaSampleRateHertz: 16000,
      MediaEncoding: 'pcm',
      EnablePartialResultsStabilization: true,
      PartialResultsStability: 'medium',
      AudioStream: (async function* () {
        for await (const chunk of audioGen) {
          yield { AudioEvent: { AudioChunk: chunk } };
        }
      })(),
    });

    const response = await this.#client.send(command, { abortSignal: this.#controller.signal });
    for await (const event of response.TranscriptResultStream) {
      const results = event.TranscriptEvent?.Transcript?.Results ?? [];
      for (const result of results) {
        const text = result.Alternatives?.[0]?.Transcript ?? '';
        if (result.IsPartial) onPartial(text);
        else onFinal(text);
      }
    }
  }

  abort() { this.#controller?.abort(); }
}
```

Now extend the `/ws` WebSocket handler in `run.mjs` to accept binary PCM frames and route them through the bridge. **Add to the existing `ws.on('message')` handler** — do not replace the existing `type: "talk"` or `type: "ping"` blocks:

```javascript
// In run.mjs → wss.on('connection', (ws) => { ws.on('message', async (raw) => { ...
// ADD these cases AFTER the existing msg.type checks:

// Binary message = raw PCM Float32Array from browser mic during active speech
if (raw instanceof Buffer) {
  const f32 = new Float32Array(raw.buffer, raw.byteOffset, raw.byteLength / 4);
  // push into the async generator feeding TranscribeBridge
  currentAudioPush?.(f32);
  return;
}

if (msg.type === 'speech-start') {
  // New utterance: create a generator/bridge pair and start streaming
  let resolver;
  const chunks = [];
  const gen = (async function* () {
    while (true) {
      if (chunks.length) { yield new Uint8Array(chunks.shift().buffer); }
      else await new Promise(r => { resolver = r; });
    }
  })();
  currentAudioPush = (f32) => { chunks.push(f32); resolver?.(); };
  bridge.startStream(gen, {
    onPartial: (text) => ws.send(JSON.stringify({ type: 'partial-transcript', text })),
    onFinal:   (text) => { ws.send(JSON.stringify({ type: 'final-transcript', text })); handleFinal(text, ws); },
  }).catch(() => {}); // absorb AbortError on interruption
}

if (msg.type === 'speech-end') {
  currentAudioPush = null;
  bridge.abort();
}
```

At the top of `run.mjs` (after existing imports), add:
```javascript
import { TranscribeBridge } from './transcribe-bridge.mjs';
const bridge = new TranscribeBridge();
let currentAudioPush = null; // set when speech is active
```

**Credentials**: ace-controller reads AWS credentials from environment variables set by `aws-setup.sh` (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_REGION`). Do not add `get_key()` calls to ace-controller — the AMANDA vault is for Kiro's build-time context, not ace-controller's runtime.

Restart ace-controller with `node run.mjs` and confirm no import errors. Then:
- Open browser, speak one test sentence
- Confirm ace-controller logs show partial transcript events

**Evidence required**: ace-controller console output showing partial transcript lines during a real speech test.

---

### T4 — [CAT 3] Wire Bedrock Converse into ace-controller's transcript handler

**Model: Amazon Nova Pro**

Implement `client-services/ace-controller/bedrock-router.mjs`:

```javascript
import { BedrockRuntimeClient, ConverseCommand } from '@aws-sdk/client-bedrock-runtime';

const SYSTEM_PROMPT = `You are EVE, a real-time digital human companion built by Beryl AI Labs.
Respond in 1-2 sentences. Be warm, present, and conversational.
Never describe your own actions. Never say "As an AI."`;

export class BedrockRouter {
  #client = new BedrockRuntimeClient({ region: process.env.AWS_REGION || 'us-east-1' });

  async route(transcript) {
    const response = await this.#client.send(new ConverseCommand({
      modelId: process.env.BEDROCK_MODEL_ID || 'amazon.nova-pro-v1:0',
      messages: [{ role: 'user', content: [{ text: transcript }] }],
      system: [{ text: SYSTEM_PROMPT }],
    }));
    return response.output?.message?.content?.[0]?.text ?? '';
  }
}
```

Add the `handleFinal` function to `run.mjs` (referenced in T3's `onFinal` handler):

```javascript
import { BedrockRouter } from './bedrock-router.mjs';
const bedrockRouter = new BedrockRouter();

async function handleFinal(transcript, ws) {
  const t0 = Date.now();
  try {
    const reply = await bedrockRouter.route(transcript);
    const latencyMs = Date.now() - t0;
    console.log(`[pipeline-1] transcript="${transcript}" reply="${reply}" latency=${latencyMs}ms`);
    ws.send(JSON.stringify({ type: 'turn-complete', transcript, reply, latencyMs }));
    // Also broadcast visemes for EVE's mouth (reuse existing phoneme-direct path)
    const { frames, durationMs } = phonemeTimeline(reply);
    broadcast({ type: 'visemes', source: 'pipeline-1', durationMs, frames });
  } catch (err) {
    console.error('[pipeline-1] bedrock error', err?.message);
    ws.send(JSON.stringify({ type: 'turn-complete-error', transcript, error: err?.message }));
  }
}
```

**Manual end-to-end verification** (required before marking T4 done):
1. Start ace-controller: `node run.mjs` from `client-services/ace-controller/`
2. Start THE VANITY: `npm run dev` from `client-apps/web/`
3. Open browser, allow microphone, speak: "Hello EVE, how are you?"
4. Check ace-controller console for this exact log line pattern:
   `[pipeline-1] transcript="Hello EVE, how are you?" reply="..." latency=<N>ms`
5. Record the latency value — this is the Pipeline 1 Bedrock baseline

**Evidence required**: Paste the ace-controller console log line verbatim (transcript + reply + latency). Do not summarize.

---

## SESSION 2 — Pipeline 2 (parakeet.cpp + Nemotron-Flash)

Start a new Kiro session. Re-read this tasks.md pre-flight. Run `cargo test -p miranda-ipc` again to confirm WO-1 is still clean after Session 1 changes.

---

### T5 — [CAT 2] cpal mic capture in `miranda-audio`

**Model: Amazon Nova Lite**

Create `miranda-audio/src/capture.rs` with the `cpal`-based mic capture skeleton from design.md. The cpal callback must:
- Capture 16 kHz mono PCM as `f32`
- Use `BufferSize::Fixed(160)` — 10 ms at 16 kHz
- Write one `AudioChunk` per callback to `audio_bus` via the WO-1 ring API
- Never allocate in the audio callback — no `Vec::push`, no `Box::new`

Add `cpal` to `miranda-audio/Cargo.toml`. Run:

```bash
cargo build -p miranda-audio
```

Must exit 0.

Write a unit test `test_chunk_size` that creates an `AudioChunk::from_pcm` with 160 samples and confirms `chunk.sample_count == 160` and `chunk.sample_rate == 16000`. Run:

```bash
cargo test -p miranda-audio -- test_chunk_size --nocapture
```

Must pass.

**Evidence required**: `cargo build` output (0 errors, 0 warnings), `cargo test` output showing `test_chunk_size ok`.

---

### T6 — [CAT 2] Silero VAD ONNX inference

**Model: Amazon Nova Lite**

Implement `miranda-audio/src/vad.rs` using the Silero VAD ONNX model via the `ort` crate. Before writing any code:
- Check `assets/models/silero_vad.onnx` — if it exists, use it. Do not download again.
- If it does not exist: download from `https://github.com/snakers4/silero-vad/raw/master/files/silero_vad.onnx`, place at `assets/models/silero_vad.onnx`, add to `.gitignore`.

Add `ort = { version = "2", features = ["load-dynamic"] }` to `miranda-audio/Cargo.toml`.

Implement `SileroVad::feed(&mut self, chunk: &[f32]) -> Option<VadEvent>` per the design.md spec. Key correctness requirement: the LSTM hidden state (`h`, `c`) must persist across `feed()` calls — if you reset hidden state per call, VAD accuracy collapses to near zero.

Write unit test `test_vad_silence_no_event`: 
- Feed 100 chunks of zero-value PCM (silence)
- Confirm no `VadEvent::SpeechStart` is ever returned

Write unit test `test_vad_tone_detected`:
- Feed 50 chunks of 1.0 amplitude sine wave (clear speech signal)
- Confirm `VadEvent::SpeechStart` is returned at some point in the first 20 chunks

These tests are pure Rust (no FFI) and must pass under MIRI:
```bash
cargo +nightly miri test -p miranda-audio -- test_vad_silence_no_event test_vad_tone_detected
```

**Evidence required**: `cargo test` passing both vad tests, then `cargo +nightly miri test` output showing both pass under MIRI.

---

### T7 — [CAT 4] parakeet.cpp cxx FFI binding

**Model: Claude Sonnet 5 — STOP before this task and switch models**

Before starting T7, on the Amazon Nova Pro session, emit:

```
=== CAT 4 HANDOFF — switching to Claude Sonnet 5 ===
Task: T7 — parakeet.cpp cxx FFI binding
Status: T5 and T6 complete and verified
State: miranda-audio crate builds clean with cpal mic capture and Silero VAD.
       Both VAD tests pass including under MIRI. audio_bus ring writes confirmed.
       parakeet.cpp binary is at [INSERT PATH FROM llamacpp-huggingface-expert SKILL].
       The C API exposed is: extern "C" const char* transcribe_pcm(const float* buf, size_t len);
Incoming needs to know:
  - Use cxx = "1" in Cargo.toml, not bindgen
  - DO NOT add new symbols to parakeet.cpp — bind the existing C API only
  - The FFI integration test must be wrapped in #[cfg(not(miri))] — MIRI cannot cross the FFI boundary
  - Build flags: -march=native mandatory (no -mavx2 on Celeron N4500)
  - Read llamacpp-huggingface-expert Kiro skill before any FFI code — ISA mismatch section is critical
=== END HANDOFF ===
```

**Then switch to Claude Sonnet 5 and re-issue T7 on that session.**

---

**[Claude Sonnet 5 picks up here]**

Read the `llamacpp-huggingface-expert` Kiro skill fully before writing any code. Specifically:
- ISA mismatch section (Celeron N4500 lacks AVX2 — build parakeet.cpp with `-march=native` only)
- The existing binary path for parakeet.cpp in this repo
- The C API surface (`transcribe_pcm`)

Implement `miranda-audio/src/asr/parakeet_ffi.rs` per the design.md spec:

```rust
#[cxx::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("parakeet.h");
        unsafe fn transcribe_pcm(buf: *const f32, len: usize) -> *const c_char;
    }
}

pub fn transcribe(samples: &[f32]) -> String {
    let raw = unsafe { ffi::transcribe_pcm(samples.as_ptr(), samples.len()) };
    if raw.is_null() { return String::new(); }
    // SAFETY: parakeet.cpp returns a valid null-terminated UTF-8 string
    // valid until the next transcribe_pcm call. We own the data after to_string_lossy.
    unsafe { std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned() }
}
```

Add `build.rs` in `miranda-audio/` to configure the cxx build linking to parakeet.cpp. Run:

```bash
cargo build -p miranda-audio
```

Must exit 0 — this is the primary verification. A cxx FFI build failure is a real verification failure and counts toward CAT 4 escalation.

Write integration test `test_transcribe_sine_wave` wrapped in `#[cfg(not(miri))]`:
- Generate 16000 f32 samples of a 440 Hz sine wave (1 second of audio)
- Call `transcribe(samples)` 
- Confirm the return value is a non-null, non-empty `String` (the specific text doesn't matter — just that the FFI round-trip works without a segfault)

```bash
cargo test -p miranda-audio -- test_transcribe_sine_wave --nocapture
```

**If this test fails twice** (real `cargo build` error or segfault — not uncertainty), emit:

```
CAT 5 ESCALATION — T7 parakeet.cpp FFI has failed real verification twice on Claude Sonnet 5.
Switch to Claude Opus 5 before the third attempt.
```

**Evidence required**: `cargo build -p miranda-audio` exit 0, `cargo test` output showing `test_transcribe_sine_wave ok`.

---

### T8 — [CAT 3] Nemotron-Flash turn-taking state machine

**Model: Amazon Nova Pro**

Before starting T8, switch back from Claude Sonnet 5 to Amazon Nova Pro. Emit a brief handoff:

```
=== CAT 3 HANDOFF — switching back to Amazon Nova Pro ===
Task: T8 — Nemotron-Flash state machine in miranda-supervisor
Status: T7 complete — parakeet.cpp FFI builds clean, sine-wave round-trip test passes
State: miranda-audio builds with cpal + Silero VAD + cxx FFI. Next: miranda-supervisor routing.
Incoming needs to know:
  - Nemotron-Flash is NOT a Bedrock model — it uses NVIDIA NIM API (AMANDA vault key "nemotron")
  - Read live-avatar-expert Kiro skill for the NIM endpoint URL format
  - State machine must handle: partial transcript, final transcript, and INTERRUPTION case
=== END HANDOFF ===
```

Implement `miranda-supervisor/src/turn_machine.rs` with the four-state machine from design.md:

States: `IDLE → LISTENING → PROCESSING_PARTIAL → PROCESSING_FINAL → IDLE`

Interruption path: `PROCESSING_FINAL --SpeechStart--> LISTENING` (cancel in-flight Nemotron call).

Use `tokio::select!` for cancellation:

```rust
tokio::select! {
    result = nemotron_call => {
        // normal completion
        emit_turn_complete(result);
        self.state = State::Idle;
    }
    _ = cancellation_rx.recv() => {
        // interrupted — discard nemotron result, return to listening
        self.state = State::Listening;
    }
}
```

Read the `live-avatar-expert` skill for the Nemotron-Flash NIM API endpoint and request format. Credentials via `get_key("nemotron")` from AMANDA vault MCP.

Write unit tests for the three behavioral cases (mock the Nemotron call):
- `test_normal_turn`: partial → final → TurnComplete emitted
- `test_interruption`: start turn → SpeechStart interrupts mid-Nemotron → second TurnComplete (not first) emitted
- `test_silence_no_dispatch`: silence frames → no TurnComplete

```bash
cargo test -p miranda-supervisor -- --nocapture
```

All three tests must pass.

**Evidence required**: `cargo test -p miranda-supervisor` output showing all three tests passing.

---

### T9 — [CAT 1] Latency measurement

**Model: Qwen3 Coder Next**

Add a benchmark integration test `miranda-audio/tests/bench_asr.rs` that measures wall-clock latency from PCM audio write to transcript return via parakeet.cpp FFI:

```rust
#[cfg(not(miri))]
#[test]
fn bench_asr_latency() {
    // Generate 3 seconds of test audio (sine wave at 440 Hz)
    let samples: Vec<f32> = (0..48000)
        .map(|i| (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 16000.0).sin() as f32)
        .collect();

    let start = std::time::Instant::now();
    let transcript = miranda_audio::asr::parakeet_ffi::transcribe(&samples);
    let elapsed_us = start.elapsed().as_micros();

    println!("parakeet.cpp latency: {elapsed_us} μs");
    println!("transcript: {transcript:?}");
    // No assert on latency — just measure and print. The Pipeline 1 baseline from T4 is the comparison point.
    assert!(!transcript.is_empty(), "parakeet.cpp returned empty transcript for sine wave input");
}
```

Run:

```bash
cargo test -p miranda-audio --test bench_asr -- --nocapture
```

Record the printed `latency` value. This is the Pipeline 2 ASR baseline. Compare to the Pipeline 1 Transcribe Streaming latency recorded in T4. Document both numbers in a comment at the top of `bench_asr.rs`.

**Evidence required**: `cargo test` output with the two latency numbers visible in console output.

---

## WO-2 done — what this unlocks

When all 9 tasks have real command-output evidence:
- Pipeline 1 path: browser mic → Transcribe → Bedrock → TurnComplete ✓
- Pipeline 2 path: cpal → Silero VAD → parakeet.cpp → Nemotron-Flash → TurnComplete ✓
- `audio_bus` ring in `/dev/shm/miranda_bus` receiving live AudioChunks ✓
- Latency baseline measured for both paths ✓

WO-3 (ARKit-52 SIMD kinematics — BlendshapeFrame generation) can now start. The `audio_bus` WO-3 reads from is live, and the `TurnComplete` signal WO-3 waits for is being emitted.

Run `node scripts/cat-router-check.mjs` — WO-2 tasks should show all `[x]` complete. If any remain open, do not mark WO-2 done.
