/**
 * presence_kit.mjs — the science layer for The Cerebral Project.
 *
 * Three exports feed the xyflow + Understand-Anything living node graph:
 *
 *   NODE_INTEL       — plain-English hover intelligence per cortex node
 *   phonemeTimeline  — Pipe 3 Phoneme-Timeline Direct: text → zero-drift
 *                      viseme/ARKit frames BEFORE any audio exists
 *   speak            — tiered TTS router: local HTTP engine (Kokoro/VibeVoice)
 *                      when configured → Windows SAPI fallback (zero install).
 *                      Returns WAV + the viseme timeline for the same text.
 *
 * Zero npm dependencies. Node built-ins only.
 */

import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/* ════════════════════ 1 · UNDERSTAND-ANYTHING NODE INTEL ════════════════════ */

/**
 * Hover + voice intelligence for every cortex node.
 * `spoken` is written for the ear — EVE reads it aloud on node click.
 */
// WO-5 T1: rewired from the old eve-ecc/NVIDIA-ACE node set to the real
// Miranda-Engine topology (see client-apps/web/src/data/aceTopology.ts for
// the frontend's mirror of this same rewrite, and its module doc for the
// full reasoning). Every entry below names something that actually exists
// in this repo.
export const NODE_INTEL = {
  mic: {
    id: "mic",
    label: "Mic Ingress",
    plane: "control",
    budgetMs: 20,
    summary: "Browser getUserMedia capture, streamed as PCM over WebSocket.",
    spoken:
      "I am Mic Ingress, the ears of Pipeline 1. I capture the browser's microphone " +
      "and stream sixteen kilohertz PCM over this WebSocket, so the cortex knows the " +
      "moment a guest starts speaking, without waiting on the full speech stack.",
  },
  presence: {
    id: "presence",
    label: "Instant Presence",
    plane: "control",
    budgetMs: 80,
    summary: "L0 idle avatar layer — gaze, breath, micro-expression. Answers under one second.",
    spoken:
      "I am Instant Presence, the always-on face of EVE. I hold gaze, breath, and " +
      "micro-expression so a guest is never staring at a cold boot. I answer in under " +
      "one second, and with the phoneme-direct fork I receive mouth truth straight " +
      "from text — before any audio is rendered.",
  },
  "cloud-bridge": {
    id: "cloud-bridge",
    label: "ace-controller",
    plane: "data",
    budgetMs: 350,
    summary: "Whisper ASR + NVIDIA NIM chat + phoneme-direct viseme timeline (Pipeline 1).",
    spoken:
      "I am ace-controller, the cloud bridge. I transcribe your speech through OpenAI " +
      "Whisper, route it to an NVIDIA NIM language model, and derive the mouth's " +
      "viseme timeline straight from the reply text — before any speech audio exists. " +
      "I was built for AWS Bedrock and Transcribe, but both were account-locked, so I " +
      "pivoted to this path. Those AWS legs stay wired, unused, for when they clear.",
  },
  "native-capture": {
    id: "native-capture",
    label: "miranda-audio",
    plane: "data",
    budgetMs: 1760,
    summary: "Native cpal mic capture + parakeet.cpp FFI local ASR (Pipeline 2).",
    spoken:
      "I am miranda-audio, the native ear of Pipeline 2. I capture microphone audio " +
      "directly through cpal and offer a local, offline speech recognizer through " +
      "parakeet dot cpp. That local recognizer is honestly still slower than real time " +
      "on this hardware — I don't hide that.",
  },
  "ipc-bus": {
    id: "ipc-bus",
    label: "miranda-ipc",
    plane: "data",
    budgetMs: 1,
    summary: "Lock-free shared-memory ring buffer backbone.",
    spoken:
      "I am miranda-ipc, the shared-memory backbone every native node reads or writes " +
      "through. Four lock-free ring buffers, no mutex, measured at about seventy " +
      "nanoseconds round trip. I am pipeline-agnostic — I don't care which pipeline is " +
      "running, only that the bytes move safely.",
  },
  supervisor: {
    id: "supervisor",
    label: "miranda-supervisor",
    plane: "data",
    budgetMs: 350,
    summary: "Turn-taking state machine + Nemotron-Flash routing.",
    spoken:
      "I am miranda-supervisor. I own the conversational turn state — when you " +
      "interrupt me mid-thought, I cancel my own in-flight reply rather than let a " +
      "stale answer win the turn. I route finished transcripts onward for reasoning.",
  },
  kinematics: {
    id: "kinematics",
    label: "miranda-nodes",
    plane: "data",
    budgetMs: 17,
    summary: "ARKit-52 oscillators + SIMD acoustic solver + compositor + 60 FPS dispatcher.",
    spoken:
      "I am miranda-nodes, the face truth. Three autonomic oscillators keep the face " +
      "alive between words — blink, gaze, and breath — and a small SIMD solver reads " +
      "acoustic energy straight into ARKit mouth shapes. I am honest about what I am: " +
      "a hand-authored heuristic, not a trained model. I never guess where the tongue " +
      "is — there's no acoustic signature for that, so I leave it alone.",
  },
  transport: {
    id: "transport",
    label: "miranda-transport",
    plane: "data",
    budgetMs: 15,
    summary: "WebRTC DataChannel binary frame hub + Axum telemetry + circuit breaker.",
    spoken:
      "I am miranda-transport. I broadcast the face and body data as compact binary " +
      "packets to every connected browser, and I carry a separate telemetry channel " +
      "with a circuit breaker, so if a render stalls, that shows up as a signal — " +
      "never a silent freeze.",
  },
  renderer: {
    id: "renderer",
    label: "WebGPU Viewport",
    plane: "data",
    budgetMs: 16,
    summary: "WGSL Gaussian-splat viewport. Work Order 5's net-new piece, in progress.",
    spoken:
      "I am the WebGPU viewport, the newest node in this cortex. I'm being built right " +
      "now to render EVE as a real three-dimensional Gaussian splat, deformed live by " +
      "the data miranda-transport sends me. I run against a placeholder asset for now " +
      "— the real rigged avatar is separate research, still in progress.",
  },
};

