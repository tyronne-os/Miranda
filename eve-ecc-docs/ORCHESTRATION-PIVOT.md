# The Orchestration Pivot

**2026-08-09** · Beryl AI Labs · read this before touching the avatar pipeline

---

## 1. What happened tonight

We built a CSS "life layer" on EVE's portrait: parallax depth, an amplitude-modulated
breath curve, hair lag, a masked eyelid sweep, viseme co-articulation. It measured
beautifully — 100% parallax opposition, blink at 14.1bpm against a 14.0 target, hair
trailing 1.06px behind the skull. The ECC-5 Realness Index scored it **96/100**.

Then the director looked at the screen:

> *"It is not alive — this is a blinking portrait."*

He was right.

## 2. The two failures, named

**Failure A — cosmetics mistaken for science.**
Transform math on a flat photograph is theater. Real life from a 2D image comes from
diffusion-based portrait animation driving actual facial geometry, spatially aware,
streamed over WebRTC — the Maxine Audio2Face-2D ("Speech Live Portrait") class of model.
CSS cannot get there. The ceiling was flagged early — *"a procedural warp cannot close an
eyelid"* — and the layer was built anyway.

**Failure B — a high score on the wrong metric.**
This is the worse one. The certification ranked a cosmetic placeholder as near-solved.
Twice the MOTION pillar passed something dead, because it read transform **values**
instead of asking what was actually **painted**. The second time, every depth layer was
animating perfectly *behind an opaque portrait* — 100% correct, 100% invisible.

> **A high score on the wrong metric is worse than no score at all.**
> Any measurement of a placeholder must be labeled as measuring a placeholder.

## 3. The insight that reframes the project

> *"Assembling and connecting, when it comes to building a human avatar, are unrelated."*

We have every component: NIM endpoints, Nemotron, entitlements, keys, a live cortex, a
persona, a phoneme pipeline. **Nothing owns making them behave as one living thing.**

The flow is not accountable and not manageable, because **no one is responsible for her
talking.** That is not a rendering problem. It is an orchestration problem — and it is the
one worth solving, because it is the one nobody else is solving publicly.

## 4. The instrument: Microsoft Agent Framework

Open-source SDK and runtime for building, orchestrating, and governing multi-agent
systems. **GA v1.0 since April 2026.** Python and .NET. Successor to and merger of
Semantic Kernel + AutoGen. Orchestration patterns are stable in both SDKs.

### Patterns, and what each is for

| Pattern | Shape | Use for |
|---|---|---|
| **Magentic** | A dedicated manager coordinates specialists, choosing who acts next from evolving context and task progress | **The supervisor.** Open-ended problems with no predetermined plan. Least hand-wired: give it a goal, a manager, and specialists |
| **Handoff** | Agents declare directed edges; framework injects handoff tools. Mesh topology, no central orchestrator | Node-to-node transfer along the real signal path (ASR → Agent → TTS → A2F) |
| **Group chat** | A manager selects the next speaker from an immutable conversation snapshot; selector can be a function or an agent | Multi-perspective diagnosis — several nodes reasoning about one failure together |
| **Sequential / Concurrent** | Fixed order / parallel fan-out | Deterministic stages where the path is already known |

Key Python surface: `ChatAgent` (tools, context providers, middleware, streaming),
`GroupChatBuilder`, and a speaker-selection function receiving an immutable conversation
state and returning the next participant — or `None` to end the conversation.

### Reference
- [Agent Framework 1.0 GA](https://devblogs.microsoft.com/agent-framework/microsoft-agent-framework-version-1-0/)
- [Workflow orchestrations](https://learn.microsoft.com/en-us/agent-framework/workflows/orchestrations/)
- [Magentic](https://learn.microsoft.com/en-us/agent-framework/workflows/orchestrations/magentic) ·
  [Handoff](https://learn.microsoft.com/en-us/agent-framework/workflows/orchestrations/handoff) ·
  [Group chat](https://learn.microsoft.com/en-us/agent-framework/workflows/orchestrations/group-chat)
- [GroupChatBuilder API](https://learn.microsoft.com/en-us/python/api/agent-framework-core/agent_framework.groupchatbuilder?view=agent-framework-python-latest)
- [AutoGen → Agent Framework migration](https://learn.microsoft.com/en-us/agent-framework/migration-guide/from-autogen/)

## 5. What already exists to build on

Do **not** start from zero. Running today in `services/ace-controller`:

- `POST /v1/nodes/{id}/converse` — every cortex node is already a first-person agent with
  live self-knowledge (its health, latency, stage, peers) and per-node thread memory
- `GET /v1/nodes` · `/v1/nodes/{id}` — Understand-Anything telemetry per node
- `POST /v1/speak` — any node speaks aloud; viseme timeline broadcast on the same clock
- `GET /v1/report` — session telemetry: talks, speaks, converses, errors, per-node counters
- `GET|POST /v1/persona` · `/v1/settings` · `/v1/realness` — the lab bench dials
- `scripts/ecc5-certify.mjs` — scored certification emitting `docs/ECC5-CERTIFICATE.md`

**The gap, precisely:** nodes can talk to a *human*. They cannot talk to *each other*, and
**no supervisor owns the outcome.** That gap is the entire build — far smaller than it
sounds.

## 6. The build

1. **Supervisor (Magentic manager)** — one agent accountable for "she speaks." Owns the
   turn end-to-end, decides which node acts next, and reports why.
2. **Node-to-node conversation (Handoff)** — directed edges mirroring the real signal path,
   so a node can challenge its neighbour: *"your VAD dropped a frame, that's why my
   timeline slipped."*
3. **Diagnosis quorum (Group chat)** — on failure, the implicated nodes convene, with the
   supervisor selecting speakers, and return a root cause rather than an error string.
4. **Bind to Beryl Studio** — the standalone QC lab consumes `/v1/report` and the presence
   audit, runs the certification, and the squad proposes fixes as **pull requests, never
   pushes to main.**

## 7. Standing rules carried into this build

- **Agents propose, humans merge.** PR access only. A bad night must not be able to rewrite
  the repo.
- **The key rule holds.** The `nvapi-` credential is used from the director's machine
  through his agent. A swarm either runs locally under him or gets its own scoped
  credential.
- **Never score a placeholder as if it were the product.** Label scaffolding as scaffolding
  in every report, chart, and certificate.
- **Fault injection must hit the real path.** Pipe 3 derives visemes from *text*, so
  shifting the audio will not desync her mouth — it will produce a null result. Inject at
  `speech.coarticulationMs → 0`, at the viseme timeline offset, or at
  `blink.frequencyBpm → 0`. (The audio-shift test is worth running separately: "we desynced
  the audio and her lip-sync didn't care" is the Pipe 3 pitch in one gesture.)

## 8. Open failures, carried forward honestly

| Item | State |
|---|---|
| **Realness** | CSS layer is a **placeholder**. Real life requires the neural renderer. Hosted Maxine endpoints returned 404 — container + cloud GPU only |
| **UPON LOAD** | **FAIL at 3.77s** measured at real frame rate. Mostly Vite dev-mode module loading, but that is untested — no production-build claim is being made |
| **Blink** | Renders, but it is a masked lid sweep, not a rendered eyelid. Upgrade path is A2F-2D `blink_frequency` |
| **MOTION pillar** | Must be rewritten to measure painted pixels, not transform values. It has now passed dead output twice |
