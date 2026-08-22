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

### T1 — [CAT 1] Add AWS SDK deps and scaffold TypeScript audio files

**Model: Qwen3 Coder Next**

1. In `client-apps/web/package.json`, add:
   ```json
   "@aws-sdk/client-transcribe-streaming": "^3",
   "@aws-sdk/client-bedrock-runtime": "^3"
   ```
2. Run `npm install` from `client-apps/web/` and confirm it exits 0.
3. Create these empty files (scaffold only — T2/T3/T4 will fill them):
   - `client-apps/web/src/audio/MicCapture.ts`
   - `client-apps/web/src/audio/VadDetector.ts`
   - `client-apps/web/src/audio/TranscribeClient.ts`
   - `client-apps/web/src/audio/BedrockRouter.ts`
4. Run `npx tsc --noEmit` from `client-apps/web/` — must exit 0 (empty files are valid TS).

**Evidence required**: `npm install` output (0 errors), `tsc --noEmit` exit 0.

---

### T2 — [CAT 2] Implement browser VAD (energy threshold)

**Model: Amazon Nova Lite**

Implement `client-apps/web/src/audio/VadDetector.ts`. This is a simple RMS energy threshold — no ML model needed for Pipeline 1.

```typescript
// Energy-threshold VAD for Pipeline 1 browser context
export type VadEvent = 'speech-start' | 'speech-end';

export class VadDetector {
  private isSpeaking = false;
  private silenceFrames = 0;
  private readonly THRESHOLD = 0.01;       // RMS energy floor
  private readonly SILENCE_FRAMES = 15;    // 15 × 64ms ≈ 1 second of silence → SpeechEnd

  feed(pcm: Float32Array): VadEvent | null {
    const rms = Math.sqrt(pcm.reduce((s, x) => s + x * x, 0) / pcm.length);
    if (rms > this.THRESHOLD) {
      this.silenceFrames = 0;
      if (!this.isSpeaking) {
        this.isSpeaking = true;
        return 'speech-start';
      }
    } else {
      this.silenceFrames++;
      if (this.isSpeaking && this.silenceFrames >= this.SILENCE_FRAMES) {
        this.isSpeaking = false;
        this.silenceFrames = 0;
        return 'speech-end';
      }
    }
    return null;
  }
}
```

Write one unit test in `client-apps/web/src/audio/VadDetector.test.ts` using Vitest:
- Feed 20 frames of silence (all zeros) → no event
- Feed 5 frames of loud audio (RMS > threshold) → `'speech-start'` on the first frame
- Feed 15 more frames of silence → `'speech-end'`

Run `npx vitest run --reporter=verbose` from `client-apps/web/` — test must pass.

**Evidence required**: Vitest output showing the VadDetector test passing.

---

### T3 — [CAT 3] Implement Transcribe Streaming client

**Model: Amazon Nova Pro**

Implement `client-apps/web/src/audio/TranscribeClient.ts`. This is the Amazon Transcribe Streaming WebSocket integration. Read `.kiro/steering/pipeline-1-aws-native.md` and the `aws-pipeline-architect` Kiro skill before writing this.

Key requirements:
- Credentials: call AMANDA vault MCP `get_key("aws")` — never hardcode keys
- Stream PCM chunks as 2048-sample batches (2 × 1024 from ScriptProcessor = 128 ms per send)
- Handle both `IsPartial: true` (emit `onPartialTranscript`) and `IsPartial: false` (emit `onFinalTranscript`)
- Re-entrancy: if a new `SpeechStart` arrives while a Transcribe stream is active, close the current stream gracefully and open a new one

