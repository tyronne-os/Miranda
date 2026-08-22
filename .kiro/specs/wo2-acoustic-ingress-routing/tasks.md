# WO-2 Tasks — Acoustic Ingress, VAD & Supervisory Routing

## Pre-flight before touching a single task

**You are Kiro, the agent building this.** Read the full pre-flight in `requirements.md` before starting any task in this file. Every task below assumes you have already done these three things:

1. `cargo test -p miranda-ipc` — passes clean on this machine. If it fails, stop and fix WO-1 before proceeding. Pipeline 2 (T5–T7 below) writes into WO-1's ring buffers — a broken bus poisons every Pipeline 2 integration test. Pipeline 1 (T1–T4) does **not** touch the ring buffer at all (see below) so it has no hard WO-1 runtime dependency, but WO-1 must still build clean as the workspace foundation.
2. Read `.kiro/steering/pipeline-1-aws-native.md` — you need to know exactly which AWS service fills the ASR role in Pipeline 1 (Amazon Transcribe Streaming) versus Pipeline 2 (parakeet.cpp).
3. Run `node scripts/cat-router-check.mjs` — confirms which CAT tiers have pending tasks and whether you'll need Claude Sonnet 5 or Opus 5 this session.

---

## Architectural resolution #1 — headless hardware constraint (Pipeline 1 only)

**Pipeline 1's microphone lives at the browser edge. Pipeline 2's microphone is native `cpal` in `miranda-audio`. These are two deliberately different ingress paths — that comparison is the point of having two pipelines.**

Miranda-Engine's AWS-deployed compute (an EC2 `t3.small`/`t4g.small` instance) is a **headless server** — no sound card, no ALSA/CoreAudio/WASAPI backend. Calling `cpal::default_input_device()` on such a host panics, because the audio subsystem `cpal` expects does not exist there. This means:

- **Pipeline 1 (AWS-native, do first)**: the browser is the only place a real microphone exists in this deployment target. It captures via `getUserMedia()`, runs VAD, and sends audio to `ace-controller` over a WebSocket. Pipeline 1 stays entirely inside `ace-controller`'s own Node.js process memory — see resolution #2 below for why it never touches `/dev/shm`.
- **Pipeline 2 (parakeet.cpp + Nemotron-Flash, do second)**: targets **local/bare-metal** deployment, where a real sound card exists. `cpal` in `miranda-audio` captures the mic directly and writes `AudioChunk`s into WO-1's `audio_bus` ring — this is the Work Order's actual "universal ingress for local hardware" path. **`cpal` is not removed from WO-2. It is Pipeline 2's mic capture, same as it always was; only Pipeline 1 was corrected to browser-edge capture.**
- The two pipelines produce two independently measured ASR latency baselines (Transcribe Streaming vs. parakeet.cpp) from two different, deliberately real ingress paths (cloud/browser vs. local/native). That comparison is lost if Pipeline 2 is also forced onto browser capture — do not collapse the two pipelines into one ingress path.

---

## Architectural resolution #2 — Pipeline 1 stays in-process, no `/dev/shm` bridge

**`/dev/shm/miranda_bus` is WO-1's contract for the Rust-native Pipeline 2 path. Pipeline 1 never needs to write into it.**

`ace-controller` already holds conversational/session state in its own process memory for the existing `/v1/talk` path (see `eveChat`, `nodeChats`, `state` in `run.mjs`). Pipeline 1's transcript and turn state follow the exact same pattern: an in-memory object per active session, not a write into the POSIX ring buffer. There is no requirement anywhere in WO-1 or WO-2's actual goal that Pipeline 1 audio become a `miranda-ipc` `AudioChunk` — that requirement was a mistaken addition and is removed. Building an N-API/Rust-addon or UDP-relay bridge from Node into `/dev/shm` for Pipeline 1 would be real, unnecessary engineering effort solving a problem this Work Order doesn't have.

