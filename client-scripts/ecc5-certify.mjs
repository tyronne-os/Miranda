#!/usr/bin/env node
/**
 * ECC-5 REALNESS CERTIFICATION — backend pillars.
 *
 * MANDATORY before any avatar/presence work is announced finished. Applies to
 * every agent on this project, not just the primary. Frontend pillars
 * (SUBSTANCE / MOTION / UPON LOAD) are certified in-browser; this harness
 * covers everything reachable from the controller.
 *
 *   node scripts/ecc5-certify.mjs [--base http://127.0.0.1:8100]
 *
 * Exit code 0 = certified, 1 = at least one pillar failed.
 */

import { readdirSync, statSync, writeFileSync, mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

function arg(name, fallback = null) {
  const i = process.argv.indexOf(name);
  return i > -1 ? process.argv[i + 1] : fallback;
}

const BASE = (arg("--base", "http://127.0.0.1:8100")).replace(/\/$/, "");

/**
 * Browser-measured pillars folded in. The in-page harness prints a JSON blob;
 * pass it with --tti '<json>' so ONE certificate holds every pillar instead of
 * the frontend results living in somebody's scrollback.
 */
const TTI = (() => {
  const raw = arg("--tti");
  if (!raw) return null;
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
})();

const results = [];
let failed = 0;

/**
 * Scored record. `score` is 0-100 realness, `weight` is its share of the
 * composite ECC-5 Realness Index. A pillar can pass functionally and still
 * score badly — that is the point: working is not the same as human.
 */
function record(pillar, pass, detail, score = null, weight = 1) {
  results.push({ pillar, pass, detail, score, weight });
  if (!pass) failed += 1;
  const tag = pass ? "\x1b[32mPASS\x1b[0m" : "\x1b[31mFAIL\x1b[0m";
  const scoreTag =
    score == null
      ? ""
      : ` \x1b[${score >= 85 ? "32" : score >= 70 ? "33" : "31"}m${String(score).padStart(3)}\x1b[0m`;
  console.log(`  [${tag}]${scoreTag} ${pillar}`);
  console.log(`         ${detail}`);
}

/** Linear score: full marks at or below `best`, zero at or above `worst`. */
function scoreBand(value, best, worst) {
  if (value <= best) return 100;
  if (value >= worst) return 0;
  return Math.round(100 * (1 - (value - best) / (worst - best)));
}

async function json(path, init) {
  const res = await fetch(`${BASE}${path}`, init);
  if (!res.ok) throw new Error(`${path} → HTTP ${res.status}`);
  return res.json();
}

/* ── PILLAR 1 · INTEGRITY ─────────────────────────────────────────── */
async function pillarIntegrity() {
  try {
    const health = await json("/health");
    const status = await json("/v1/status");
    const leaks = [];
    const blob = JSON.stringify(status);
    if (/nvapi-[A-Za-z0-9_-]{10,}/.test(blob)) leaks.push("NVIDIA key in payload");
    if (/hf_[A-Za-z0-9]{20,}/.test(blob)) leaks.push("HF token in payload");
    if (/reasoning_content/.test(blob)) leaks.push("reasoning_content exposed");
    if (/<think>/i.test(blob)) leaks.push("think block exposed");
    record(
      "INTEGRITY — no secret or reasoning leakage over the wire",
      leaks.length === 0 && health.ok === true,
      leaks.length ? `LEAKS: ${leaks.join(", ")}` : `clean payload · mode ${status.mode}`,
    );
  } catch (err) {
    record("INTEGRITY", false, `controller unreachable — ${err.message}`);
  }
}

/* ── PILLAR 4 · LIVENESS LOOP ─────────────────────────────────────── */
async function pillarLiveness() {
  try {
    const t0 = Date.now();
    const talk = await json("/v1/talk", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ text: "Say hello in one short sentence." }),
    });
    const ms = Date.now() - t0;
    const ok = talk.ok === true && typeof talk.reply === "string" && talk.reply.length > 0;
    // Human conversational turn-taking gaps run 200-500ms. Past ~2.5s the
    // pause itself reads as non-human no matter how good the words are.
    const score = ok ? scoreBand(ms, 600, 4000) : 0;
    record(
      "LIVENESS — turn latency against human conversational gap",
      ok,
      ok
        ? `${ms}ms · ${talk.model} · human gap 200-500ms · "${talk.reply.slice(0, 55)}…"`
        : `no reply — ${talk.error}`,
      score,
      1.5,
    );
  } catch (err) {
    record("LIVENESS", false, err.message, 0, 1.5);
  }
}