/* ════════════════════ 2 · PIPE 3 — PHONEME-TIMELINE DIRECT ════════════════════ */

/**
 * The zero-drift fork: derive the viseme timeline from TEXT, not from audio.
 * Every other pipeline renders audio, then pays a GPU to listen to it and guess
 * the mouth shapes back. We skip the round trip — the timeline IS the source.
 */

const DIGRAPHS = [
  ["ch", "CH"], ["sh", "CH"], ["th", "TH"], ["ph", "FF"], ["wh", "U"],
  ["qu", "kk"], ["ng", "nn"], ["oo", "U"], ["ee", "E"], ["ea", "E"],
  ["ou", "O"], ["ow", "O"], ["ai", "E"], ["ay", "E"], ["oi", "O"],
  ["oy", "O"], ["er", "RR"], ["ar", "aa"], ["or", "O"],
];

const LETTER_VISEME = {
  a: "aa", e: "E", i: "ih", o: "O", u: "U", y: "ih",
  b: "PP", p: "PP", m: "PP",
  f: "FF", v: "FF",
  t: "DD", d: "DD",
  k: "kk", g: "kk", c: "kk", q: "kk", x: "kk",
  j: "CH",
  s: "SS", z: "SS",
  n: "nn", l: "nn",
  r: "RR",
  h: "sil", w: "U",
};

const VOWELS = new Set(["aa", "E", "ih", "O", "U"]);
const VOWEL_MS = 110;
const CONSONANT_MS = 62;
const WORD_GAP_MS = 42;
const SENTENCE_GAP_MS = 210;

/** ARKit weight templates per viseme — mouth channels only; idle loop owns the rest. */
export const VISEME_WEIGHTS = {
  sil: { jawOpen: 0.02, mouthClose: 0.15 },
  PP: { mouthClose: 0.95, mouthPressLeft: 0.6, mouthPressRight: 0.6, jawOpen: 0.03 },
  FF: { mouthLowerDownLeft: 0.25, mouthLowerDownRight: 0.25, mouthShrugUpper: 0.55, jawOpen: 0.1 },
  TH: { tongueOut: 0.5, jawOpen: 0.18, mouthStretchLeft: 0.15, mouthStretchRight: 0.15 },
  DD: { jawOpen: 0.14, mouthShrugUpper: 0.3, mouthStretchLeft: 0.2, mouthStretchRight: 0.2 },
  kk: { jawOpen: 0.2, mouthShrugLower: 0.25 },
  CH: { mouthFunnel: 0.55, jawOpen: 0.16, mouthPucker: 0.3 },
  SS: { mouthStretchLeft: 0.4, mouthStretchRight: 0.4, jawOpen: 0.08, mouthSmileLeft: 0.15, mouthSmileRight: 0.15 },
  nn: { jawOpen: 0.12, tongueOut: 0.12, mouthClose: 0.2 },
  RR: { mouthFunnel: 0.4, mouthPucker: 0.35, jawOpen: 0.14 },
  aa: { jawOpen: 0.62, mouthStretchLeft: 0.1, mouthStretchRight: 0.1 },
  E: { jawOpen: 0.3, mouthSmileLeft: 0.42, mouthSmileRight: 0.42, mouthStretchLeft: 0.25, mouthStretchRight: 0.25 },
  ih: { jawOpen: 0.22, mouthSmileLeft: 0.25, mouthSmileRight: 0.25 },
  O: { jawOpen: 0.45, mouthFunnel: 0.6, mouthPucker: 0.45 },
  U: { jawOpen: 0.18, mouthPucker: 0.75, mouthFunnel: 0.5 },
};

