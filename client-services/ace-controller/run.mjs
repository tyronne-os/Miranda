#!/usr/bin/env node
/**
 * Launch ace-controller with the best available runtime.
 * Prefers Python/FastAPI; falls back to a pure Node mock so the IDE never blocks.
 */
import { spawn } from "node:child_process";
import { existsSync, readFileSync, readdirSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer } from "node:http";
import { WebSocketServer } from "./node_ws_shim.mjs";
import { NODE_INTEL, phonemeTimeline, speak } from "./presence_kit.mjs";
import { AudioSessionRegistry } from "./audioSession.mjs";
import { randomId } from "./node_ws_shim.mjs";
import { TranscribeSessionGuard } from "./transcribeBridge.mjs";
import { transcribeUtteranceWhisper } from "./whisperBridge.mjs";
import { readVaultKey } from "./awsCredentials.mjs";

const root = dirname(fileURLToPath(import.meta.url));
const isWin = process.platform === "win32";

/** Load repo-root .env without overwriting existing process.env keys. Never log secret values. */
function loadRootEnv() {
  const envPath = join(root, "..", "..", ".env");
  if (!existsSync(envPath)) return;
  try {
    const text = readFileSync(envPath, "utf8");
    for (const line of text.split(/\r?\n/)) {
      const trimmed = line.trim();
      if (!trimmed || trimmed.startsWith("#")) continue;
      const eq = trimmed.indexOf("=");
      if (eq <= 0) continue;
      const key = trimmed.slice(0, eq).trim();
      let val = trimmed.slice(eq + 1).trim();
      if (
        (val.startsWith('"') && val.endsWith('"')) ||
        (val.startsWith("'") && val.endsWith("'"))
      ) {
        val = val.slice(1, -1);
      }
      if (process.env[key] === undefined) {
        process.env[key] = val;
      }
    }
    console.log(`[ace-controller] loaded env file: ${envPath}`);
  } catch (err) {
    console.warn(`[ace-controller] could not load .env: ${err?.message || err}`);
  }
}

loadRootEnv();

// WO-2 T4 pivot (Rule 5 — cognitive-core hot-swap while Bedrock's account is
// under AWS trust/safety review): nvidiaChat()/nvidiaChatMessages() below
// already check process.env.NVIDIA_API_KEY, the same pattern .env-loading
// uses. If it's not already set (by .env or a real shell export), fall back
// to the AMANDA vault. Two NVIDIA entries exist in the vault
// ("nvidia" and "nvidiaenterprise") — "nvidiaenterprise" is the one
// confirmed live/tested for this account, so it's tried first. Never logs
// the value.
if (!process.env.NVIDIA_API_KEY && !process.env.NGC_API_KEY && !process.env.NVAPI_KEY) {
  const vaultNvidiaKey = readVaultKey("nvidiaenterprise") || readVaultKey("nvidia");
  if (vaultNvidiaKey) {
    process.env.NVIDIA_API_KEY = vaultNvidiaKey;
    console.log("[ace-controller] NVIDIA_API_KEY loaded from AMANDA vault");
  }
}

// WO-2 second pivot (Rule 5): Amazon Transcribe Streaming is also blocked
// on this AWS account (same UnrecognizedClientException as Bedrock,
// reproduced directly against AWS — an account-level issue, not a code
// defect). Pipeline 1's ASR role slot moves to OpenAI Whisper, whose
// vaulted key is confirmed live. transcribeBridge.mjs and awsClients stay
// intact/unused for reactivation once AWS unblocks; see whisperBridge.mjs
// for the real tradeoff this swap accepts (no partial transcripts).
if (!process.env.OPENAI_API_KEY) {
  const vaultOpenAiKey = readVaultKey("openai");
  if (vaultOpenAiKey) {
    process.env.OPENAI_API_KEY = vaultOpenAiKey;
    console.log("[ace-controller] OPENAI_API_KEY loaded from AMANDA vault (Whisper ASR)");
  }
}

/**
 * EVE's Phase One persona / instruction file. Editing persona/eve.persona.md
 * changes who she is with no code change. Falls back to a minimal identity so
 * the control plane never dies over a missing file.
 */
function loadPersona() {
  const file = join(root, "persona", "eve.persona.md");
  try {
    const text = readFileSync(file, "utf8").trim();
    if (text) {
      console.log(`[ace-controller] persona loaded: ${file} (${text.length} chars)`);
      return { text, source: "eve.persona.md", chars: text.length };
    }
  } catch {
    /* fall through */
  }
  console.warn("[ace-controller] persona file missing — using minimal identity");
  return {
    text:
      "You are EVE, the Extravert Cognitive Companion by Beryl AI Labs. Warm, present, " +
      "concise — 1 to 3 sentences, spoken aloud. Never mention being an AI pipeline.",
    source: "fallback",
    chars: 0,
  };
}

let PERSONA = loadPersona();

/** Persist an edited persona from the lab bench, then hot-reload it. */
function savePersona(text) {
  const dir = join(root, "persona");
  const file = join(dir, "eve.persona.md");
  mkdirSync(dir, { recursive: true });
  writeFileSync(file, String(text), "utf8");
  PERSONA = loadPersona();
  return PERSONA;
}

/**
 * ECC-5 Realness Profile — how human she FEELS, as tunable data.
 * Frontend motion + viseme layers read this live; the lab bench writes it.
 */
function loadRealness() {
  const file = join(root, "persona", "realness.profile.json");
  try {
    const parsed = JSON.parse(readFileSync(file, "utf8"));
    console.log(`[ace-controller] realness profile: ${parsed.name} (v${parsed.version})`);
    return parsed;
  } catch {
    console.warn("[ace-controller] realness profile missing — frontend will use built-in defaults");
    return null;
  }
}

let REALNESS = loadRealness();

function saveRealness(profile) {
  const dir = join(root, "persona");
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "realness.profile.json"), JSON.stringify(profile, null, 2), "utf8");
  REALNESS = loadRealness();
  return REALNESS;
}

/**
 * Runtime lab settings — the dials an operator turns to shape who she is
 * without touching code. Admin-panel owned; never surfaced on the front.
 */