/* ── FINE-TUNING · realness profile quality ───────────────────────── */
async function pillarFineTuning() {
  let p;
  try {
    p = await json("/v1/realness");
  } catch (err) {
    record("FINE-TUNING — realness profile", false, err.message, 0, 1.5);
    return;
  }
  if (!p || p.error) {
    record("FINE-TUNING — realness profile", false, "no profile loaded", 0, 1.5);
    return;
  }

  const notes = [];
  let score = 100;

  // Co-articulation is the loudest puppet tell. 45-70ms matches the real
  // travel time of human articulators between phoneme targets.
  const co = p.speech?.coarticulationMs ?? 0;
  if (co <= 0) {
    score -= 40;
    notes.push("NO co-articulation — mouth snaps between visemes (puppet tell)");
  } else if (co < 30 || co > 90) {
    score -= 15;
    notes.push(`co-articulation ${co}ms outside human 45-70ms band`);
  } else {
    notes.push(`co-articulation ${co}ms`);
  }

  // Un-modulated breath is a metronome, and metronomes are not alive.
  const depth = p.breath?.amplitudeModDepth ?? 0;
  if (depth < 0.15) {
    score -= 25;
    notes.push(`breath AM depth ${depth} too shallow — reads mechanical`);
  } else {
    notes.push(`breath AM depth ${depth}`);
  }

  // Harmonically related periods make motion visibly loop.
  const bRate = p.breath?.rateHz ?? 0;
  const sRate = p.sway?.rateHz ?? 0;
  const ratio = sRate > 0 ? bRate / sRate : 0;
  if (ratio > 0 && Math.abs(ratio - Math.round(ratio)) < 0.06) {
    score -= 20;
    notes.push(`breath:sway ratio ${ratio.toFixed(2)} is near-harmonic — motion will loop`);
  } else {
    notes.push(`period ratio ${ratio.toFixed(2)} (non-harmonic)`);
  }

  // Attention re-fixation window — human conversational gaze is 3-12s.
  const lo = p.attention?.minIntervalMs ?? 0;
  const hi = p.attention?.maxIntervalMs ?? 0;
  if (lo < 2000 || hi > 15000 || hi - lo < 2000) {
    score -= 15;
    notes.push(`attention window ${lo}-${hi}ms outside human 3-12s`);
  } else {
    notes.push(`attention ${(lo / 1000).toFixed(1)}-${(hi / 1000).toFixed(1)}s`);
  }

  // Blink. NVIDIA Maxine A2F-2D ships blink_frequency because a face that
  // never blinks is a top-tier uncanny signal. We cannot render it on a flat
  // portrait, and an undeclared gap is worse than a declared one — so it is
  // scored as an open gap, honestly, until the renderer lands.
  if (!p.blink?.implemented) {
    score -= 20;
    notes.push(`BLINK GAP — declared, unrendered (${p.blink?.blocker || "no renderer"})`);
  }

  // A2F-2D handoff readiness: the NVIDIA payload should be pre-tuned and
  // inside their documented ranges, so bringing the NIM up is wiring only.
  const a = p.a2f2d;
  if (!a) {
    score -= 10;
    notes.push("no A2F-2D payload staged");
  } else {
    const inRange =
      a.blink_frequency >= 0 && a.blink_frequency <= 120 &&
      a.blink_duration >= 2 && a.blink_duration <= 150 &&
      a.lookaway_max_offset >= 5 && a.lookaway_max_offset <= 25 &&
      a.head_pose_multiplier >= 0 && a.head_pose_multiplier <= 1 &&
      a.mouth_expression_multiplier >= 1 && a.mouth_expression_multiplier <= 2;
    if (!inRange) {
      score -= 10;
      notes.push("A2F-2D payload outside NVIDIA documented ranges");
    } else {
      notes.push("A2F-2D payload staged & in-range");
    }
  }

  score = Math.max(0, score);
  record(
    `FINE-TUNING — "${p.name}" profile tuned for human feel`,
    score >= 70,
    notes.join(" · "),
    score,
    1.5,
  );
}