If a future Work Order wants Pipeline 1's audio telemetry visible on the same bus WO-3/4/5 read from, that is a deliberate, separately-scoped decision — not an implicit requirement of "receiving audio in a WebSocket handler."

---

## Clarifications embedded — Kiro does not need to ask about these

**Q: Is Hermes running? Should I route these tasks through Hermes?**
A: No. Hermes is not integrated, not running. These Work Orders were written in a Hermes-style prompt format, but Kiro is the agent executing them. Do not wait for or reference Hermes.

**Q: Pipeline 1 uses parakeet.cpp?**
A: No. Pipeline 1 uses Amazon Transcribe Streaming for ASR. parakeet.cpp is Pipeline 2. The node label in THE VANITY says "Riva ASR" — that is a role label. Transcribe Streaming fills that role slot in Pipeline 1. parakeet.cpp fills it in Pipeline 2. Do not install or use parakeet.cpp for Pipeline 1 tasks.

**Q: Which Amazon Bedrock model ID should I use for Pipeline 1 routing?**
A: `amazon.nova-pro-v1:0` — CAT 3, Bedrock-native, full tool-calling. Only escalate to `anthropic.claude-sonnet-5` if Nova Pro fails after two real verified attempts on a specific turn-taking edge case.

**Q: Does the browser call AWS SDKs directly?**
A: No, never. The browser (`client-apps/web/`) cannot call AWS Transcribe/Bedrock directly — doing so would require pushing SigV4-signed credentials into public-facing client JS, a real security exposure this project explicitly avoids. All AWS SDK calls live server-side, in `client-services/ace-controller/`, authenticated via the AMANDA vault MCP (`get_key("aws")`) or an IAM instance profile once deployed to EC2 — never hardcoded, never shipped to the browser.

**Q: Does Pipeline 1 write into `/dev/shm/miranda_bus`?**
A: No. See "Architectural resolution #2" above. Pipeline 1 keeps transcript/turn state in `ace-controller`'s own process memory, the same pattern already used for `/v1/talk`. Only Pipeline 2 (native `cpal` capture in `miranda-audio`) writes into the WO-1 ring buffer.

**Q: Single session or multi-session for this Work Order?**
A: Multi-session. Pipeline 1 tasks (T1–T4) are one session. Pipeline 2 tasks (T5–T7) are a second session that starts after Pipeline 1 is verified complete. Before closing each session, emit the CAT handoff block for the next tier change. Re-read the tasks.md pre-flight at the start of the second session.

**Q: What do I do when I hit the CAT 4 parakeet.cpp FFI task?**
A: Stop on the CAT 2 model, emit the handoff block in T6 below, switch to Claude Sonnet 5, then re-issue T6 alone on the Sonnet 5 session. Do not attempt T6 on Nova Lite/Nova Pro.

---

## CAT routing summary for WO-2

| Task | CAT | Model | Why |
|---|---|---|---|
| T1 | CAT 1 | Qwen3 Coder Next | Add AWS SDK deps to `ace-controller/package.json` — pure mechanical boilerplate |
| T2 | CAT 2 | Amazon Nova Lite | Browser WebSocket receiver in `ace-controller` — accepts PCM frames, holds session state in process memory |
| T3 | CAT 3 | Amazon Nova Pro | Amazon Transcribe Streaming bridge — reads frames from T2's session buffer, streams to AWS over HTTP/2 |
| T4 | CAT 3 | Amazon Nova Pro | Amazon Bedrock Converse routing — final transcript → response text |
| T5 | CAT 2 | Amazon Nova Lite | `cpal` mic capture in `miranda-audio` — native Pipeline 2 ingress, writes `AudioChunk` into WO-1's `audio_bus` |
| T6 | CAT 4 | Claude Sonnet 5 | parakeet.cpp `cxx` FFI binding — unsafe Rust ABI, cross-language memory contract, real failure risk |
| T7 | CAT 3 | Amazon Nova Pro | Nemotron-Flash turn-taking state machine (NIM API) — complex but bounded logic |

