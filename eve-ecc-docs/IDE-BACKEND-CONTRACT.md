# IDE ⇄ Backend Contract — The Cerebral Project

Living reference for the right-side IDE work (design, gallery, reporting UI).
Backend base: `http://127.0.0.1:8100` · live bus: `ws://127.0.0.1:8100/ws`.
Everything here is running now — bind, don't mock.

## Node graph telemetry (Understand-Anything)

| Endpoint | Returns |
|---|---|
| `GET /v1/nodes` | All 9 nodes: intel (`label`, `summary`, `spoken`, `budgetMs`, `plane`) + `runtime` (`health`, `latencyMs`, `load`, `message`) |
| `GET /v1/nodes/{id}` | One node, same shape |
| `GET /v1/status` | Full snapshot (stage, warmProgress, mode, nodes, nvidia, lastTalk) |

Node ids: `mic` `presence` `syncer` `riva-asr` `nemotron` `riva-tts` `a2f` `animgraph` `omniverse`.
Health values: `cold → warming → ready → hot` (+ `degraded`, `error`).

## WebSocket messages (`/ws`)

- `{type:"snapshot", stage, warmProgress(0..1), mode, controlMs, nodes{...}, event?, nvidia, lastTalk}` — continuous
- `{type:"visemes", source, nodeId?, durationMs, frames:[{tMediaMs, viseme, energy, weights}]}` — Pipe 3 mouth
  timeline; `weights` are ARKit-52 partials. Sources: `phoneme-direct` (talk), `node-voice` (speak), `node-converse`.

## Node voice (click → the node explains itself aloud)

`POST /v1/speak` `{nodeId}` or `{text}` → `audio/wav` body.
Headers: `X-Eve-Tts-Engine`, `X-Eve-Viseme-Count`, `X-Eve-Duration-Ms`.
Matching viseme timeline broadcasts on `/ws` at the same moment — play the WAV and
drive any mouth/energy UI off the frames on one clock. Cached per node → repeat clicks instant.

## Bi-directional node conversation (talk WITH the node about its role)

`POST /v1/nodes/{id}/converse` `{text, audio?: true}` →
```json
{ "ok": true, "reply": "…", "model": "…", "latencyMs": 4225,
  "nodeId": "riva-tts", "audioB64": "…wav-base64…", "engine": "sapi-com", "turns": 1 }
```
- Node answers in FIRST PERSON with live self-knowledge (health, latency, stage, peers).
- Thread memory per node (last 6 exchanges) — follow-ups work.
- `audio: false` for text-only; default returns spoken WAV (base64) + viseme broadcast.
- IDE-owned capability: works identically over any pipeline mode (mock / NVIDIA / Foundry).

## Reporting spine (documents both sides)

`GET /v1/report` → `{mode, stage, uptimeMs, totals{talks, speaks, converses, visemeFramesEmitted,
stageChanges, errors}, lastError, perNode{id:{speaks, converses}}, log[last 50]}`.
Poll it or fold it into the snapshot cadence for the reporting pane.

## Avatar gallery data source

`GET /v1/gallery` → `{avatars:[{file, url, name, status: "completed"|"under-development"}], count}`.
Files live in `apps/web/public/staff/` — drop a new still in, it appears. Status heuristic today:
`natural|closeup` filenames = completed; extend with a `gallery.json` manifest when curation starts.

## Stage control

`POST /v1/stage` `{stage: "L0"|"L1"|"L2"}` · `POST /v1/talk` `{text, promote?}` — EVE herself replies
(Nemotron 3 Super 120B live), visemes fork automatically.

## Ground rules already enforced backend-side

- Reasoning NEVER reaches voice paths (`/no_think` + sanitizer) — don't re-surface `reasoning_content` in UI.
- `.env` never committed; key loads server-side only.
- Latency numbers in telemetry are real budgets, not decorations — safe to chart.
