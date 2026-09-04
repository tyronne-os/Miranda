# New Toy — Miranda Engine Handoff Guide

**Project:** NVIDIA Tokkio + AWS Bedrock/Polly/Transcribe + GCP Qwen 14B real-time digital human pipeline  
**Status:** Alpha (Pipeline 1 — AWS-native services + GCP LLM backend)  
**Last Updated:** Aug 27, 2026  
**Deployed Stack:** GCP berylize-node (g2-standard-4, NVIDIA L4, $0.40/hr)

---

## Quick Start

### Prerequisites

1. **Node.js 18+** — for the web client and AWS SDK calls
2. **Rust 1.70+** — for the IPC harness (optional for testing on CPU)
3. **Docker** — for containerized Tokkio runtime (optional)
4. **AWS Account** with credentials for:
   - Amazon Transcribe Streaming (ASR)
   - Amazon Bedrock (LLM — Claude or Nova Pro)
   - Amazon Polly Neural TTS
5. **GCP Account** with access to berylize-node instance (34.26.220.164)
6. **Hugging Face token** (for model hub access)
7. **NVIDIA NGC API key** (for Tokkio container pull)

### Environment Setup

Clone the repo and set up your `.env` file:

```bash
git clone https://github.com/tyronne-os/new-toy.git
cd new-toy
cp .env.example .env
```

Fill in `.env` with your credentials:

```env
# AWS
AWS_ACCESS_KEY_ID=<your_key>
AWS_SECRET_ACCESS_KEY=<your_secret>
AWS_REGION=us-east-1

# GCP (for LLM backend — Qwen 14B on berylize-node)
GCP_INSTANCE_IP=34.26.220.164
GCP_QWEN_PORT=8000
GCP_VOICE_AGENT_PORT=8081

# NVIDIA
NGC_API_KEY=<your_ngc_key>
NVIDIA_NIM_API_KEY=<your_nim_key>

# Hugging Face
HUGGINGFACE_TOKEN=<your_hf_token>

# GitHub (for versioning)
GITHUB_TOKEN=<your_github_token>
```

### Fire Up the Pipeline

#### 1. Start the Web Client (React + Vite)

```bash
cd client-apps/web
npm install
npm run dev
```

The UI will be live at **http://localhost:5173**

#### 2. Open the Settings Panel

- Click the **⚙️ gear icon** in the top-right corner
- Enter your **API keys** in the Key Vault form
- Verify connectivity with the green/red indicators

#### 3. Start the AWS Transcribe Listener (ASR ingress)

In a new terminal:

```bash
cd client-services
node transcribe-stream-client.mjs
```

This opens a WebSocket to Amazon Transcribe Streaming. Your browser microphone will stream audio → real-time transcript.

#### 4. Start the GCP Qwen LLM Backend (if not already running)

The berylize-node instance (34.26.220.164) should already have llama-server running on port 8000. Verify:

```bash
curl http://34.26.220.164:8000/v1/models
```

You should see the Qwen model listed.

#### 5. Wire Polly TTS + Viseme Mapping

The pipeline automatically:
- Takes the transcript from step 3
- Sends it to GCP Qwen on berylize-node for a response
- Pipes the response to **Amazon Polly Neural TTS** with `SpeechMarkTypes=['viseme']`
- Maps Polly visemes to ARKit blend shape weights
- Pushes the frame stream to the browser canvas

#### 6. Render with Amazon Sumerian Hosts (or Tokkio)

The right-side canvas in THE VANITY UI renders the animated avatar in real-time:
- **Sumerian Hosts** (Three.js) for instant preview — works on CPU
- **Tokkio Docker** (optional) — for full-fidelity NVIDIA rendering on GPU

To use Tokkio Docker (requires NGC API key):

```bash
docker pull nvcr.io/nvidia/tokkio/tokkio:latest
docker run --gpus all \
  -e NGC_API_KEY=$NGC_API_KEY \
  -p 8080:8080 \
  nvcr.io/nvidia/tokkio/tokkio:latest
```