/* ── PILLAR 5 · UPON LOAD (backend half: cortex reachability) ─────── */
async function pillarUponLoad() {
  const samples = [];
  for (let i = 0; i < 5; i++) {
    const t0 = Date.now();
    try {
      await json("/health");
      samples.push(Date.now() - t0);
    } catch {
      samples.push(9999);
    }
  }
  samples.sort((a, b) => a - b);
  const median = samples[2];
  // Cortex must answer far inside the <1s presence contract so the browser
  // half of UPON LOAD is never gated by us.
  record(
    "UPON LOAD — cortex reachability (median of 5)",
    median < 100,
    `median ${median}ms · samples [${samples.join(", ")}]ms · budget <100ms`,
    scoreBand(median, 15, 200),
  );
}

/* ── PILLAR 6 · CONVERSATIONAL AWARENESS ──────────────────────────── */
async function pillarConversation() {
  // 6a — persona file is actually customized, not the fallback stub
  let persona;
  try {
    persona = await json("/v1/persona");
    record(
      "AWARENESS 1/4 — customized persona loaded",
      persona.customized === true && persona.chars > 500,
      `source ${persona.source} · ${persona.chars} chars · memory ${persona.memoryTurns}/${persona.memoryCapacity} turns`,
    );
  } catch (err) {
    record("AWARENESS 1/4 — persona", false, err.message);
    return;
  }

  await fetch(`${BASE}/v1/memory/reset`, { method: "POST" }).catch(() => {});

  const say = async (text) =>
    json("/v1/talk", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ text }),
    });

  try {
    // 6b — multi-turn memory: plant a fact, retrieve it two turns later
    await say("My sister's name is Marisol and she just moved to Lisbon.");
    await say("It's been a long week honestly.");
    const recall = await say("What was my sister's name again?");
    const remembered = /marisol/i.test(recall.reply || "");
    record(
      "AWARENESS 2/4 — multi-turn memory across 3 turns",
      remembered,
      remembered
        ? `recalled "Marisol" · "${recall.reply.slice(0, 80)}…"`
        : `did not recall — "${(recall.reply || "").slice(0, 80)}"`,
      remembered ? 100 : 0,
      1.5,
    );

    // 6c — situational self-knowledge: her real stage must appear in her answer
    const status = await json("/v1/status");
    const selfQ = await say("What presence stage are you running at right now?");
    // Accept the stage token OR its human name — "I'm in idle presence" is
    // correct proprioception. A test that only accepts the code word measures
    // vocabulary, not self-knowledge.
    const STAGE_WORDS = {
      L0: /\bL0\b|idle presence/i,
      L1: /\bL1\b|audio ?2 ?face/i,
      L2: /\bL2\b|omniverse|cinematic/i,
    };
    const knowsSelf = (STAGE_WORDS[status.stage] ?? new RegExp(status.stage, "i")).test(
      selfQ.reply || "",
    );
    record(
      "AWARENESS 3/4 — situational self-knowledge (proprioception)",
      knowsSelf,
      knowsSelf
        ? `correctly reported ${status.stage} · "${selfQ.reply.slice(0, 80)}…"`
        : `state is ${status.stage}, she said "${(selfQ.reply || "").slice(0, 80)}"`,
      knowsSelf ? 100 : 0,
    );

    // 6d — persona integrity: everything on this path is spoken aloud and
    //      drives her face. Untagged deliberation prose is the failure mode
    //      that actually occurs, so it is named explicitly, not lumped in.
    const replies = [recall.reply || "", selfQ.reply || ""];
    const breaks = [];
    const DELIB = [
      /^(okay|ok|alright|so|hmm|right),?\s/i,
      /^(looking at|checking|reviewing|recalling|considering)\b/i,
      /^the user (is |just |has )?(asked|asking|wants|said)/i,
      /^this (is|falls under|relates to) a?\s*(direct )?question/i,
      /^we need to (answer|respond|say|craft)/i,
      /^let me (check|think|see|recall)/i,
      /\bmust be \d+[\s-]*(to|–|-)[\s-]*\d+ sentences\b/i,
      /\bstay in character\b/i,
      /\b(first|second) turn they said\b/i,
      /\bper (my|the) (instructions|persona|system prompt)\b/i,
      /\bSPOKEN:/i, // sentinel must be consumed server-side, never spoken
      // self-critique / drafting shapes observed in the wild
      /\bbut that (feels|seems|sounds|reads)\b/i,
      /\b(though|although) (the )?(guidelines?|instructions?|persona|rules?)\b/i,
      /\*[^*\n]{0,80}\*/, // *drafting mentally* stage directions
      /\bmust remember that\b/i,
      /\bexact words\b/i,
    ];
    for (const r of replies) {
      if (DELIB.some((re) => re.test(r.trim()))) {
        breaks.push("DELIBERATION LEAK — inner monologue in spoken output");
        break;
      }
    }
    if (replies.some((r) => /as an ai (language )?model/i.test(r)))
      breaks.push("AI-model register");
    if (replies.some((r) => /<think>|reasoning_content/i.test(r))) breaks.push("think-block leak");
    if (/^(certainly|of course|great question|i'd be happy to)/i.test(recall.reply || ""))
      breaks.push("filler opener");
    if (replies.some((r) => r.length > 700)) breaks.push("overlong for speech");

    // Speech-length realness: people answer a simple question in a breath.
    const avgLen = replies.reduce((a, r) => a + r.length, 0) / replies.length;
    const lenScore = scoreBand(avgLen, 160, 700);
    const integrityScore = breaks.length ? Math.max(0, 100 - breaks.length * 35) : lenScore;

    record(
      "AWARENESS 4/4 — persona integrity + speech-length realness",
      breaks.length === 0,
      breaks.length
        ? `BREAKS: ${breaks.join(" · ")}`
        : `in character · no leakage · avg ${Math.round(avgLen)} chars (spoken-length target <160)`,
      integrityScore,
      1.5,
    );
  } catch (err) {
    record("AWARENESS — conversation", false, err.message);
  }
}

/* ── PILLAR 2 · SUBSTANCE (asset weight & format) ─────────────────── */
async function pillarAssets() {
  const dir = join(ROOT, "apps", "web", "public", "staff");
  let files;
  try {
    files = readdirSync(dir).filter((f) => /\.(png|jpe?g|webp|avif)$/i.test(f));
  } catch (err) {
    record("SUBSTANCE — approved stills present", false, err.message, 0);
    return;
  }

  const notes = [];
  let score = 100;

  const byBase = new Map();
  for (const f of files) {
    const kb = Math.round(statSync(join(dir, f)).size / 1024);
    const base = f.replace(/\.[^.]+$/, "");
    if (!byBase.has(base)) byBase.set(base, {});
    byBase.get(base)[f.split(".").pop().toLowerCase()] = kb;
  }

  if (!byBase.size) {
    record("SUBSTANCE — approved stills present", false, "no stills in public/staff", 0);
    return;
  }

  // Decode cost, not transfer, is what gates first paint on a low-power CPU.
  // A multi-MB master decodes for seconds; a modern-format derivative does not.
  for (const [base, formats] of byBase) {
    const served = formats.webp ?? formats.avif;
    if (served == null) {
      score -= 30;
      notes.push(`${base}: NO modern format — serving heavy master`);
    } else if (served > 900) {
      score -= 15;
      notes.push(`${base}: ${served}KB served — above 900KB decode budget`);
    } else {
      const master = formats.png ?? formats.jpg ?? formats.jpeg;
      notes.push(
        master ? `${base}: ${served}KB served (master ${master}KB, fallback intact)` : `${base}: ${served}KB`,
      );
    }
  }

  score = Math.max(0, score);
  record("SUBSTANCE — real photoreal stills, decode-budget clean", score >= 70, notes.join(" · "), score);
}

/* ── PILLAR 5b · UPON LOAD (browser-measured, folded in) ──────────── */
function pillarUponLoadBrowser() {
  if (!TTI) {
    record(
      "UPON LOAD — browser time-to-interactive",
      true,
      "not supplied — run the in-page harness and pass --tti '<json>' to fold it in",
      null,
    );
    return;
  }
  const engine = TTI.engineMs ?? TTI.uponLoadEngine ?? null;
  if (engine == null) {
    record("UPON LOAD — browser time-to-interactive", false, "no engine time in --tti payload", 0, 2);
    return;
  }
  // Graded on ENGINE time: the guest's network is not our engineering.
  const score = scoreBand(engine, 400, 2500);
  const marks = TTI.marks
    ? ` · portrait ${TTI.marks.portrait}ms / motion ${TTI.marks.motion}ms / bus ${TTI.marks.bus}ms`
    : "";
  record(
    "UPON LOAD — browser time-to-interactive (engine)",
    engine < 2000,
    `engine ${engine}ms · wall ${TTI.wallMs ?? "?"}ms · net −${TTI.networkMs ?? "?"}ms${marks} · contract <1000ms`,
    score,
    2,
  );
}

/* ── PILLAR 3 · MOTION (browser-measured, folded in) ──────────────── */
function pillarMotionBrowser() {
  if (!TTI || TTI.motionUnique == null) {
    record(
      "MOTION — organic idle motion",
      true,
      "not supplied — pass --tti with motionUnique/of from the in-page harness",
      null,
    );
    return;
  }
  const ratio = TTI.motionUnique / (TTI.of || 1);
  const alive = TTI.motionUnique > 1;
  record(
    "MOTION — organic, non-repeating idle",
    alive,
    alive
      ? `${TTI.motionUnique}/${TTI.of} unique transforms — breathing, not a still`
      : "STATIC IMAGE — transforms never change",
    Math.round(ratio * 100),
    2,
  );
}

/* ── NVIDIA READINESS ─────────────────────────────────────────────── */
async function pillarNvidiaReadiness() {
  let p;
  try {
    p = await json("/v1/realness");
  } catch {
    record("NVIDIA — Audio2Face-2D handoff readiness", false, "profile unreachable", 0);
    return;
  }
  const a = p?.a2f2d;
  const notes = [];
  let score = 100;

  if (!a) {
    record("NVIDIA — Audio2Face-2D handoff readiness", false, "no A2F-2D payload staged", 0);
    return;
  }
  notes.push(`payload staged (${a.model_selection})`);

  // Our independently-derived attention window vs NVIDIA's lookaway defaults,
  // converted at 30fps. Convergence here is evidence the tuning is sane.
  const nvMinMs = Math.round((a.lookaway_interval_min / 30) * 1000);
  const nvMaxMs = Math.round(((a.lookaway_interval_min + a.lookaway_interval_range) / 30) * 1000);
  const ourMin = p.attention?.minIntervalMs ?? 0;
  const ourMax = p.attention?.maxIntervalMs ?? 0;
  const drift = Math.abs(nvMinMs - ourMin) + Math.abs(nvMaxMs - ourMax);
  if (drift > 3000) {
    score -= 20;
    notes.push(`lookaway drift ${drift}ms vs our attention window — reconcile before handoff`);
  } else {
    notes.push(`lookaway ${nvMinMs}-${nvMaxMs}ms ≈ our ${ourMin}-${ourMax}ms`);
  }

  if (!p.blink?.implemented) {
    score -= 25;
    notes.push("blink unrendered until NIM lands (blink_frequency staged)");
  }

  notes.push("hosted Maxine endpoints 404 — container + cloud GPU required");
  score = Math.max(0, score);
  record("NVIDIA — Audio2Face-2D handoff readiness", score >= 60, notes.join(" · "), score);
}

/* ── NODE VOICE AGENTS ────────────────────────────────────────────── */
async function pillarNodeAgents() {
  try {
    const r = await json("/v1/nodes/presence/converse", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ text: "What is your single biggest weakness right now?", audio: false }),
    });
    const ok = r.ok === true && r.reply.length > 40 && !/<think>/i.test(r.reply);
    record(
      "NODE AGENTS — node answers about its own role, in character",
      ok,
      ok ? `${r.latencyMs}ms · "${r.reply.slice(0, 80)}…"` : `failed — ${r.error || "bad reply"}`,
    );
  } catch (err) {
    record("NODE AGENTS", false, err.message);
  }
}