**Total: 7 tasks.** No CAT 5 in this Work Order (parakeet.cpp FFI is the hardest single task, CAT 4).

---

## SESSION 1 — Pipeline 1 (AWS-native, do first)

### T1 — [CAT 1] Add AWS SDK dependencies to `ace-controller`

**Model: Qwen3 Coder Next**

1. In `client-services/ace-controller/package.json`, add to `dependencies`:
   ```json
   "@aws-sdk/client-transcribe-streaming": "^3",
   "@aws-sdk/client-bedrock-runtime": "^3"
   ```
2. Run `npm install` from `client-services/ace-controller/` and confirm it exits 0.
3. Confirm the browser package (`client-apps/web/package.json`) does **not** carry these two SDKs — the browser never calls AWS directly. If a prior task mistakenly added them there, remove them and re-run `npm install` in `client-apps/web/` to confirm the removal resolves clean.

**Evidence required**: `npm install` output (0 errors) from `ace-controller/`, plus confirmation `client-apps/web/package.json` has no AWS SDK entries.

---

### T2 — [CAT 2] Browser WebSocket audio receiver (in-process, no `/dev/shm`)

**Model: Amazon Nova Lite**

Extend `client-services/ace-controller/run.mjs`'s existing `/ws` WebSocket server to accept binary PCM audio frames from the browser mic (see T5's *browser-side* counterpart note below — Pipeline 1's browser capture code is written as part of this task, since it is small and only needed for manual verification, not a separate task).

Requirements:
- Accept a small `getUserMedia()`-based capture in `client-apps/web/` (a minimal addition — Pipeline 1 does not need the full `VadDetector` class; a simple RMS check inline is sufficient, or reuse one if it already exists) that streams 16 kHz mono PCM frames over the existing `/ws` connection as binary WebSocket messages.
- On the `ace-controller` side, accumulate incoming frames per-connection into an **in-memory buffer** (e.g. a `Float32Array[]` on the per-socket session object), the same pattern `run.mjs` already uses for `eveChat`/`nodeChats`. Do **not** write frames into `/dev/shm/miranda_bus` — see Architectural resolution #2 above.
- On a speech-end signal (simple silence-timeout is fine for Pipeline 1), hand the accumulated buffer to T3.

**Evidence required**: a real test — connect a WebSocket test client, send a synthetic PCM buffer, confirm (via a log line or a temporary debug endpoint) that `ace-controller`'s in-memory session buffer received the expected byte/sample count. Paste the real output.

---

### T3 — [CAT 3] Amazon Transcribe Streaming bridge

**Model: Amazon Nova Pro**

Implement the Transcribe bridge inside `client-services/ace-controller/` (new module, e.g. `transcribeBridge.mjs`). This takes the in-memory PCM buffer from T2 and streams it to Amazon Transcribe Streaming over the AWS SDK's HTTP/2 connection.

