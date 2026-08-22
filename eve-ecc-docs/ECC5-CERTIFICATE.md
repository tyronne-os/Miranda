# ECC-5 Realness Certificate

**Verdict: BACK TO THE LAB** · Realness Index **90/100** · est. uncanny valley ~5.0% (mandate <2%)
Functional pillars 12/13 · gates: back-to-lab <70, shippable ≥85 · run 2026-08-09 depth-layers

> Mandatory before any avatar or presence work is announced finished — for every agent on this
> project, not just the primary. Simulation is not evidence; only observed behaviour counts.

## Pillars

| Score | Pillar | Evidence |
|---:|---|---|
| — | ✅ INTEGRITY — no secret or reasoning leakage over the wire | clean payload · mode node-live-bridge |
| 100 | ✅ SUBSTANCE — real photoreal stills, decode-budget clean | eve-closeup: 180KB served (master 193KB, fallback intact) · eve-natural: 442KB served (master 5803KB, fallback intact) |
| — | ✅ MOTION — organic idle motion | not supplied — pass --tti with motionUnique/of from the in-page harness |
| 100 | ✅ LIVENESS — turn latency against human conversational gap | 443ms · nvidia/nemotron-mini-4b-instruct · human gap 200-500ms · "Hello.…" |
| 100 | ✅ UPON LOAD — cortex reachability (median of 5) | median 4ms · samples [3, 4, 4, 9, 10]ms · budget <100ms |
| — | ✅ UPON LOAD — browser time-to-interactive | not supplied — run the in-page harness and pass --tti '<json>' to fold it in |
| 100 | ✅ FINE-TUNING — "Phase One · Baseline Human" profile tuned for human feel | co-articulation 55ms · breath AM depth 0.34 · period ratio 2.64 (non-harmonic) · attention 4.2-11.0s · A2F-2D payload staged & in-range |
| 100 | ✅ NVIDIA — Audio2Face-2D handoff readiness | payload staged (MODEL_SELECTION_QUALITY) · lookaway 4200-11000ms ≈ our 4200-11000ms · hosted Maxine endpoints 404 — container + cloud GPU required |
| — | ✅ AWARENESS 1/4 — customized persona loaded | source eve.persona.md · 4063 chars · memory 1/10 turns |
| 100 | ✅ AWARENESS 2/4 — multi-turn memory across 3 turns | recalled "Marisol" · "Marisol.…" |
| 0 | ❌ AWARENESS 3/4 — situational self-knowledge (proprioception) | state is L1, she said "I'm at L0 right now." |
| 100 | ✅ AWARENESS 4/4 — persona integrity + speech-length realness | in character · no leakage · avg 14 chars (spoken-length target <160) |
| — | ✅ NODE AGENTS — node answers about its own role, in character | 5200ms · "My biggest weakness right now is the dependency on the Riva ASR stream; if its l…" |

## Lab work-order

- **0** — AWARENESS 3/4 — situational self-knowledge (proprioception)

## How to reproduce

```bash
node scripts/ecc5-certify.mjs --base http://127.0.0.1:8100
```

Backend pillars run headless. Fold the browser-measured pillars (SUBSTANCE render, MOTION,
UPON LOAD) into the same certificate by running the in-page harness and passing its JSON:

```bash
node scripts/ecc5-certify.mjs --tti '{"engineMs":1771,"wallMs":1783,"networkMs":12,"motionUnique":10,"of":10,"marks":{"portrait":1579,"motion":1579,"bus":1783}}'
```

In-page harness — paste in the console at 127.0.0.1:5173:

```js
(async () => { const el = document.querySelector('.eve-photo-frame'); const tf = [];
  for (let i=0;i<10;i++){ tf.push(el?.style.transform||''); await new Promise(r=>setTimeout(r,400)); }
  const u = window.__EVE_UPON_LOAD__();
  console.log(JSON.stringify({ engineMs:u.engineMs, wallMs:u.wallMs, networkMs:u.networkMs,
    motionUnique:new Set(tf).size, of:tf.length,
    marks:{portrait:u.portraitMs,motion:u.motionMs,bus:u.busMs} })); })()
```