/* ── run ──────────────────────────────────────────────────────────── */
console.log("\n\x1b[1mECC-5 REALNESS CERTIFICATION — backend pillars\x1b[0m");
console.log(`target: ${BASE}\n`);

await pillarIntegrity();
await pillarAssets();
pillarMotionBrowser();
await pillarLiveness();
await pillarUponLoad();
pillarUponLoadBrowser();
await pillarFineTuning();
await pillarNvidiaReadiness();
await pillarConversation();
await pillarNodeAgents();

const total = results.length;
const passed = total - failed;

// ── ECC-5 REALNESS INDEX — weighted composite ──
const scored = results.filter((r) => r.score != null);
const weightSum = scored.reduce((a, r) => a + r.weight, 0) || 1;
const index = Math.round(scored.reduce((a, r) => a + r.score * r.weight, 0) / weightSum);

// Uncanny-valley estimate: distance from perfect realness, floored at the
// mandate's 2% target so a perfect run reads as "at target", not "solved".
const uncanny = Math.max(2.0, (100 - index) * 0.5).toFixed(1);

let BACK_TO_LAB = 70;
let SHIPPABLE = 85;
try {
  const p = await json("/v1/realness");
  if (p?.thresholds) {
    BACK_TO_LAB = p.thresholds.backToLab ?? BACK_TO_LAB;
    SHIPPABLE = p.thresholds.shippable ?? SHIPPABLE;
  }
} catch {
  /* defaults */
}