Key requirements:
- Credentials: call AMANDA vault MCP `get_key("aws")` — never hardcode keys. On EC2, prefer an IAM instance profile once deployed; the vault call is the local-dev path.
- `MediaSampleRateHertz: 16000`, `MediaEncoding: 'pcm'`.
- Handle both `IsPartial: true` (emit a partial-transcript event over `/ws` to the browser, driving EVE's "processing" micro-expression) and `IsPartial: false` (final result — hand off to T4).
- Re-entrancy: a new speech-start while a Transcribe stream is active for that session must close the current stream gracefully and open a new one, not silently interleave two streams.

**Evidence required**: a real integration test — mock or real Transcribe call with a short synthetic audio buffer from T2's in-memory path, confirm both a partial and a final result are observed and logged with timestamps. Paste the real output.

---

### T4 — [CAT 3] Amazon Bedrock Converse routing

**Model: Amazon Nova Pro**

Implement the Bedrock Converse bridge in `client-services/ace-controller/` (e.g. `bedrockRouter.mjs`). Takes the final transcript from T3 and calls Amazon Bedrock's Converse API to generate EVE's response.

Model ID: `amazon.nova-pro-v1:0`. Credentials: `get_key("aws")`, same pattern as T3.

```javascript
import { BedrockRuntimeClient, ConverseCommand } from '@aws-sdk/client-bedrock-runtime';

const SYSTEM_PROMPT = `You are EVE, a real-time digital human companion.
Respond in 1-2 sentences. Be warm, present, and conversational.
Never describe your own actions. Never say "As an AI."`;

export async function routeToConverse(client, transcript) {
  const response = await client.send(new ConverseCommand({
    modelId: 'amazon.nova-pro-v1:0',
    messages: [{ role: 'user', content: [{ text: transcript }] }],
    system: [{ text: SYSTEM_PROMPT }],
  }));
  return response.output?.message?.content?.[0]?.text ?? '';
}
```

Wire T2 (in-memory buffer) → T3 (Transcribe) → T4 (Bedrock) → a `TurnComplete` event broadcast over `/ws` so the browser/THE VANITY sees the response text.

**Manual verification required** (no automated test for a real end-to-end AWS round trip):
- Start `ace-controller` (`npm run dev` from `client-services/ace-controller/`)
- Speak into the mic via the small browser capture added in T2
- Confirm: final transcript triggers a Bedrock call, a response is returned, and a `TurnComplete` event is broadcast
- Log timestamps at speech-end, Transcribe-final, and TurnComplete — record real elapsed times

**Evidence required**: console log output showing the three timestamps and the actual response text from Bedrock. Paste the output verbatim — do not summarize or assert success in prose.

---

## SESSION 2 — Pipeline 2 (parakeet.cpp + Nemotron-Flash, native/local target)

Start a new Kiro session. Re-read this `tasks.md` pre-flight, including both architectural resolutions above. Run `cargo test -p miranda-ipc` again to confirm WO-1 is still clean.

**Pipeline 2 is the native/bare-metal deployment path — a real machine with a real sound card.** It does not reuse Pipeline 1's browser/WebSocket ingress at all; it is a fully independent, Rust-native capture-to-transcript chain, which is what makes the two pipelines a genuine head-to-head comparison.

---

### T5 — [CAT 2] `cpal` mic capture in `miranda-audio`

**Model: Amazon Nova Lite**

Create `miranda-audio/src/capture.rs` with a `cpal`-based mic capture. The cpal callback must:
- Capture 16 kHz mono PCM as `f32`
- Use `BufferSize::Fixed(160)` — 10 ms at 16 kHz, matching WO-1's `AudioChunk.samples` size
- Write one `AudioChunk` per callback directly into `audio_bus` via the WO-1 `MirandaBus::push_audio` API
- Never allocate in the audio callback — no `Vec::push`, no `Box::new`

Add `cpal = "0.15"` to `miranda-audio/Cargo.toml`. Run `cargo build -p miranda-audio` — must exit 0.

Write a unit test `test_chunk_size` confirming an `AudioChunk` built from 160 samples has `frame_count == 160` and `sample_rate == 16000`. Run `cargo test -p miranda-audio -- test_chunk_size --nocapture` — must pass.

**Evidence required**: `cargo build -p miranda-audio` output (0 errors, 0 warnings), `cargo test` output showing `test_chunk_size ok`.

---

### T6 — [CAT 4] parakeet.cpp `cxx` FFI binding — Claude Sonnet 5

⚠️ **Model check: this task requires Claude Sonnet 5. If you are on Amazon Nova Lite (from T5), STOP and switch.**

Emit this handoff before switching:

```
=== CAT 4 HANDOFF — switching to Claude Sonnet 5 ===
Task: T6 — parakeet.cpp cxx FFI binding
Status: T1-T5 complete — Pipeline 1 verified end-to-end (browser → ace-controller → Transcribe → Bedrock);
        Pipeline 2's cpal mic capture (T5) builds clean and writes real AudioChunks into audio_bus.
State: Two independent ingress paths now exist: Pipeline 1 (browser/cloud) and Pipeline 2 (cpal/native).
       Pipeline 1's measured latency is the baseline parakeet.cpp must be compared against.
Incoming needs to know:
  - This FFI binding lives in miranda-audio (Rust), reading from audio_bus that T5 writes into
  - Read the llamacpp-huggingface-expert Kiro skill before any code — ISA mismatch section
    (Celeron N4500 has no AVX2 — build with -march=native only, never -mavx2)
  - Bind the EXISTING parakeet.cpp C API (extern "C" const char* transcribe_pcm(const float*, size_t))
    — do not add new C++ symbols
  - Use cxx = "1", not bindgen
  - The FFI integration test must be #[cfg(not(miri))] — MIRI cannot cross the FFI boundary
=== END HANDOFF ===
```

Implement `miranda-audio/src/asr/parakeet_ffi.rs`:

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
    // SAFETY: parakeet.cpp returns a valid null-terminated UTF-8 string or
    // null on empty audio; valid until the next transcribe_pcm call.
    unsafe { std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned() }
}
```

Run `cargo build -p miranda-audio` — must exit 0. Write `test_transcribe_sine_wave` (`#[cfg(not(miri))]`): 1 second of 440 Hz sine wave in, confirm a non-empty `String` out (no segfault). Run `cargo test -p miranda-audio -- test_transcribe_sine_wave --nocapture`.