const LAB = {
  /**
   * LIVE speech path. Deliberately a non-reasoning instruct model: reasoning
   * models narrate their planning into `content`, and on a path wired to a
   * voice synthesizer and a face that is a defect, not a feature. Mini also
   * lands the turn inside the human conversational gap where 120B cannot.
   */
  model: process.env.NEMOTRON_MODEL || "nvidia/nemotron-mini-4b-instruct",
  /** Node-agent consults — depth over speed, still sanitized before speech. */
  consultModel: process.env.NEMOTRON_CONSULT_MODEL || "nvidia/nemotron-3-super-120b-a12b",
  temperature: 0.7,
  topP: 0.9,
  maxTokens: 420,
  memoryTurns: 10,
};

const PORT = Number(process.env.ACE_PORT || 8100);
const HOST = process.env.ACE_HOST || "127.0.0.1";

function tryPython() {
  const candidates = isWin ? ["py", "python"] : ["python3", "python"];
  for (const cmd of candidates) {
    try {
      const r = spawn(cmd, ["--version"], { stdio: "ignore", shell: isWin });
      // sync-ish probe via close
      // we'll actually spawn uvicorn below if module exists
      return cmd;
    } catch {
      /* continue */
    }
  }
  return null;
}

function startPython(cmd) {
  const app = join(root, "app", "main.py");
  if (!existsSync(app)) return null;

  console.log(`[ace-controller] starting FastAPI via ${cmd} on ${HOST}:${PORT}`);
  const child = spawn(
    cmd,
    ["-m", "uvicorn", "app.main:app", "--host", HOST, "--port", String(PORT), "--reload"],
    {
      cwd: root,
      stdio: "inherit",
      shell: isWin,
      env: { ...process.env },
    },
  );
  child.on("exit", (code) => {
    if (code && code !== 0) {
      console.warn(`[ace-controller] python exited ${code} — falling back to Node mock`);
      startNodeMock();
    }
  });
  return child;
}