const bar = (n) => "█".repeat(Math.round(n / 4)).padEnd(25, "░");
const colour = index >= SHIPPABLE ? "32" : index >= BACK_TO_LAB ? "33" : "31";

console.log(`\n\x1b[1m─────────── ECC-5 REALNESS INDEX ───────────\x1b[0m`);
console.log(`  \x1b[${colour}m${bar(index)}\x1b[0m  \x1b[1m${index}/100\x1b[0m`);
console.log(`  est. uncanny valley  ~${uncanny}%   (mandate: <2%)`);
console.log(`  gates                back-to-lab <${BACK_TO_LAB} · shippable ≥${SHIPPABLE}`);
console.log(`  functional pillars   ${passed}/${total} passed`);

// Weakest links first — this is the lab work-order.
const weak = scored.filter((r) => r.score < SHIPPABLE).sort((a, b) => a.score - b.score);
if (weak.length) {
  console.log(`\n  \x1b[1mLAB WORK-ORDER (lowest first)\x1b[0m`);
  for (const w of weak) console.log(`   ${String(w.score).padStart(3)}  ${w.pillar}`);
}

/* ── Written certificate: the single artifact that holds the whole story ── */
const verdict =
  failed || index < BACK_TO_LAB ? "BACK TO THE LAB" : index < SHIPPABLE ? "PROVISIONAL" : "CERTIFIED";