**If this fails twice** (real build error or segfault, not uncertainty), emit:
```
CAT 5 ESCALATION — T6 parakeet.cpp FFI has failed real verification twice on Claude Sonnet 5.
Switch to Claude Opus 5 before the third attempt.
```

**Evidence required**: `cargo build -p miranda-audio` exit 0, `cargo test` output showing the sine-wave round trip passing.

---

### T7 — [CAT 3] Nemotron-Flash turn-taking state machine

**Model: Amazon Nova Pro**

Implement `miranda-supervisor/src/turn_machine.rs`. Nemotron-Flash is NOT a Bedrock model — it runs on NVIDIA's NIM API (`https://integrate.api.nvidia.com/v1`), credentials via `get_key("nemotron")` from the AMANDA vault MCP. Read the `live-avatar-expert` Kiro skill for the exact endpoint format.

Four-state machine: `IDLE → LISTENING → PROCESSING_PARTIAL → PROCESSING_FINAL → IDLE`, with an interruption path `PROCESSING_FINAL --SpeechStart--> LISTENING` that cancels the in-flight Nemotron call via `tokio::select!`.

Write three behavioral tests (mock the Nemotron call):
- `test_normal_turn`: partial → final → `TurnComplete` emitted
- `test_interruption`: new `SpeechStart` mid-Nemotron-call → the interrupted call's result is discarded, not emitted
- `test_silence_no_dispatch`: silence frames → no `TurnComplete`

Run `cargo test -p miranda-supervisor -- --nocapture` — all three must pass.

**Evidence required**: `cargo test -p miranda-supervisor` output showing all three tests passing.

---

## WO-2 done — what this unlocks

When all 7 tasks have real command-output evidence:
- Pipeline 1 path: browser mic → `ace-controller` in-process buffer → Transcribe → Bedrock → `TurnComplete` ✓ (no `/dev/shm` involvement)
- Pipeline 2 path: `cpal` (native) → `audio_bus` → parakeet.cpp (via `miranda-audio`) → Nemotron-Flash → `TurnComplete` ✓
- `audio_bus` ring in `/dev/shm/miranda_bus` receiving live `AudioChunk`s from real native mic hardware in Pipeline 2 ✓
- Latency baseline measured for both ASR paths (Transcribe vs. parakeet.cpp), from two genuinely independent ingress implementations ✓

WO-3 (ARKit-52 SIMD kinematics — `BlendshapeFrame` generation) can now start against Pipeline 2's live `audio_bus`.

Run `node scripts/cat-router-check.mjs` — WO-2 tasks should show all `[x]` complete. If any remain open, do not mark WO-2 done.