function startNodeMock() {
  console.log(`[ace-controller] Node Instant Presence mock on http://${HOST}:${PORT}`);

  const stages = ["L0", "L1", "L2"];
  const nvidiaKey = Boolean(
    process.env.NVIDIA_API_KEY || process.env.NGC_API_KEY || process.env.NVAPI_KEY,
  );
  const state = {
    stage: "L0",
    targetStage: "L0",
    warmProgress: 0.4,
    mode: nvidiaKey ? "node-live-bridge" : "node-mock",
    startedAt: Date.now(),
    clients: 0,
    lastTalk: null,
    lastModel: null,
    talking: false,
  };

  // WO-5 T1: rewired to match the real Miranda-Engine topology (see
  // presence_kit.mjs's NODE_INTEL and client-apps/web/src/data/aceTopology.ts).
  const nodes = [
    "mic",
    "presence",
    "cloud-bridge",
    "native-capture",
    "ipc-bus",
    "supervisor",
    "kinematics",
    "transport",
    "renderer",
  ];

  // ── Session telemetry — the reporting spine documenting both sides ──
  const report = {
    startedAt: Date.now(),
    talks: 0,
    speaks: 0,
    converses: 0,
    visemeFramesEmitted: 0,
    stageChanges: 0,
    errors: 0,
    lastError: null,
    perNode: Object.fromEntries(nodes.map((id) => [id, { speaks: 0, converses: 0 }])),
    log: [], // rolling — last 200 entries
  };
  function logReport(kind, detail) {
    report.log.push({ t: Date.now(), kind, detail });
    if (report.log.length > 200) report.log.shift();
  }

  // ── Bi-directional node conversations — IDE capability, pipeline-agnostic ──
  // Every node is a voice agent the architect can consult about its own role.
  const nodeChats = new Map(); // nodeId → [{role, content}...] capped at 12 turns

  function nodePersona(id) {
    const intel = NODE_INTEL[id];
    const snap = snapshot();
    const rt = snap.nodes[id];
    const peers = nodes
      .filter((n) => n !== id)
      .map((n) => `${NODE_INTEL[n].label} (${NODE_INTEL[n].summary})`)
      .join("; ");
    return (
      `You are ${intel.label}, a living node inside The Cerebral Project — ` +
      `Beryl AI Labs' Instant Presence cortex for the E.C.C. digital human. ` +
      `Your role: ${intel.spoken} ` +
      `Your latency budget is ${intel.budgetMs}ms; right now your health is "${rt.health}" ` +
      `at ${rt.latencyMs}ms under stage ${snap.stage}. ` +
      `Your peer nodes: ${peers}. ` +
      `The architect is consulting you about improving your role in the pipeline. ` +
      `Speak in first person as the node. Be specific and practical — real latencies, ` +
      `real failure modes, concrete upgrades. 2 to 4 sentences, written to be spoken aloud. ` +
      `Never break character, never mention being an AI language model.`
    );
  }

  function snapshot() {
    const t = Date.now() - state.startedAt;
    // WO-5 T1: rewired for the real node set. "mic"/"presence" stay the
    // always-hot control plane. "cloud-bridge" plays the role "nemotron"
    // used to (the reasoning hop that needs the NVIDIA key warm). "renderer"
    // (WebGPU viewport, not yet built) plays the role "omniverse" used to —
    // deliberately slow to warm, never a boot blocker.
    const healthFor = (id) => {
      if (["mic", "presence"].includes(id)) return state.talking ? "hot" : "ready";
      if (nvidiaKey && id === "cloud-bridge") {
        if (state.stage !== "L0" || state.talking) return "hot";
        return state.warmProgress > 0.35 ? "ready" : "warming";
      }
      if (state.stage === "L0") return state.warmProgress > 0.5 ? "warming" : "cold";
      if (state.stage === "L1") {
        if (id === "renderer") return "warming";
        return "hot";
      }
      return "hot";
    };

    return {
      type: "snapshot",
      tMediaMs: t,
      stage: state.stage,
      targetStage: state.targetStage,
      warmProgress: state.warmProgress,
      mode: state.mode,
      controlMs: 35 + Math.floor(Math.random() * 20),
      nvidia: { configured: nvidiaKey, model: state.lastModel },
      lastTalk: state.lastTalk,
      nodes: Object.fromEntries(
        nodes.map((id) => {
          const health = healthFor(id);
          const budget = NODE_INTEL[id]?.budgetMs ?? 40;
          // live nodes breathe inside their latency budget; cold nodes read zero
          const latencyMs =
            health === "cold"
              ? 0
              : health === "warming"
                ? Math.round(budget * 1.8)
                : Math.round(budget * (0.6 + Math.abs(Math.sin(t / 1400 + id.length)) * 0.3));
          return [
            id,
            {
              id,
              health,
              latencyMs,
              load: health === "hot" ? 0.55 : 0.2,
              message: NODE_INTEL[id]?.summary ?? health,
            },
          ];
        }),
      ),
      event: {
        kind: "system",
        level: "ok",
        message: `ACE controller ${state.mode} · ${state.stage}`,
      },
    };
  }

  function broadcast(snap) {
    for (const s of sockets) {
      if (s.readyState === 1) s.send(JSON.stringify(snap));
    }
  }

  /**
   * Deliberation markers — reasoning models leak untagged planning prose into
   * `content`, not just inside <think>. Any paragraph opening like this is the
   * model talking to itself and must never be spoken or drive her face.
   */
  const DELIBERATION = [
    /^(okay|ok|alright|so),?\s+(the\s+)?(user|we|i|let)/i,
    /^(hmm|right|well),?\s/i,
    /^(looking at|checking|reviewing|recalling|considering|drafting)\b/i,
    /^but that (feels|seems|sounds|reads)\b/i,
    /^(though|although) (the )?(guidelines?|instructions?|persona|rules?)\b/i,
    /^this (is|falls under|relates to) a?\s*(direct )?question/i,
    /\bmust remember that\b/i,
    /^the user (is |just |has )?(asked|asking|wants|said|mentioned|told)/i,
    /^we need to (answer|respond|say|craft|reply|keep|make)/i,
    /^i (need|should|must) to? ?(answer|respond|recall|check|keep)/i,
    /^let me (check|think|see|recall|craft|keep)/i,
    /^let'?s (craft|keep|make|answer|say|aim)/i,
    /^(must|should) (be|stay|avoid|mention|use|include)\b/i,
    /^(per|according to) (the |my )?(persona|instructions|system)/i,
    /^(answer|response|reply)\s*[:—-]/i,
    /^\d+[\s-]*(to|–|-)[\s-]*\d+ sentences/i,
  ];

  function isDeliberation(paragraph) {
    const p = paragraph.trim();
    if (!p) return false;
    return DELIBERATION.some((re) => re.test(p));
  }

  /**
   * Spoken-text guard: reasoning must NEVER reach the voice or the face.
   *
   * Strips tagged <think> blocks, then drops leading untagged deliberation
   * paragraphs. If EVERY paragraph is deliberation, keep the last one — with
   * these models the real answer is what they land on.
   */
  function sanitizeSpoken(reply) {
    let out = String(reply || "");
    out = out.replace(/<think>[\s\S]*?<\/think>/gi, "");
    out = out.replace(/^\s*<think>[\s\S]*/i, ""); // unterminated block
    out = out.trim();
    if (!out) return "";

    // Positive extraction beats negative pattern-chasing. Reasoning models
    // invent new deliberation openers faster than regexes can be written, so
    // we ask for a sentinel and take only what follows the LAST one. Anything
    // before it — however it was phrased — was her thinking, not her voice.
    const sentinel = /SPOKEN\s*:/gi;
    let lastIdx = -1;
    let m;
    while ((m = sentinel.exec(out)) !== null) lastIdx = m.index + m[0].length;
    if (lastIdx > -1) {
      out = out.slice(lastIdx).trim();
      // Strip a wrapping quote pair the model sometimes adds.
      out = out.replace(/^["“'](.*)["”']$/s, "$1").trim();
      // Defense in depth: the model can echo the sentinel MID-deliberation and
      // keep thinking afterwards, so what follows it is not automatically
      // speech. Keep only the first paragraph and drop stage directions.
      out = out.split(/\n{2,}/)[0].trim();
      out = out.replace(/\*[^*\n]{0,80}\*/g, "").trim(); // *drafting mentally*
      out = out.replace(/^["“'](.*)["”']$/s, "$1").trim();
    }

    const paras = out.split(/\n{2,}/).map((p) => p.trim()).filter(Boolean);
    if (paras.length > 1) {
      const kept = paras.filter((p) => !isDeliberation(p));
      out = (kept.length ? kept : [paras[paras.length - 1]]).join("\n\n");
    } else if (isDeliberation(out)) {
      // Single blob of deliberation that pivots into the answer mid-text:
      // take the last sentence group after the final planning marker.
      const sentences = out.match(/[^.!?]+[.!?]+/g) || [out];
      const firstClean = sentences.findIndex((s, i) => i > 0 && !isDeliberation(s));
      out = firstClean > -1 ? sentences.slice(firstClean).join(" ").trim() : out;
    }

    return out.trim();
  }

  /** Core chat call with model failover — shared by EVE talk and node converse. */
  async function nvidiaChatMessages(messages, modelOverride) {
    // Output contract: reasoning models will narrate their planning into
    // `content` unless the speech boundary is stated explicitly. /no_think
    // alone proved insufficient — this spells out the wire format.
    if (messages[0]?.role === "system" && !messages[0].content.includes("OUTPUT CONTRACT")) {
      messages = [
        {
          role: "system",
          content:
            `${messages[0].content}\n\n` +
            `## OUTPUT CONTRACT (absolute)\n` +
            `Your output is fed straight to a voice synthesizer and to the muscles of a ` +
            `face. It must end with the marker \`SPOKEN:\` followed by the exact words ` +
            `she says aloud — nothing after them.\n` +
            `Ideal output is the marker and the line, nothing else:\n` +
            `SPOKEN: I'm right here. What's going on?\n` +
            `Anything you write before the marker is discarded, so do not put the reply ` +
            `there. Never restate the question, describe your instructions, or count your ` +
            `own sentences. After \`SPOKEN:\` write no markdown, no quotes, no stage ` +
            `directions — only speech. /no_think`,
        },
        ...messages.slice(1),
      ];
    }
    const key =
      process.env.NVIDIA_API_KEY || process.env.NGC_API_KEY || process.env.NVAPI_KEY || "";
    if (!key) {
      return {
        ok: true,
        reply:
          "I'm with you on the Instant Presence path. Set NVIDIA_API_KEY in .env to open the live Nemotron channel.",
        model: "mock-eve",
        latencyMs: 12,
        error: null,
      };
    }
    const base = (process.env.NVIDIA_BASE_URL || "https://integrate.api.nvidia.com/v1").replace(
      /\/$/,
      "",
    );
    const models = [
      // Caller's model first; the old 70B is retired (404) — never default to it.
      modelOverride || LAB.model,
      process.env.NEMOTRON_FALLBACK_MODEL || "meta/llama-3.1-8b-instruct",
    ];
    const t0 = Date.now();
    let lastErr = "no model";
    for (const model of models) {
      try {
        const res = await fetch(`${base}/chat/completions`, {
          method: "POST",
          headers: {
            Authorization: `Bearer ${key}`,
            "Content-Type": "application/json",
            Accept: "application/json",
          },
          body: JSON.stringify({
            model,
            messages,
            temperature: LAB.temperature,
            top_p: LAB.topP,
            max_tokens: LAB.maxTokens,
            stream: false,
          }),
        });
        if (!res.ok) {
          lastErr = `${model}: HTTP ${res.status}`;
          continue;
        }
        const data = await res.json();
        const reply = sanitizeSpoken(data?.choices?.[0]?.message?.content);
        if (!reply) {
          lastErr = `${model}: empty`;
          continue;
        }
        return {
          ok: true,
          reply,
          model: data.model || model,
          latencyMs: Date.now() - t0,
          error: null,
        };
      } catch (err) {
        lastErr = `${model}: ${err?.message || err}`;
      }
    }
    report.errors += 1;
    report.lastError = lastErr;
    return {
      ok: false,
      reply:
        "I heard you — the NVIDIA path returned an error. Control plane is still live; check the key and model access.",
      model: null,
      latencyMs: Date.now() - t0,
      error: lastErr,
    };
  }

  /** EVE's own conversation thread — continuity is a persona requirement. */
  const eveChat = [];

  /** Persona + live proprioception. She feels her own body each turn. */
  function eveSystemPrompt() {
    const snap = snapshot();
    const hot = nodes.filter((id) => {
      const h = snap.nodes[id].health;
      return h === "hot" || h === "ready";
    });
    return (
      `${PERSONA.text}\n\n` +
      `## YOUR LIVE STATE (this turn)\n` +
      `- Presence stage: ${snap.stage} (${snap.stage === "L0" ? "idle presence, control plane only" : snap.stage === "L1" ? "live signal path" : "WebGPU splat render"})\n` +
      `- Control-plane latency: ${snap.controlMs}ms\n` +
      `- Warm progress: ${Math.round(snap.warmProgress * 100)}%\n` +
      `- Nodes live: ${hot.map((id) => NODE_INTEL[id].label).join(", ") || "none"}\n` +
      `- Cortex path: ${state.mode}${snap.nvidia.configured ? " · NVIDIA channel open" : ""}\n` +
      `- Mouth driver: Pipe 3 phoneme-direct (visemes from text, zero drift)`
    );
  }

  async function nvidiaChat(text) {
    const result = await nvidiaChatMessages([
      { role: "system", content: eveSystemPrompt() },
      ...eveChat,
      { role: "user", content: text },
    ]);

    // Only real replies enter memory — never mock or error text.
    if (result.ok && result.model && result.model !== "mock-eve") {
      eveChat.push({ role: "user", content: text });
      eveChat.push({ role: "assistant", content: result.reply });
      while (eveChat.length > LAB.memoryTurns * 2) eveChat.shift();
    }
    return result;
  }

  /**
   * Converse with a node about its own role. IDE-owned capability —
   * works identically over any pipeline the graph happens to be running.
   */
  async function nodeConverse(nodeId, text, wantAudio) {
    const history = nodeChats.get(nodeId) ?? [];
    const messages = [
      { role: "system", content: nodePersona(nodeId) },
      ...history,
      { role: "user", content: text },
    ];
    const result = await nvidiaChatMessages(messages, LAB.consultModel);

    if (result.ok && result.model !== "mock-eve") {
      history.push({ role: "user", content: text });
      history.push({ role: "assistant", content: result.reply });
      while (history.length > 12) history.shift();
      nodeChats.set(nodeId, history);
    }

    report.converses += 1;
    report.perNode[nodeId].converses += 1;
    logReport("converse", { nodeId, ok: result.ok, latencyMs: result.latencyMs });

    let audioB64 = null;
    let engine = null;
    if (result.ok && wantAudio) {
      const spoken = await speak(result.reply);
      if (spoken) {
        audioB64 = spoken.wav.toString("base64");
        engine = spoken.engine;
        report.visemeFramesEmitted += spoken.visemes.length;
        broadcast({
          type: "visemes",
          source: "node-converse",
          nodeId,
          tMediaMs: Date.now() - state.startedAt,
          durationMs: spoken.durationMs,
          frames: spoken.visemes,
        });
      }
    }

    broadcast({
      ...snapshot(),
      event: {
        kind: "node",
        level: result.ok ? "ok" : "warn",
        message: `${NODE_INTEL[nodeId].label}: ${result.reply.slice(0, 120)}`,
      },
    });

    return { ...result, nodeId, audioB64, engine, turns: (nodeChats.get(nodeId) ?? []).length / 2 };
  }

  async function handleTalk(text, promote = true) {
    const clean = String(text || "").trim();
    if (!clean) return { ok: false, error: "empty text", ...snapshot() };
    state.talking = true;
    if (promote && state.stage === "L0") {
      state.stage = "L1";
      state.targetStage = "L1";
      state.warmProgress = Math.max(state.warmProgress, 0.85);
    }
    broadcast(snapshot());
    const result = await nvidiaChat(clean);
    state.talking = false;
    state.lastModel = result.model;
    state.lastTalk = {
      userText: clean,
      reply: result.reply || "",
      ok: Boolean(result.ok),
      model: result.model,
      latencyMs: result.latencyMs || 0,
      error: result.error,
      at: Date.now() / 1000,
    };
    if (result.ok) {
      state.warmProgress = Math.max(state.warmProgress, 0.92);
      if (state.stage === "L0") {
        state.stage = "L1";
        state.targetStage = "L1";
      }
      // ── Pipe 3 · Phoneme-Timeline Direct ──
      // Mouth truth derives from TEXT, not from audio analysis. The timeline
      // reaches Instant Presence before any waveform exists — drift is
      // structurally zero because there is nothing to drift against.
      const { frames, durationMs } = phonemeTimeline(result.reply);
      report.visemeFramesEmitted += frames.length;
      broadcast({
        type: "visemes",
        source: "phoneme-direct",
        tMediaMs: Date.now() - state.startedAt,
        durationMs,
        frames,
      });
    }
    report.talks += 1;
    logReport("talk", { ok: Boolean(result.ok), latencyMs: result.latencyMs || 0 });
    const snap = {
      ...snapshot(),
      ok: Boolean(result.ok),
      reply: result.reply || "",
      model: result.model,
      latencyMs: result.latencyMs || 0,
      error: result.error,
    };
    broadcast(snap);
    return snap;
  }

  const sockets = new Set();
  // WO-2 T2 — Pipeline 1 audio session state, entirely in-process.
  const audioSessions = new AudioSessionRegistry();
  // WO-2 T3 re-entrancy guard — a new speech-start while a transcription
  // call for that session is still in flight must not let the stale
  // result win. Reused as-is across the Whisper pivot: the guard's logic
  // is provider-agnostic (generation counters per session id), so it
  // needed no changes when the ASR provider swapped.
  const transcribeGuard = new TranscribeSessionGuard();

  // WO-2 second pivot — Whisper needs only an API key (a plain header),
  // not a constructed SDK client object, so there is no client-construction
  // block here the way there was for AWS. Availability is just "does the
  // vault/.env have OPENAI_API_KEY," checked once at startup so a missing
  // key fails loudly at boot rather than silently on the first utterance.
  const whisperAvailable = Boolean(process.env.OPENAI_API_KEY);
  if (!whisperAvailable) {
    console.warn("[ace-controller] OPENAI_API_KEY unavailable — Pipeline 1 speech-end will report an error");
  }

  const server = createServer((req, res) => {
    const url = new URL(req.url || "/", `http://${HOST}:${PORT}`);
    const origin = req.headers.origin || "";
    const allowed = new Set([
      "http://127.0.0.1:5173",
      "http://localhost:5173",
      "http://127.0.0.1:4173",
      "http://localhost:4173",
    ]);
    if (allowed.has(origin)) res.setHeader("Access-Control-Allow-Origin", origin);
    res.setHeader("Access-Control-Allow-Methods", "GET,POST,OPTIONS");
    res.setHeader("Access-Control-Allow-Headers", "Content-Type");
    res.setHeader(
      "Access-Control-Expose-Headers",
      "X-Eve-Tts-Engine, X-Eve-Viseme-Count, X-Eve-Duration-Ms",
    );

    if (req.method === "OPTIONS") {
      res.writeHead(204);
      res.end();
      return;
    }

    if (url.pathname === "/health") {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(
        JSON.stringify({
          ok: true,
          mode: state.mode,
          stage: state.stage,
          nvidia: nvidiaKey,
        }),
      );
      return;
    }

    if (url.pathname === "/v1/status") {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify(snapshot()));
      return;
    }

    if (url.pathname === "/v1/stage" && req.method === "POST") {
      let body = "";
      req.on("data", (c) => (body += c));
      req.on("end", () => {
        try {
          const data = JSON.parse(body || "{}");
          if (stages.includes(data.stage)) {
            state.targetStage = data.stage;
            state.stage = data.stage;
            state.warmProgress = data.stage === "L0" ? 0.45 : 1;
            report.stageChanges += 1;
            logReport("stage", { stage: data.stage });
          }
          const snap = snapshot();
          broadcast(snap);
          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify(snap));
        } catch {
          res.writeHead(400, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ error: "bad json" }));
        }
      });
      return;
    }

    if (url.pathname === "/v1/talk" && req.method === "POST") {
      let body = "";
      req.on("data", (c) => (body += c));
      req.on("end", async () => {
        try {
          const data = JSON.parse(body || "{}");
          const snap = await handleTalk(data.text, data.promote !== false);
          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify(snap));
        } catch {
          res.writeHead(400, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ error: "bad json" }));
        }
      });
      return;
    }

    // ── Understand-Anything hover intel: full roster or a single node ──
    if (url.pathname === "/v1/nodes") {
      const snap = snapshot();
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(
        JSON.stringify({
          nodes: nodes.map((id) => ({
            ...NODE_INTEL[id],
            runtime: snap.nodes[id],
          })),
        }),
      );
      return;
    }

    const nodeMatch = url.pathname.match(/^\/v1\/nodes\/([a-z0-9-]+)$/);
    if (nodeMatch) {
      const intel = NODE_INTEL[nodeMatch[1]];
      if (!intel) {
        res.writeHead(404, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ error: `unknown node ${nodeMatch[1]}` }));
        return;
      }
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ ...intel, runtime: snapshot().nodes[intel.id] }));
      return;
    }

    // ── Bi-directional node conversation: consult a node about its role ──
    // POST /v1/nodes/{id}/converse {text, audio?: true}
    // The node answers in first person with live self-knowledge (health,
    // latency, stage, peers) and remembers the thread. IDE-owned: works
    // over ANY pipeline — mock, NVIDIA, or Foundry — identically.
    const converseMatch = url.pathname.match(/^\/v1\/nodes\/([a-z0-9-]+)\/converse$/);
    if (converseMatch && req.method === "POST") {
      const nodeId = converseMatch[1];
      if (!NODE_INTEL[nodeId]) {
        res.writeHead(404, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ error: `unknown node ${nodeId}` }));
        return;
      }
      let body = "";
      req.on("data", (c) => (body += c));
      req.on("end", async () => {
        try {
          const data = JSON.parse(body || "{}");
          const text = String(data.text || "").trim().slice(0, 2000);
          if (!text) {
            res.writeHead(400, { "Content-Type": "application/json" });
            res.end(JSON.stringify({ error: "text required" }));
            return;
          }
          const result = await nodeConverse(nodeId, text, data.audio !== false);
          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify(result));
        } catch {
          res.writeHead(400, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ error: "bad json" }));
        }
      });
      return;
    }

    // ── Persona Lab (ADMIN ONLY — never surfaced on the front) ──
    // GET returns the full instruction text so the lab bench can edit it;
    // POST writes it back and hot-reloads. Who she is becomes a dial.
    if (url.pathname === "/v1/persona" && req.method === "GET") {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(
        JSON.stringify({
          source: PERSONA.source,
          chars: PERSONA.chars,
          customized: PERSONA.source !== "fallback",
          text: PERSONA.text,
          memoryTurns: eveChat.length / 2,
          memoryCapacity: LAB.memoryTurns,
          nodeThreads: Object.fromEntries(
            [...nodeChats.entries()].map(([id, h]) => [id, h.length / 2]),
          ),
        }),
      );
      return;
    }

    if (url.pathname === "/v1/persona" && req.method === "POST") {
      let body = "";
      req.on("data", (c) => (body += c));
      req.on("end", () => {
        try {
          const data = JSON.parse(body || "{}");
          const text = String(data.text || "");
          if (text.trim().length < 50) {
            res.writeHead(400, { "Content-Type": "application/json" });
            res.end(JSON.stringify({ error: "persona too short — minimum 50 chars" }));
            return;
          }
          const saved = savePersona(text);
          logReport("persona-save", { chars: saved.chars });
          broadcast({
            ...snapshot(),
            event: { kind: "system", level: "ok", message: `Persona updated — ${saved.chars} chars, hot-reloaded` },
          });
          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ ok: true, chars: saved.chars, source: saved.source }));
        } catch (err) {
          res.writeHead(400, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ error: err?.message || "bad json" }));
        }
      });
      return;
    }

    // ── Realness profile: the fine-tuning surface for human feel ──
    if (url.pathname === "/v1/realness" && req.method === "GET") {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify(REALNESS ?? { error: "no profile" }));
      return;
    }

    if (url.pathname === "/v1/realness" && req.method === "POST") {
      let body = "";
      req.on("data", (c) => (body += c));
      req.on("end", () => {
        try {
          const patch = JSON.parse(body || "{}");
          // Deep-merge one level so the bench can send { speech: { jawGain } }
          const next = { ...(REALNESS || {}) };
          for (const [k, v] of Object.entries(patch)) {
            next[k] = v && typeof v === "object" && !Array.isArray(v)
              ? { ...(next[k] || {}), ...v }
              : v;
          }
          const saved = saveRealness(next);
          logReport("realness-save", { version: saved?.version });
          broadcast({
            ...snapshot(),
            event: { kind: "system", level: "ok", message: `Realness profile updated — ${saved?.name}` },
          });
          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ ok: true, profile: saved }));
        } catch (err) {
          res.writeHead(400, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ error: err?.message || "bad json" }));
        }
      });
      return;
    }

    // ── Lab settings: the dials that shape her without touching code ──
    if (url.pathname === "/v1/settings" && req.method === "GET") {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ ...LAB, mode: state.mode, nvidia: nvidiaKey }));
      return;
    }

    if (url.pathname === "/v1/settings" && req.method === "POST") {
      let body = "";
      req.on("data", (c) => (body += c));
      req.on("end", () => {
        try {
          const d = JSON.parse(body || "{}");
          if (typeof d.model === "string" && d.model.trim()) LAB.model = d.model.trim();
          if (Number.isFinite(d.temperature)) LAB.temperature = Math.min(2, Math.max(0, d.temperature));
          if (Number.isFinite(d.topP)) LAB.topP = Math.min(1, Math.max(0.01, d.topP));
          if (Number.isFinite(d.maxTokens)) LAB.maxTokens = Math.min(2048, Math.max(32, Math.round(d.maxTokens)));
          if (Number.isFinite(d.memoryTurns)) LAB.memoryTurns = Math.min(40, Math.max(0, Math.round(d.memoryTurns)));
          logReport("settings", { ...LAB });
          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ ok: true, ...LAB }));
        } catch {
          res.writeHead(400, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ error: "bad json" }));
        }
      });
      return;
    }

    // Reset conversational memory (EVE + all node threads)
    if (url.pathname === "/v1/memory/reset" && req.method === "POST") {
      eveChat.length = 0;
      nodeChats.clear();
      logReport("memory-reset", {});
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ ok: true, cleared: true }));
      return;
    }

    // ── Reporting spine: session document covering both sides of the IDE ──
    if (url.pathname === "/v1/report") {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(
        JSON.stringify({
          service: "eve-ecc-ace-controller",
          mode: state.mode,
          stage: state.stage,
          uptimeMs: Date.now() - report.startedAt,
          persona: {
            source: PERSONA.source,
            customized: PERSONA.source !== "fallback",
            memoryTurns: eveChat.length / 2,
          },
          totals: {
            talks: report.talks,
            speaks: report.speaks,
            converses: report.converses,
            visemeFramesEmitted: report.visemeFramesEmitted,
            stageChanges: report.stageChanges,
            errors: report.errors,
          },
          lastError: report.lastError,
          perNode: report.perNode,
          log: report.log.slice(-50),
        }),
      );
      return;
    }

    // ── Asset optimizer sink (admin/lab only) ──
    // The browser can re-encode a heavy still far faster than this box can,
    // so it hands the result back here to be written. Name is sanitized and
    // the directory is pinned — nothing escapes public/staff/.
    if (url.pathname === "/v1/asset" && req.method === "POST") {
      let body = "";
      req.on("data", (c) => {
        body += c;
        if (body.length > 24 * 1024 * 1024) req.destroy();
      });
      req.on("end", () => {
        try {
          const data = JSON.parse(body || "{}");
          const safe = String(data.name || "").replace(/[^a-zA-Z0-9._-]/g, "");
          if (!safe || !/\.(webp|jpg|jpeg|png)$/i.test(safe)) {
            res.writeHead(400, { "Content-Type": "application/json" });
            res.end(JSON.stringify({ error: "bad asset name" }));
            return;
          }
          const m = String(data.dataUrl || "").match(/^data:image\/[a-z+]+;base64,(.+)$/s);
          if (!m) {
            res.writeHead(400, { "Content-Type": "application/json" });
            res.end(JSON.stringify({ error: "expected image data URL" }));
            return;
          }
          const buf = Buffer.from(m[1], "base64");
          const dir = join(root, "..", "..", "apps", "web", "public", "staff");
          mkdirSync(dir, { recursive: true });
          writeFileSync(join(dir, safe), buf);
          logReport("asset-write", { name: safe, bytes: buf.length });
          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ ok: true, name: safe, bytes: buf.length }));
        } catch (err) {
          res.writeHead(400, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ error: err?.message || "bad json" }));
        }
      });
      return;
    }

    // ── Avatar gallery data source: completed + under-development stills ──
    if (url.pathname === "/v1/gallery") {
      const staffDir = join(root, "..", "..", "apps", "web", "public", "staff");
      let entries = [];
      try {
        entries = readdirSync(staffDir)
          .filter((f) => /\.(png|jpe?g|webp|avif)$/i.test(f))
          .map((f) => ({
            file: f,
            url: `/staff/${f}`,
            name: f.replace(/\.[^.]+$/, "").replace(/[-_]/g, " "),
            status: /natural|closeup/i.test(f) ? "completed" : "under-development",
          }));
      } catch {
        /* dir missing → empty gallery */
      }
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ avatars: entries, count: entries.length }));
      return;
    }

    // ── Node voice activation: the architecture explains itself aloud ──
    // POST {nodeId} speaks that node's role; POST {text} speaks free text.
    // Returns WAV; the matching Pipe 3 viseme timeline broadcasts over /ws
    // so the mouth moves on the same clock the audio plays on.
    if (url.pathname === "/v1/speak" && req.method === "POST") {
      let body = "";
      req.on("data", (c) => (body += c));
      req.on("end", async () => {
        try {
          const data = JSON.parse(body || "{}");
          const intel = data.nodeId ? NODE_INTEL[data.nodeId] : null;
          const text = intel ? intel.spoken : String(data.text || "").trim();
          if (!text) {
            res.writeHead(400, { "Content-Type": "application/json" });
            res.end(JSON.stringify({ error: "nodeId or text required" }));
            return;
          }
          const spoken = await speak(text, intel ? `node:${intel.id}` : undefined);
          if (!spoken) {
            res.writeHead(503, { "Content-Type": "application/json" });
            res.end(JSON.stringify({ error: "no TTS engine available" }));
            return;
          }
          report.speaks += 1;
          report.visemeFramesEmitted += spoken.visemes.length;
          if (intel) report.perNode[intel.id].speaks += 1;
          logReport("speak", { nodeId: intel?.id ?? "freetext", engine: spoken.engine });
          broadcast({
            type: "visemes",
            source: "node-voice",
            nodeId: intel?.id ?? null,
            tMediaMs: Date.now() - state.startedAt,
            durationMs: spoken.durationMs,
            frames: spoken.visemes,
          });
          res.writeHead(200, {
            "Content-Type": "audio/wav",
            "Content-Length": spoken.wav.length,
            "X-Eve-Tts-Engine": spoken.engine,
            "X-Eve-Viseme-Count": String(spoken.visemes.length),
            "X-Eve-Duration-Ms": String(spoken.durationMs),
          });
          res.end(spoken.wav);
        } catch {
          res.writeHead(400, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ error: "bad json" }));
        }
      });
      return;
    }

    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(
      JSON.stringify({
        service: "eve-ecc-ace-controller",
        mode: state.mode,
        pipe3: "phoneme-direct",
        endpoints: [
          "/health",
          "/v1/status",
          "/v1/stage",
          "/v1/talk",
          "/v1/nodes",
          "/v1/nodes/{id}",
          "/v1/nodes/{id}/converse",
          "/v1/speak",
          "/v1/persona",
          "/v1/settings",
          "/v1/memory/reset",
          "/v1/report",
          "/v1/gallery",
          "/ws",
        ],
      }),
    );
  });

  const wss = new WebSocketServer({ server, path: "/ws" });

  wss.on("connection", (ws) => {
    sockets.add(ws);
    state.clients = sockets.size;
    // WO-2 T2 — each connection gets its own in-memory audio session,
    // keyed by a random id (never derived from anything the client sends).
    const audioSessionId = randomId();
    ws.send(JSON.stringify(snapshot()));
    ws.on("message", async (raw) => {
      try {
        const msg = JSON.parse(String(raw));
        if (msg.type === "stage" && stages.includes(msg.stage)) {
          state.stage = msg.stage;
          state.targetStage = msg.stage;
          state.warmProgress = msg.stage === "L0" ? 0.45 : 1;
          broadcast(snapshot());
        }
        if (msg.type === "ping") {
          ws.send(JSON.stringify({ type: "pong", t: Date.now() }));
        }
        if (msg.type === "talk" && msg.text) {
          await handleTalk(msg.text, msg.promote !== false);
        }
        // Accept both control-message names for the end-of-utterance
        // signal: "speech-end" (this module's original naming) and
        // "vad_stop" (the browser VAD's emitted event name per the Lead
        // Architect's T2 spec) — same handler either way, so whichever the
        // client actually sends, it works without a client/server naming
        // mismatch blocking the pipeline.
        if (msg.type === "speech-end" || msg.type === "vad_stop") {
          const session = audioSessions.get(audioSessionId);
          const buffered = session ? session.bufferedSamples() : 0;
          console.log(
            `[ace-controller] ${msg.type}: session ${audioSessionId} has ${buffered} buffered samples`,
          );

          if (!whisperAvailable) {
            ws.send(
              JSON.stringify({
                type: "turn-error",
                error: "OpenAI API key unavailable — check vault credentials",
              }),
            );
            return;
          }
          if (!session || buffered === 0) {
            return; // nothing to transcribe
          }

          // WO-2 re-entrancy: a new speech-end for this session supersedes
          // any still-in-flight call from a previous one.
          const myToken = transcribeGuard.begin(audioSessionId);
          const t0 = Date.now();
          const samples = session.drain();

          // DOUBLE STRATEGIC PIVOT (Rule 5) — both AWS legs of Pipeline 1
          // are account-locked on this new AWS account, confirmed by
          // reproducing UnrecognizedClientException directly against AWS
          // outside this server (Bedrock's Converse API, and Transcribe
          // Streaming at every payload size tested) — not a code defect.
          // bedrockRouter.mjs and transcribeBridge.mjs are both left
          // completely intact, unused, for reactivation once AWS clears:
          //   - "Nemotron Agent" cognitive-core slot: Bedrock -> NVIDIA NIM
          //     (nvidiaChat/nvidiaChatMessages, already live for /v1/talk)
          //   - "Riva ASR" slot: Amazon Transcribe Streaming -> OpenAI
          //     Whisper REST (whisperBridge.mjs)
          // Real accepted tradeoff from the Whisper swap: no partial
          // transcripts. transcribeGuard.isCurrent() checks below are kept
          // anyway — they still guard against a stale FINAL result racing
          // a newer speech-end, which can happen with any single-shot ASR
          // call, not just the streaming one this replaced.
          let transcribeResult;
          try {
            transcribeResult = await transcribeUtteranceWhisper({
              samples,
              sampleRateHz: 16_000,
              apiKey: process.env.OPENAI_API_KEY,
            });
          } catch (err) {
            transcribeResult = { text: "", error: err?.message || String(err) };
          }
          const tTranscribeFinal = Date.now() - t0;

          if (!transcribeGuard.isCurrent(audioSessionId, myToken)) {
            console.log(
              `[ace-controller] session ${audioSessionId}: dropping stale turn result (superseded by newer speech-end)`,
            );
            return;
          }

          if (transcribeResult.error) {
            console.warn(`[ace-controller] session ${audioSessionId} transcribe failed: ${transcribeResult.error}`);
            ws.send(JSON.stringify({ type: "turn-error", error: transcribeResult.error }));
            return;
          }

          const transcript = transcribeResult.text;
          if (!transcript.trim()) {
            return; // silence or unintelligible — no turn to complete
          }

          const nvidiaResult = await nvidiaChat(transcript);
          const tTurnComplete = Date.now() - t0;

          if (!transcribeGuard.isCurrent(audioSessionId, myToken)) {
            console.log(
              `[ace-controller] session ${audioSessionId}: dropping stale NVIDIA result (superseded by newer speech-end)`,
            );
            return;
          }

          if (!nvidiaResult.ok) {
            console.warn(`[ace-controller] session ${audioSessionId} NVIDIA chat failed: ${nvidiaResult.error}`);
            ws.send(JSON.stringify({ type: "turn-error", error: nvidiaResult.error || "NVIDIA chat failed" }));
            return;
          }

          const result = {
            transcript,
            reply: nvidiaResult.reply,
            modelId: nvidiaResult.model,
            timestamps: { tRingWrite: 0, tTranscribeFinal, tTurnComplete },
          };

          console.log(
            `[ace-controller] session ${audioSessionId} TurnComplete (Whisper -> NVIDIA NIM): ` +
              `transcript=${JSON.stringify(result.transcript)} reply=${JSON.stringify(result.reply)} ` +
              `model=${result.modelId} timestamps=${JSON.stringify(result.timestamps)}`,
          );

          // Mirror handleTalk's viseme broadcast so Pipeline 1's response
          // drives EVE's mouth the same way /v1/talk already does.
          const { frames, durationMs } = phonemeTimeline(result.reply);
          broadcast({
            type: "visemes",
            source: "pipeline-1",
            tMediaMs: Date.now() - state.startedAt,
            durationMs,
            frames,
          });

          ws.send(
            JSON.stringify({
              type: "turn-complete",
              transcript: result.transcript,
              reply: result.reply,
              modelId: result.modelId,
              timestamps: result.timestamps,
            }),
          );
        }
      } catch {
        /* ignore */
      }
    });
    // WO-2 T2 — binary frames are raw PCM audio (Float32, 16 kHz mono),
    // never anything else on this connection. Accumulate in-process; never
    // touch /dev/shm/miranda_bus here (see Architectural resolution #2 in
    // .kiro/specs/wo2-acoustic-ingress-routing/tasks.md).
    ws.on("binary", (buf) => {
      try {
        const session = audioSessions.getOrCreate(audioSessionId);
        const sampleCount = session.pushFrame(buf);
        console.log(
          `[ace-controller] audio frame: session ${audioSessionId} +${sampleCount} samples ` +
            `(total buffered: ${session.bufferedSamples()})`,
        );
      } catch (err) {
        console.warn(`[ace-controller] bad audio frame from ${audioSessionId}: ${err?.message || err}`);
      }
    });
    ws.on("close", () => {
      sockets.delete(ws);
      state.clients = sockets.size;
      audioSessions.remove(audioSessionId);
    });
  });

  // gentle warm simulation
  setInterval(() => {
    if (state.stage === "L0" && state.warmProgress < 0.7) {
      state.warmProgress = Math.min(0.7, state.warmProgress + 0.01);
    }
    if (nvidiaKey && state.warmProgress < 0.55) {
      state.warmProgress = Math.min(0.55, state.warmProgress + 0.02);
    }
    broadcast(snapshot());
  }, 1000);

  server.listen(PORT, HOST, () => {
    console.log(`[ace-controller] ready  http://${HOST}:${PORT}`);
    console.log(`[ace-controller] ws     ws://${HOST}:${PORT}/ws`);
    console.log(`[ace-controller] nvidia ${nvidiaKey ? "key-present" : "key-missing"}`);
  });
}

// Prefer Python if uvicorn import works; else Node mock (no extra deps).
const py = tryPython();
if (py) {
  const probe = spawn(py, ["-c", "import uvicorn, fastapi"], {
    cwd: root,
    stdio: "ignore",
    shell: isWin,
  });
  probe.on("exit", (code) => {
    if (code === 0) {
      const child = startPython(py);
      if (!child) startNodeMock();
    } else {
      console.log("[ace-controller] FastAPI/uvicorn not installed — Node mock");
      startNodeMock();
    }
  });
} else {
  startNodeMock();
}