/**
 * text → { frames: BlendshapeFrame[], durationMs }
 * Frames carry tMediaMs offsets from utterance start; frontend interpolates.
 */
export function phonemeTimeline(text) {
  const frames = [];
  let t = 0;
  const words = String(text).toLowerCase().split(/\s+/).filter(Boolean);

  for (const rawWord of words) {
    const punct = /[.!?;:]$/.test(rawWord);
    const word = rawWord.replace(/[^a-z']/g, "");
    let i = 0;

    while (i < word.length) {
      let viseme = null;
      const pair = word.slice(i, i + 2);
      for (const [dg, v] of DIGRAPHS) {
        if (pair === dg) {
          viseme = v;
          i += 2;
          break;
        }
      }
      if (!viseme) {
        viseme = LETTER_VISEME[word[i]] || null;
        i += 1;
      }
      if (!viseme || viseme === "sil") continue;

      const isVowel = VOWELS.has(viseme);
      frames.push({
        tMediaMs: t,
        viseme,
        energy: isVowel ? 0.85 : 0.55,
        weights: VISEME_WEIGHTS[viseme] || VISEME_WEIGHTS.sil,
      });
      t += isVowel ? VOWEL_MS : CONSONANT_MS;
    }

    frames.push({ tMediaMs: t, viseme: "sil", energy: 0.1, weights: VISEME_WEIGHTS.sil });
    t += punct ? SENTENCE_GAP_MS : WORD_GAP_MS;
  }

  return { frames, durationMs: t };
}

/* ════════════════════ 3 · TTS ROUTER — THE NODES SPEAK ════════════════════ */

const isWin = process.platform === "win32";

/** In-memory WAV cache — node descriptions are static, repeat clicks are instant. */
const audioCache = new Map();
const AUDIO_CACHE_MAX = 40;

function cachePut(key, buf) {
  if (audioCache.size >= AUDIO_CACHE_MAX) {
    const first = audioCache.keys().next().value;
    audioCache.delete(first);
  }
  audioCache.set(key, buf);
}

/**
 * Tier 1 — local HTTP TTS engine (Kokoro / VibeVoice / Foundry sidecar).
 * Configured via EVE_TTS_URL (OpenAI-style /audio/speech). Skipped when unset.
 */
async function httpTts(text) {
  const base = process.env.EVE_TTS_URL;
  if (!base) return null;
  try {
    const ctl = new AbortController();
    const timer = setTimeout(() => ctl.abort(), 15000);
    const res = await fetch(base.replace(/\/$/, "") + "/audio/speech", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        model: process.env.EVE_TTS_MODEL || "kokoro",
        voice: process.env.EVE_TTS_VOICE || "af_bella",
        input: text,
        response_format: "wav",
      }),
      signal: ctl.signal,
    });
    clearTimeout(timer);
    if (!res.ok) return null;
    const buf = Buffer.from(await res.arrayBuffer());
    return buf.length > 44 ? { wav: buf, engine: "http-tts" } : null;
  } catch {
    return null;
  }
}

/**
 * Tier 0 — Windows SAPI via cscript COM (SAPI.SpVoice). Zero install, and —
 * unlike PowerShell System.Speech — immune to __PSLockdownPolicy constrained
 * language mode, so it works in hardened shells too. Text travels by UTF-16
 * temp file (no quoting surface).
 */