Then point the canvas renderer to `http://localhost:8080/stream` in the UI settings.

---

## Architecture Overview

```
[Browser Microphone]
         ↓
[Amazon Transcribe Streaming] ← Real-time ASR (WebSocket)
         ↓ (WebSocket → audio stream)
[Transcript JSON] → Miranda IPC bus
         ↓
[GCP berylize-node:8000 — Qwen 14B LLM]
         ↓ (LLM response text)
[Amazon Polly Neural TTS + Speech Marks] ← Viseme events (JSON)
         ↓ (Polly viseme → ARKit blend shape weights)
[BlendshapeFrame] → Miranda IPC bus
         ↓
[Amazon Sumerian Hosts / Tokkio Docker] ← Animated avatar renderer
         ↓
[THE VANITY UI Canvas] ← Real-time live video stream
```

### Key Services

| Service | Role | Provider | Cost/Status |
|---------|------|----------|------------|
| **Transcribe Streaming** | ASR (speech-to-text) | AWS | ~$0.02/min |
| **Qwen 14B LLM** | Cognitive routing | GCP g2-standard-4 (L4 GPU) | $0.40/hr |
| **Polly Neural TTS** | Voice synthesis + visemes | AWS | ~$0.02/1k chars |
| **Sumerian Hosts** | Avatar renderer | Browser (Three.js) | Free |
| **IPC Bus (Miranda)** | Real-time sync, latency tracking | Local tmpfs (/dev/shm) | Free |

---


## Phase One Design Specs (Miranda's Brain — Complete)

Phase One of Miranda's cognitive architecture is now fully specced (requirements → design → tasks), covering two new Work Orders in addition to WO-1 through WO-5:

### WO-Memory: Memory Data Lake (`.kiro/specs/wo-memory-data-lake/`)

A bi-directional, local-first memory system built on three coordinated backends:
- **Neo4j** (rootless Podman) — knowledge graph of entities, conversations, mood states, relationships
- **Obsidian vault** — human-readable, bidirectionally-linked markdown notes for browsing history
- **DuckDB** — SQL-queryable event index for fast analytics (mood filtering, entity lookup)
- **Data lake** — immutable JSONL event log, source of truth for all conversation turns

All storage lives under `/mnt/NOBILITY_VAULT/.miranda/`, encrypted at rest, zero cloud transmission. Retrieval is bi-directional: every new user message triggers a graph + index query for relevant past context (entity overlap, temporal recency, mood continuity), which is injected into the LLM system prompt before inference. This is what lets Miranda reference "yesterday's training run" or "the quantization issue we debugged last week" instead of resetting context every session.

14 tasks, CAT 1-3 only (no CAT 4/5 in this spec), routed primarily to Nova Pro.

### WO-Conversational-Intelligence: Adaptive Conversation Layer (`.kiro/specs/wo-conversational-intelligence/`)

Moves Miranda from reactive Q&A to adaptive, anticipatory conversation:
- **Continuous mood tracking** — mood is a live vector updated mid-message, not a per-turn snapshot; drives real-time avatar ARB color transitions
- **Conversation state machine** — Opening/Deep Work/Debugging/Reflection/Casual states with micro-states, controlling response depth and tone
- **Anticipatory move generator** — proactively surfaces next-step suggestions above a 0.7 confidence threshold (CAT 4 — real correctness risk, since a wrong confident prediction is worse than none)
- **Interest model & curiosity engine** — Miranda tracks recurring topics/techniques and surfaces genuine questions about the user's own work (rate-limited to ≤1/hour)
- **Real-time knowledge updates** — corrections, framework mentions, and code style apply forward within the same session
- **Role/persona fluidity** — Research Partner, Rubber Duck, Peer Reviewer, Therapist, Brainstorm Co-Creator, auto-detected from conversational cues
- **Autonomy calibration interview** — Miranda interviews the user on acceptable autonomy per action category (file ops, spending/GPU provisioning, git, install/config); stores thresholds and periodically re-checks as a track record builds
- **Fixed autonomy floor (non-negotiable)** — destructive-at-scale, production-impacting, and high-blast-radius actions always require explicit confirmation regardless of calibration; this holds under every possible interview input
- **Partnership investment tracking** — tracks user goals, surfaces progress unprompted, filtered against a banned-pattern list so acknowledgment never uses dependency or guilt language
- **Mahogany Hall groundwork** — same memory/persona architecture supports sustained role-play and relationship continuity for the companionship project, using the same local-encrypted storage guarantees

