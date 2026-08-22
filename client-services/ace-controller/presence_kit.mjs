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
export const NODE_INTEL = {
  mic: {
    id: "mic",
    label: "Mic Ingress",
    plane: "control",
    budgetMs: 20,
    summary: "Local capture + VAD gate. Control-plane entry.",
    spoken:
      "I am Mic Ingress, the ears of Instant Presence. I capture local audio and run " +
      "voice activity detection, so the cortex knows the moment a guest starts speaking, " +
      "without waiting on the full speech stack.",
  },
  presence: {
    id: "presence",
    label: "Instant Presence",
    plane: "control",
    budgetMs: 80,
    summary: "L0 idle avatar — gaze, breath, micro-expression. Answers under one second.",
    spoken:
      "I am Instant Presence, the always-on face of EVE. I hold gaze, breath, and " +
      "micro-expression so a guest is never staring at a cold boot. I answer in under " +
      "one second, and with the phoneme-direct fork I now receive mouth truth straight " +
      "from text — before any audio is rendered.",
  },
  syncer: {
    id: "syncer",
    label: "Spatial Syncer",
    plane: "control",
    budgetMs: 8,
    summary: "Stage bus + media clock. Couples cortex to studio.",
    spoken:
      "I am the Spatial Syncer, the heartbeat of the stage. I keep blendshapes, intent, " +
      "and pixels on a single timeline, so face, voice, and body never drift apart.",
  },
  "riva-asr": {
    id: "riva-asr",
    label: "Riva ASR",
    plane: "data",
    budgetMs: 180,
    summary: "Streaming speech recognition — speech becomes tokens.",
    spoken:
      "I am the speech recognizer. I turn the live microphone stream into text tokens " +
      "the agent can reason over. On the edge path I run as Nemotron speech streaming " +
      "on pure CPU — no GPU required.",
  },
  nemotron: {
    id: "nemotron",
    label: "Nemotron Agent",
    plane: "data",
    budgetMs: 350,
    summary: "Reasoning + dialogue policy. Decides what EVE says next.",
    spoken:
      "I am the Nemotron agent, the cognitive core. I hold dialogue policy and intent. " +
      "I decide what EVE says next, and I signal the syncer so the face, the voice, and " +
      "the stage stay perfectly aligned.",
  },
  "riva-tts": {
    id: "riva-tts",
    label: "Riva TTS",
    plane: "data",
    budgetMs: 160,
    summary: "Neural text-to-speech stream.",
    spoken:
      "I am the voice. I turn the agent's words into a neural speech stream. In the " +
      "phoneme-direct architecture, my sibling fork hands the mouth its timeline before " +
      "I even finish rendering the waveform — so the lips never chase the audio.",
  },
  a2f: {
    id: "a2f",
    label: "Audio2Face-3D",
    plane: "data",
    budgetMs: 40,
    summary: "ARKit 52-channel blendshapes — face truth.",
    spoken:
      "I am Audio2Face. I emit true ARKit fifty-two channel blendshapes — not a generic " +
      "mesh warp. I am the face truth that Live Studio consumes.",
  },
  animgraph: {
    id: "animgraph",
    label: "AnimGraph",
    plane: "data",
    budgetMs: 33,
    summary: "Body + gesture graph driven by intent and prosody.",
    spoken:
      "I am AnimGraph, the body language. Driven by agent intent and prosody, I make " +
      "presence feel embodied — not just lip-synced.",
  },
  omniverse: {
    id: "omniverse",
    label: "Omniverse Stream",
    plane: "data",
    budgetMs: 50,
    summary: "L2 cinematic pixel takeover. Never a boot blocker.",
    spoken:
      "I am the Omniverse pixel stream, the cinematic takeover. I am deliberately " +
      "optional at boot — Instant Presence never waits on a full render to greet a guest.",
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