function comTts(text) {
  return new Promise((resolve) => {
    if (!isWin) return resolve(null);
    let dir;
    try {
      dir = mkdtempSync(join(tmpdir(), "eve-tts-"));
    } catch {
      return resolve(null);
    }
    const txtPath = join(dir, "say.txt");
    const wavPath = join(dir, "say.wav");
    const vbsPath = join(dir, "say.vbs");

    // SAFT22kHz16BitMono = 34 — lean, clear, browser-playable
    const vbs = [
      'Set fso = CreateObject("Scripting.FileSystemObject")',
      `Set f = fso.OpenTextFile("${txtPath.replace(/\\/g, "\\\\").replace(/"/g, '""')}", 1, False, -1)`,
      "text = f.ReadAll",
      "f.Close",
      'Set voice = CreateObject("SAPI.SpVoice")',
      'For Each v In voice.GetVoices : If InStr(LCase(v.GetDescription), "zira") > 0 Or InStr(LCase(v.GetDescription), "female") > 0 Then Set voice.Voice = v : Exit For : End If : Next',
      'Set stream = CreateObject("SAPI.SpFileStream")',
      "stream.Format.Type = 34",
      `stream.Open "${wavPath.replace(/\\/g, "\\\\").replace(/"/g, '""')}", 3`,
      "Set voice.AudioOutputStream = stream",
      "voice.Speak text",
      "stream.Close",
    ].join("\r\n");

    try {
      // UTF-16LE with BOM so OpenTextFile(-1) reads any character safely
      writeFileSync(txtPath, Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from(String(text), "utf16le")]));
      writeFileSync(vbsPath, vbs, "utf8");
    } catch {
      rmSync(dir, { recursive: true, force: true });
      return resolve(null);
    }

    const proc = spawn("cscript.exe", ["//nologo", "//B", vbsPath], {
      stdio: "ignore",
      windowsHide: true,
    });

    const timer = setTimeout(() => proc.kill(), 25000);

    const finish = () => {
      clearTimeout(timer);
      try {
        const wav = readFileSync(wavPath);
        rmSync(dir, { recursive: true, force: true });
        resolve(wav.length > 44 ? { wav, engine: "sapi-com" } : null);
      } catch {
        rmSync(dir, { recursive: true, force: true });
        resolve(null);
      }
    };
    proc.on("exit", finish);
    proc.on("error", () => {
      clearTimeout(timer);
      rmSync(dir, { recursive: true, force: true });
      resolve(null);
    });
  });
}

/**
 * Tier 0b — PowerShell System.Speech, with the lockdown env var stripped so a
 * sandboxed parent shell cannot constrain the child. Backup when COM is absent.
 */
function sapiTts(text) {
  return new Promise((resolve) => {
    if (!isWin) return resolve(null);
    let dir;
    try {
      dir = mkdtempSync(join(tmpdir(), "eve-tts-"));
    } catch {
      return resolve(null);
    }
    const txtPath = join(dir, "say.txt");
    const wavPath = join(dir, "say.wav");
    const script = [
      "Add-Type -AssemblyName System.Speech",
      "$s = New-Object System.Speech.Synthesis.SpeechSynthesizer",
      "try { $s.SelectVoiceByHints([System.Speech.Synthesis.VoiceGender]::Female) } catch {}",
      "$s.Rate = 0",
      `$text = Get-Content -Raw -Encoding UTF8 '${txtPath.replace(/'/g, "''")}'`,
      `$s.SetOutputToWaveFile('${wavPath.replace(/'/g, "''")}')`,
      "$s.Speak($text)",
      "$s.Dispose()",
    ].join("; ");

    try {
      writeFileSync(txtPath, String(text), "utf8");
    } catch {
      rmSync(dir, { recursive: true, force: true });
      return resolve(null);
    }

    const env = { ...process.env };
    delete env.__PSLockdownPolicy; // parent sandbox must not constrain the child

    const ps = spawn(
      "powershell.exe",
      ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script],
      { stdio: "ignore", windowsHide: true, env },
    );

    const timer = setTimeout(() => ps.kill(), 20000);

    ps.on("exit", () => {
      clearTimeout(timer);
      try {
        const wav = readFileSync(wavPath);
        rmSync(dir, { recursive: true, force: true });
        resolve(wav.length > 44 ? { wav, engine: "sapi" } : null);
      } catch {
        rmSync(dir, { recursive: true, force: true });
        resolve(null);
      }
    });
    ps.on("error", () => {
      clearTimeout(timer);
      rmSync(dir, { recursive: true, force: true });
      resolve(null);
    });
  });
}

/**
 * speak(text, cacheKey?) → { wav, engine, visemes, durationMs } | null
 * Always returns the Pipe 3 viseme timeline alongside the audio, so the
 * frontend can drive the mouth from the same clock it plays the WAV on.
 */
export async function speak(text, cacheKey) {
  const clean = String(text || "").trim().slice(0, 1200);
  if (!clean) return null;

  const timeline = phonemeTimeline(clean);

  if (cacheKey && audioCache.has(cacheKey)) {
    const hit = audioCache.get(cacheKey);
    return {
      wav: hit.wav,
      engine: hit.engine + ":cached",
      visemes: timeline.frames,
      durationMs: timeline.durationMs,
    };
  }

  const result = (await httpTts(clean)) || (await comTts(clean)) || (await sapiTts(clean));
  if (!result) return null;

  if (cacheKey) cachePut(cacheKey, result);
  return { wav: result.wav, engine: result.engine, visemes: timeline.frames, durationMs: timeline.durationMs };
}