11 tasks; only Task 3 (Anticipatory Move Generator) is CAT 4, everything else CAT 2-3.

Both specs pass full format validation (`validate_spec_format`) with zero errors.

## Deployment Checklist

- [ ] `.env` file filled in with all credentials
- [ ] AWS credentials verified with `aws sts get-caller-identity`
- [ ] GCP berylize-node is running and accessible (ping 34.26.220.164)
- [ ] Qwen llama-server responding on port 8000
- [ ] `npm run dev` started in client-apps/web
- [ ] Settings Panel ⚙️ shows all keys as "Connected" (green)
- [ ] Browser microphone working (test in browser console)
- [ ] THE VANITY UI renders canvas on the right

---

## Idle Auto-Stop (Cost Discipline)

The berylize-node instance will **auto-stop after 15 minutes of inactivity** to control GCP costs.

To keep it running during development:

```bash
# From your local machine, ping the instance every 5 minutes
watch -n 300 'curl -s http://34.26.220.164:8000/v1/models > /dev/null && echo "Instance alive"'
```

Or manually restart if stopped:

```bash
gcloud compute instances start berylize-node --zone=us-east1-c --project=posh-eden
```

---

## Troubleshooting

### "Connection refused" on port 8000

- Verify berylize-node is running: `gcloud compute instances list --project=posh-eden`
- Check if llama-server crashed: SSH and run `ps aux | grep llama-server`
- Restart the instance: `gcloud compute instances stop/start berylize-node ...`

### Polly returns empty visemes

- Check AWS Bedrock region is correct (should be us-east-1 or us-west-2)
- Verify Polly model access is enabled in AWS Console → Bedrock → Model access
- Ensure `SpeechMarkTypes: ['viseme']` is set in the Polly request

### Avatar not animating

- Open browser DevTools → Network tab, check if WebSocket to Polly is open
- Verify BlendshapeFrame messages are flowing on the Miranda IPC bus (check `/dev/shm/miranda_bus`)
- Check if Sumerian Hosts is loaded: `curl http://localhost:5173/index.html | grep sumerian`

### "VcpuLimitExceeded" on AWS (GPU quota issue)

- This means your AWS startup account doesn't have GPU quota yet
- For now, deploy on **t3.large (CPU)** and test the pipeline topology
- AWS typically approves GPU quota increases within 24-48 hours; check Support console

---

## Next Steps (Post-Alpha)

1. **Pipeline 1 validation:** Measure end-to-end latency (Transcribe → Bedrock → Polly → render)
2. **Pipeline 2 (Research):** Swap GCP Qwen for **parakeet.cpp** (Riva ASR on-device) + **SIMD kinematics** (52-channel blend shape solver)
3. **Pipeline 3 (GPU rendering):** Replace Sumerian Hosts with **WebGPU Gaussian-splat renderer** (WO-5)
4. **Quad-test:** Run Pipeline 1, 2, 3, and variants in parallel, score each against the **Instant Presence Standard**

---

## Versioning & Updates

Every commit to this repo updates this **HANDOFF.md** with:
- New services added / removed
- IP addresses or port changes
- Credentials rotation (if needed)
- Deployment checklist updates
- Cost tracking changes
- New troubleshooting entries

**Current commit:** `fb34b5f` (Aug 27, 2026)  
**Synced to:**
- GitHub: https://github.com/tyronne-os/new-toy
- Hugging Face (backup): https://huggingface.co/AIBRUH/miranda-engine

---

## Support