```typescript
import { TranscribeStreamingClient, StartStreamTranscriptionCommand } from '@aws-sdk/client-transcribe-streaming';

export class TranscribeClient {
  private client: TranscribeStreamingClient;

  constructor(
    private onPartialTranscript: (text: string) => void,
    private onFinalTranscript: (text: string) => void
  ) {
    this.client = new TranscribeStreamingClient({ region: 'us-east-1' });
  }

  async startStream(audioGenerator: AsyncGenerator<Uint8Array>): Promise<void> {
    const command = new StartStreamTranscriptionCommand({
      LanguageCode: 'en-US',
      MediaSampleRateHertz: 16000,
      MediaEncoding: 'pcm',
      AudioStream: (async function* () {
        for await (const chunk of audioGenerator) {
          yield { AudioEvent: { AudioChunk: chunk } };
        }
      })(),
      EnablePartialResultsStabilization: true,
      PartialResultsStability: 'medium',
    });
    const response = await this.client.send(command);
    for await (const event of response.TranscriptResultStream!) {
      const results = event.TranscriptEvent?.Transcript?.Results ?? [];
      for (const result of results) {
        const text = result.Alternatives?.[0]?.Transcript ?? '';
        if (result.IsPartial) this.onPartialTranscript(text);
        else this.onFinalTranscript(text);
      }
    }
  }
}
```

Write an integration test in `TranscribeClient.test.ts` that:
- Mocks the `TranscribeStreamingClient` to return one partial result and one final result
- Confirms `onPartialTranscript` is called once and `onFinalTranscript` is called once

**Evidence required**: `npx vitest run` passing with the mock integration test.

---

### T4 — [CAT 3] Implement Bedrock Converse routing

**Model: Amazon Nova Pro**

Implement `client-apps/web/src/audio/BedrockRouter.ts`. This takes the final transcript from T3 and calls Amazon Bedrock Converse API to generate EVE's response.

Read `aws-pipeline-architect` skill section "Amazon Bedrock Converse API" before writing.

Model ID: `amazon.nova-pro-v1:0`  
Credentials: `get_key("aws")` via AMANDA vault MCP — same pattern as T3.

```typescript
import { BedrockRuntimeClient, ConverseCommand } from '@aws-sdk/client-bedrock-runtime';

const SYSTEM_PROMPT = `You are EVE, a real-time digital human companion.
Respond in 1-2 sentences. Be warm, present, and conversational.
Never describe your own actions. Never say "As an AI."`;

export class BedrockRouter {
  private client = new BedrockRuntimeClient({ region: 'us-east-1' });

  async route(transcript: string): Promise<string> {
    const response = await this.client.send(new ConverseCommand({
      modelId: 'amazon.nova-pro-v1:0',
      messages: [{ role: 'user', content: [{ text: transcript }] }],
      system: [{ text: SYSTEM_PROMPT }],
    }));
    return response.output?.message?.content?.[0]?.text ?? '';
  }
}
```

Wire T2 + T3 + T4 together in `client-apps/web/src/audio/AcousticPipeline.ts`:

```typescript
// Orchestrator: MicCapture → VadDetector → TranscribeClient → BedrockRouter → TurnComplete
export class AcousticPipeline {
  constructor(
    private onTurnComplete: (transcript: string, response: string) => void
  ) {}

  async start(): Promise<void> {
    const vad = new VadDetector();
    const router = new BedrockRouter();
    const transcribe = new TranscribeClient(
      (partial) => console.log('[partial]', partial),  // TODO WO-3: trigger EVE processing expression
      async (final) => {
        const response = await router.route(final);
        this.onTurnComplete(final, response);
      }
    );
    // MicCapture feeds VadDetector; on speech-start, open Transcribe stream
    // ... (mic capture glue goes here — use getUserMedia from design.md)
  }
}
```

**Manual verification required** (no automated test for mic → real AWS → response):
- Open THE VANITY in browser (`npm run dev` from `client-apps/web/`)
- Speak: "Hello EVE, how are you?"
- Confirm: partial transcript appears in console within 1 second of speech start
- Confirm: final transcript appears and BedrockRouter returns a response
- Log timestamps at SpeechStart, FinalTranscript, and TurnComplete — record elapsed times

**Evidence required**: Console log output showing the three timestamps and the response text from Bedrock. Paste the output verbatim (do not summarize).

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