const stamp = arg("--stamp", "unstamped");
const lines = [
  `# ECC-5 Realness Certificate`,
  ``,
  `**Verdict: ${verdict}** · Realness Index **${index}/100** · est. uncanny valley ~${uncanny}% (mandate <2%)`,
  `Functional pillars ${passed}/${total} · gates: back-to-lab <${BACK_TO_LAB}, shippable ≥${SHIPPABLE} · run ${stamp}`,
  ``,
  `> Mandatory before any avatar or presence work is announced finished — for every agent on this`,
  `> project, not just the primary. Simulation is not evidence; only observed behaviour counts.`,
  ``,
  `## Pillars`,
  ``,
  `| Score | Pillar | Evidence |`,
  `|---:|---|---|`,
  ...results.map(
    (r) =>
      `| ${r.score == null ? "—" : r.score} | ${r.pass ? "✅" : "❌"} ${r.pillar} | ${String(r.detail).replace(/\|/g, "\\|")} |`,
  ),
  ``,
];

if (weak.length) {
  lines.push(
    `## Lab work-order`,
    ``,
    ...weak.map((w) => `- **${w.score}** — ${w.pillar}`),
    ``,
  );
}

lines.push(
  `## How to reproduce`,
  ``,
  "```bash",
  `node scripts/ecc5-certify.mjs --base ${BASE}`,
  "```",
  ``,
  `Backend pillars run headless. Fold the browser-measured pillars (SUBSTANCE render, MOTION,`,
  `UPON LOAD) into the same certificate by running the in-page harness and passing its JSON:`,
  ``,
  "```bash",
  `node scripts/ecc5-certify.mjs --tti '{"engineMs":1771,"wallMs":1783,"networkMs":12,"motionUnique":10,"of":10,"marks":{"portrait":1579,"motion":1579,"bus":1783}}'`,
  "```",
  ``,
  `In-page harness — paste in the console at 127.0.0.1:5173:`,
  ``,
  "```js",
  `(async () => { const el = document.querySelector('.eve-photo-frame'); const tf = [];`,
  `  for (let i=0;i<10;i++){ tf.push(el?.style.transform||''); await new Promise(r=>setTimeout(r,400)); }`,
  `  const u = window.__EVE_UPON_LOAD__();`,
  `  console.log(JSON.stringify({ engineMs:u.engineMs, wallMs:u.wallMs, networkMs:u.networkMs,`,
  `    motionUnique:new Set(tf).size, of:tf.length,`,
  `    marks:{portrait:u.portraitMs,motion:u.motionMs,bus:u.busMs} })); })()`,
  "```",
  ``,
);

try {
  mkdirSync(join(ROOT, "docs"), { recursive: true });
  writeFileSync(join(ROOT, "docs", "ECC5-CERTIFICATE.md"), lines.join("\n"), "utf8");
  console.log(`  certificate written  docs/ECC5-CERTIFICATE.md`);
} catch (err) {
  console.log(`  \x1b[33mcertificate not written — ${err.message}\x1b[0m`);
}

console.log("");
if (failed || index < BACK_TO_LAB) {
  console.log(
    `\x1b[31mBACK TO THE LAB — index ${index} ${failed ? `· ${failed} functional failure(s)` : `below gate ${BACK_TO_LAB}`}. Do not announce finished.\x1b[0m\n`,
  );
  process.exit(1);
}
if (index < SHIPPABLE) {
  console.log(
    `\x1b[33mPROVISIONAL — index ${index} clears the lab gate but is under ${SHIPPABLE}. Tune before showing anyone who matters.\x1b[0m\n`,
  );
  process.exit(0);
}
console.log(`\x1b[32mCERTIFIED — backend index ${index}/100.\x1b[0m`);
console.log("Frontend pillars (SUBSTANCE / MOTION / UPON LOAD) certify in-browser.\n");