- **Slack:** #miranda-engine (Beryl Labs)
- **Issues:** Open a GitHub issue on this repo
- **Email:** contact@beryllabs.com

Happy streaming! 🎬✨

---

## Session Status Update — Sep 4, 2026, 08:40 CDT

### Current Bottleneck

`cargo build -p miranda-nodes` has been running since **07:23 CDT** (started PID 589828) and is still compiling `libduckdb-sys` — the bundled DuckDB C++ source tree gets compiled from scratch on first build (no prebuilt binary), and this is the single largest time cost in the build. As of 08:40 CDT it is actively progressing (verified via live `cc1plus` processes compiling different `.cpp` translation units each check, not stalled).

**Root cause of tonight's slowdowns (non-network, non-security):**
1. `duckdb-sys` compiles the full DuckDB C++ engine from source — this is a known multi-minute cost for a cold build, independent of machine load.
2. Multiple redundant `cargo build` invocations were started in parallel earlier in the session, causing CPU contention across ~9 competing processes. Killed; now a single build process is running.
3. Numerous tool-call aborts ("aborted by user") during the session were client-side cancellations (new messages sent while a command was mid-flight, plus one page reload) — not a security compromise. A full process/listener/extension audit was run and returned clean (see below).

### Security Audit Summary (completed this session)
- Found and removed `listener_watchdog.py` (a defensive allowlist-based rogue-socket killer the user had written earlier, never armed) — deleted per user request.
- Full listening-socket and established-connection review: no unauthorized listeners, no unexpected outbound connections beyond known apps (Opera, Kiro, Claude, Hugging Face CLI).
- Verified `sshd` is not installed on this machine at all.
- CUPS/printing: user requested removal; requires interactive sudo (not completable via agent — provided manual command for user's own terminal).
- Chrome extension concern: no Chrome installed; Opera's 6 extensions are all Opera-signed built-ins (Rich Hints Agent, Continue on Support, Opera AI, 2 themes, 1 localized component) — nothing third-party.
- Playwright/headless Chromium cache traced to the `saoudrizwan.claude-dev` (Cline) VS Code Insiders extension.
- **VS Code Insiders fully removed** (946MB, including Cline) per user request — no installed package existed (portable/already-uninstalled binary), so leftover `~/.vscode-insiders` and `~/.vscode-insiders-shared` config dirs were deleted directly.
- Full process audit: zero processes running from deleted binaries, zero live curl/wget processes, all ~529 running processes traced to verified binary paths (system packages, snap packages, or known user apps). No malware found.

### Outstanding Build Work (blocking Phase One compile)
14 stub `.rs` files were created to unblock the `miranda-nodes` crate build (files declared in `mod.rs` but missing from disk):
- `conversation/`: state_machine.rs, anticipation.rs, interest_model.rs, knowledge_updater.rs, persona_injection.rs, response_tuning.rs, autonomy_calibration.rs, partnership_tracker.rs, prompt_builder.rs
- `forge/`: naming.rs, compatibility.rs, gpu_provisioner.rs, finetune_pipeline.rs, merge_pipeline.rs

These are minimal compiling stubs, not final implementations. Once `cargo build -p miranda-nodes` succeeds, each needs real logic per `wo-conversational-intelligence` and Model Forge design docs, plus the critical non-negotiable invariants:
- `autonomy_calibration.rs`: floor categories (DestructiveAtScale, ProductionImpacting, HighBlastRadius) must never resolve to Autonomous — structural enforcement.
- `partnership_tracker.rs`: banned dependency/guilt-language patterns must be filtered before any acknowledgment surfaces.

### Next Steps
1. Let current `cargo build -p miranda-nodes` finish (no parallel builds, no interruption).
2. Fix any compile errors surfaced.
3. Implement real logic for the 14 stub files.
4. `cargo build --workspace` then `cargo test -p miranda-nodes`.
5. Manual step still needed from user (requires interactive sudo): purge CUPS per command provided earlier in session.


---

## Session Status Update — Sep 4, 2026, 09:57 CDT — BOTTLENECK CLEARED

### Build Status: PASSING

`cargo build -p miranda-nodes` now completes successfully in 14.74s (once duckdb-sys's one-time C++ compile was cached). Verified via fresh `.rlib` artifact timestamp (09:56 CDT) matching build completion.

### What actually happened
1. The `libduckdb-sys` C++ compile (bundled DuckDB source, ~280 object files) was the real time cost — not a hang, not an interruption, just a large one-time native compile. It completed successfully around 09:54 CDT.
2. Once that finished, a real Rust type error surfaced in `miranda-nodes/src/memory/duckdb_writer.rs`: `Vec<String>` was passed directly into DuckDB's `params!` macro for the `entities` and `mood_contexts` list columns, but `duckdb-rs` does not implement `ToSql` for `Vec<String>`.
3. **Fix applied:** wrapped both list values in `duckdb::types::Value::List(vec![Value::Text(...)])`, which does implement `ToSql`. This matches how `duckdb-rs` expects to bind DuckDB's native `VARCHAR[]` list columns.
4. Rebuilt — compiles clean, no errors, no warnings blocking the build.

### Current verified state
- `cargo build -p miranda-nodes`: **PASSING**
- All 14 stub files (conversation/, forge/) + full memory module (obsidian_writer, retriever, prompt_injection, duckdb_writer, neo4j_writer, event_writer, entity_extractor, mood_classifier): **compiling cleanly**
- `duckdb_writer.rs` has a real integration test (`writes_and_queries_real_duckdb_rows`) against a temp DuckDB file with the live schema — not yet executed this session, next step below

### Immediate Next Steps
1. Run `cargo test -p miranda-nodes` to verify the duckdb_writer integration test and any other existing tests actually pass (not just compile).
2. Run `cargo build --workspace` to confirm no other crate in the workspace is broken.
3. Begin replacing the 14 stub files' placeholder logic with real implementations per `wo-conversational-intelligence` and `wo-model-forge` design docs — starting with the two non-negotiable invariant modules: `autonomy_calibration.rs` and `partnership_tracker.rs`.
4. User's manual step still outstanding (needs interactive sudo, cannot be done via agent): CUPS purge commands provided earlier in session.


---

## Session Status Update — Sep 4, 2026, 10:15 CDT — ALL TESTS PASSING

### Verified via real test run: 164 passed, 0 failed, 2 ignored (`cargo test -p miranda-nodes`)

Two real bugs found and fixed after the build cleared:

1. **`duckdb_writer.rs`** — `duckdb-rs` 1.10505.0 does not support binding native `LIST` parameters (confirmed by tracing the panic to `ToSqlConversionFailure("binding List parameters is not yet supported")` in the driver source). Fixed by storing the `entities` and `mood_contexts` columns as JSON-encoded `VARCHAR` instead of DuckDB `VARCHAR[]`, serialized/deserialized with `serde_json`. Schema updated in both the test's inline schema and the real `scripts/duckdb-init.sh`.
2. **`neo4j_writer.rs`** — the `exhausts_retries_against_unreachable_host` test asserted `connect().await.is_err()`, but `neo4rs::Graph::new` pools connections lazily and does not fail at connect time against a closed port. Rewrote the test to actually exercise `write_conversation`'s retry loop (3 retries, 5ms backoff) and assert `Neo4jError::RetriesExhausted { attempts: 3, .. }` — this is the real behavior the module needs to guarantee per design.md's error-handling contract (JSONL log is source of truth if graph writes are exhausted).

Both fixes verified with real `cargo test` output, not code-review confidence.

### Verified test coverage includes
- Memory: mood classifier (latency + accuracy), neo4j writer (schema shape + real retry exhaustion), duckdb writer (real temp-DB round trip)
- Conversation: solver (SIMD blendshape/viseme DSP — 20+ tests covering additivity, attack/release timing, band selectivity, determinism, autonomic-channel isolation), viseme mapping, 60Hz dispatcher cadence
- Real-time verification: `tests/rt_verification.rs` — 60fps acceptance criteria against a real shared-memory bus, velocity clamping on frames read back from shm

### Next Steps
1. `cargo build --workspace` to confirm no other crate broke.
2. Begin real implementations for the 14 stub files (currently placeholders), starting with `autonomy_calibration.rs` and `partnership_tracker.rs` (the two non-negotiable invariant modules).
3. User's outstanding manual step: CUPS purge (needs interactive sudo, commands provided earlier).


---

## Queued After Current To-Do List: WO-News-Digest + Sovereign Action Layer

**User directive:** do not interrupt current progress (14 stub-file real implementations). This is appended to the END of the build queue.

### Scope (plan presented, not yet spec'd — spec to be written when this item is reached)

1. **Discovery Session** — one-time/re-runnable structured interview Miranda conducts to gather user research interests, active projects, source-weighting preferences, and autonomy calibration (reuses existing `autonomy_calibration` module). Output: versioned `UserProfile` in the memory lake.

2. **AI News Digest Loop (3x/day)** — parallel fetchers against:
   - GitHub trending/search API (public)
   - Hugging Face trending models/papers API (public, HF token already vaulted)
   - NVIDIA Developer Blog RSS + NGC catalog updates (public)
   - Medium via per-tag/publication RSS feeds (`medium.com/feed/tag/<topic>`) — legitimate public route, NOT a paywall bypass; explicitly decided not to circumvent Medium's metered paywall
   - YouTube Data API v3 (needs a Google Cloud API key, separate from GPU billing, free tier sufficient)

   Pipeline: fetch → relevance-filter against UserProfile → dedupe vs. last digest → LLM-summarize into 10 ranked items with relevance rationale → store.

   Storage: new DuckDB table `ai_news_digest(digest_id, timestamp, rank, source, title, url, summary, relevance_score)` + Obsidian note per digest (`obsidian/news/YYYY-MM-DD-digest.md`), retrievable via the existing retriever module. This is Miranda's "recent happenings in AI and tech" extended memory section.

3. **Sovereign Computer-Use / Connectivity Layer (`miranda-actions`)** — capability layer for browser control, GitHub/HF/NVIDIA API calls via vaulted credentials, file/build/deploy actions. Every action writes to a visible, queryable `action_log` (non-negotiable, not hidden, regardless of autonomy level). Reuses the existing `autonomy_calibration` floor: DestructiveAtScale / ProductionImpacting / HighBlastRadius categories never auto-resolve to autonomous, including for Miranda's own self-directed actions. Everything below that floor runs with no pre-approval per user's "sovereign" directive.

### Build order when reached
1. Discovery session + UserProfile storage
2. News fetchers, one source at a time, each tested against its live API before moving to the next
3. Summarizer + digest storage + Obsidian note generation
4. 3x/day scheduler in `miranda-supervisor`
5. `miranda-actions` capability layer + action log wired to the autonomy floor

**Status: queued, not started.** Resuming current to-do list (real implementations of the 14 stub files, starting with `autonomy_calibration.rs` and `partnership_tracker.rs`).


---

## Session Status Update — All 9 WO-Conversational-Intelligence modules implemented (real logic, not stubs)

All conversation modules now have real, tested implementations per `.kiro/specs/wo-conversational-intelligence/design.md`:
- `mood_stream.rs` (Task 1) — done earlier this session
- `state_machine.rs` (Task 2) — 5 states, micro-states, cue detection, mood/entity-driven transitions
- `anticipation.rs` (Task 3, CAT 4) — confidence-gated proactive move generation with dismissal feedback
- `interest_model.rs` (Task 4) — topic frequency/sentiment tracking, rate-limited curiosity questions with dismissal deprioritization
- `knowledge_updater.rs` (Task 5) — correction detection, session-fact precedence, code style profiling
- `persona_injection.rs` (Task 6) — 5 roles with explicit-cue + state/mood fallback detection
- `response_tuning.rs` (Task 7) — mood/state-driven latency & depth targets, streaming preference
- `autonomy_calibration.rs` (Task 8) — type-level floor enforcement, interview flow, track-record recheck
- `partnership_tracker.rs` (Task 9) — goal extraction, progress detection, banned-pattern content filter
- `prompt_builder.rs` (Task 10) — full integration, token-budget truncation in the defined priority order

**Verified: 239/239 tests passing (`cargo test -p miranda-nodes`), 0 failed, 2 ignored** (the 2 ignored are the pre-existing live-Neo4j-container integration tests from WO-Memory, unrelated to this work).

Both non-negotiable invariants verified passing:
- `floor_categories_never_resolve_to_autonomous_under_any_interview_input`
- `banned_pattern_corpus_is_rejected_at_100_percent`

Two real bugs found and fixed during implementation (not just typos — logic errors caught by the tests doing their job):
1. `partnership_tracker::extract_goal` didn't strip a leading "to " after the "my goal is " stem, producing malformed descriptions like "to ship pipeline 1..." — fixed.
2. `anticipation`'s Reflection-state candidate had a base confidence (0.65) that could never clear the 0.7 gate, meaning Reflection would never surface a move at all regardless of mood — raised to 0.72 so a genuinely high-value moment ("capture what we learned") isn't permanently suppressed.

### Task 11 remaining (integration tests & benchmarks)
Not yet done: `miranda-nodes/tests/conversation_integration_tests.rs`, `scripts/conversation-intelligence-benchmarks.sh`, `CONVERSATIONAL_INTELLIGENCE.md`. Unit-level coverage is complete and real; this is the cross-module integration + measured latency layer per design.md's testing strategy.

### Next
- Task 11 (integration tests + benchmarks + report) for WO-Conversational-Intelligence
- Then: WO-Model-Forge task implementations (naming, compatibility, gpu_provisioner, finetune_pipeline, merge_pipeline — currently still placeholder stubs)
- Then: queued WO-News-Digest + sovereign action layer (per plan appended earlier this session)


---

## MILESTONE: All 14 originally-missing stub files now real, tested implementations

All 14 files that were blocking `cargo build -p miranda-nodes` at the start of this session (9 conversation/, 5 forge/) now have genuine logic per their design docs, not placeholders:

**Model Forge (`.kiro/specs/wo-model-forge/`):**
- `naming.rs` (Task 3) — deterministic name generation with pool-based collision resolution (Property 4)
- `compatibility.rs` (Task 4) — pre-merge architecture/tokenizer validation (Property 2, gate 1 of 2)
- `gpu_provisioner.rs` (Task 5) — spending-threshold confirmation gate (Property 1) + 15-min idle teardown (Property 3)
- `finetune_pipeline.rs` (Task 6) — LoRA training command construction, divergence detection, metrics parsing
- `merge_pipeline.rs` (Task 7) — mergekit command construction, typed `ValidatedModels` enforcing compatibility-before-merge, coherence smoke test (Property 2, gate 2 of 2)

**Disclosed scope limitation (per build-standards "no simulated inference" rule):** `finetune_pipeline.rs` and `merge_pipeline.rs` do not fake an actual LoRA training run or mergekit invocation — this environment has no live GPU/mergekit/peft installation. What's real and tested: subprocess command construction, divergence/coherence detection logic, and result parsing — all correctness-critical and independently verifiable without live infra. The actual `Command::spawn()` call is an injected closure seam, real once a GPU-provisioned deployment exists.

**Verified: 279/279 tests passing (`cargo test -p miranda-nodes`), 0 failed, 2 ignored.**

### What remains for full spec completion (not blocking, tracked for later)
- WO-Conversational-Intelligence Task 11: integration tests + benchmarks + `CONVERSATIONAL_INTELLIGENCE.md`
- WO-Model-Forge Task 8+ (job orchestration wiring, Task 9 scope-boundary conversational handling — check tasks.md for exact remaining task numbers)
- Then: queued WO-News-Digest + sovereign action layer

