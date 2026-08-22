import { create } from "zustand";
import { ACE_NODES } from "@/data/aceTopology";
import { synthesizeArkitFrame } from "@/lib/ace/arkit";
import { aceController, type AceSnapshot, type BusLinkState } from "@/lib/ace/controllerClient";
import { createBusEvent, globalSyncer } from "@/lib/ace/spatialSyncer";
import { realness } from "@/lib/metrics/realness";
import {
  controlPlaneReady,
  getContract,
  healthForStageRole,
  latencyForHealth,
  nextStage,
  prevStage,
  stageReadiness,
} from "@/lib/stageMachine/transitions";
import type {
  BlendshapeFrame,
  BusEvent,
  MediaClockSnapshot,
  NodeHealth,
  NodeRuntime,
  PresenceStage,
} from "@/lib/stageMachine/types";

const MAX_EVENTS = 80;

function initialNodes(): Record<string, NodeRuntime> {
  const now = performance.now();
  const nodes: Record<string, NodeRuntime> = {};
  for (const def of ACE_NODES) {
    const isControl = def.plane === "control";
    nodes[def.id] = {
      id: def.id,
      health: isControl ? "ready" : "cold",
      latencyMs: isControl ? Math.round(def.latencyBudgetMs * 0.7) : def.latencyBudgetMs * 2,
      load: isControl ? 0.18 : 0.02,
      lastBeatMs: now,
      message: isControl ? "control plane online" : "awaiting warm",
    };
  }
  return nodes;
}

function emptyBlend(): BlendshapeFrame {
  return synthesizeArkitFrame(0, { talking: false, energy: 0, stageLive: false });
}

/**
 * Pipe 3 Phoneme-Direct viseme track — mouth truth from text, not audio.
 * Module-level (not React state): read every tick, no re-render churn.
 */
interface VisemeTrack {
  frames: { tMediaMs: number; viseme: string; energy: number; weights: Record<string, number> }[];
  durationMs: number;
  startedAt: number;
  source: string;
}
let activeVisemeTrack: VisemeTrack | null = null;

/**
 * Sample the viseme track with CO-ARTICULATION.
 *
 * Stepping straight to the current frame makes the mouth snap between shapes
 * — the classic puppet tell, and the single loudest uncanny-valley signal in
 * a talking portrait. Real articulators are always in transit: the lips are
 * already travelling toward the next phoneme while the current one sounds.
 *
 * So we crossfade into the upcoming frame across `coarticulationMs`, easing
 * the blend so onsets are fast and settles are soft, the way muscle behaves.
 */
function sampleVisemeTrack(now: number): VisemeTrack["frames"][number] | null {
  if (!activeVisemeTrack) return null;
  const elapsed = now - activeVisemeTrack.startedAt;
  if (elapsed > activeVisemeTrack.durationMs + 250) {
    activeVisemeTrack = null;
    return null;
  }

  const frames = activeVisemeTrack.frames;
  let idx = 0;
  for (let i = 0; i < frames.length; i += 1) {
    if (frames[i].tMediaMs <= elapsed) idx = i;
    else break;
  }

  const current = frames[idx];
  if (!current) return null;

  const next = frames[idx + 1];
  const blendMs = realness.speech.coarticulationMs;
  const intensity = realness.speech.visemeIntensity;

  const scale = (w: Record<string, number>, k: number) => {
    if (k === 1) return w;
    const out: Record<string, number> = {};
    for (const [ch, v] of Object.entries(w)) out[ch] = v * k;
    return out;
  };

  if (!next || blendMs <= 0) {
    return intensity === 1 ? current : { ...current, weights: scale(current.weights, intensity) };
  }

  // Begin the transition `blendMs` before the next frame lands.
  const untilNext = next.tMediaMs - elapsed;
  if (untilNext > blendMs) {
    return intensity === 1 ? current : { ...current, weights: scale(current.weights, intensity) };
  }

  const raw = 1 - Math.max(0, untilNext) / blendMs; // 0 → 1 approaching next
  const t = raw * raw * (3 - 2 * raw); // smoothstep — muscle, not linear ramp

  const channels = new Set([...Object.keys(current.weights), ...Object.keys(next.weights)]);
  const weights: Record<string, number> = {};
  for (const ch of channels) {
    const a = current.weights[ch] ?? 0;
    const b = next.weights[ch] ?? 0;
    weights[ch] = (a + (b - a) * t) * intensity;
  }

  return {
    tMediaMs: current.tMediaMs,
    viseme: t > 0.5 ? next.viseme : current.viseme,
    energy: (current.energy + (next.energy - current.energy) * t) * intensity,
    weights,
  };
}

export type StillChoice = "auto" | "natural" | "closeup";

export interface LastTalkState {
  userText: string;
  reply: string;
  ok: boolean;
  model?: string | null;
  latencyMs?: number;
  error?: string | null;
  at?: number;
}

export interface PresenceState {
  stage: PresenceStage;
  targetStage: PresenceStage;
  ascending: boolean;
  warmProgress: number;
  nodes: Record<string, NodeRuntime>;
  selectedNodeId: string | null;
  micArmed: boolean;
  talking: boolean;
  autoWarm: boolean;
  /** Photoreal still binding — never a drawn/stock fallback. */
  stillChoice: StillChoice;
  /** Backend admin drawer open state (front stays mirror-clear). */
  adminOpen: boolean;
  presenceEnergy: number;
  clock: MediaClockSnapshot;
  blend: BlendshapeFrame;
  events: BusEvent[];
  controlMs: number;
  phaseLabel: string;
  dataPlaneLabel: string;
  /** ACE bus link state */
  busLink: BusLinkState;
  busDetail: string;
  lastTalk: LastTalkState | null;
  talkPending: boolean;
  remoteBus: boolean;
  lastRemoteAt: number;
  nvidiaConfigured: boolean;
  controllerMode: string;

  selectNode: (id: string | null) => void;
  setMicArmed: (v: boolean) => void;
  setTalking: (v: boolean) => void;
  setAutoWarm: (v: boolean) => void;
  setStillChoice: (v: StillChoice) => void;
  setAdminOpen: (v: boolean) => void;
  requestStage: (stage: PresenceStage) => void;
  promote: () => void;
  demote: () => void;
  pushEvent: (event: BusEvent) => void;
  /** Pipe 3: accept a phoneme-direct viseme timeline from the ACE bus. */
  ingestVisemes: (
    frames: { tMediaMs: number; viseme: string; energy: number; weights: Record<string, number> }[],
    durationMs: number,
    source?: string,
  ) => void;
  tick: (dtMs: number) => void;
  bootstrap: () => void;
  setBusLink: (state: BusLinkState, detail?: string) => void;
  applyAceSnapshot: (snap: AceSnapshot) => void;
  beginListenGate: () => void;
  endListenGate: () => void;
  sendTalk: (text: string) => Promise<{ ok: boolean; reply: string; error?: string; model?: string; latencyMs?: number }>;
}

function derivePhase(stage: PresenceStage, warmProgress: number, ready: boolean): string {
  if (stage === "L0" && warmProgress < 0.2) return "Phase A · Shell";
  if (stage === "L0") return "Phase B · Cortex";
  if (stage === "L1" && !ready) return "Phase C · A2F spin-up";
  if (stage === "L1") return "Phase C · Live face";
  if (stage === "L2" && !ready) return "Phase D · OV engage";
  return "Phase D · Cinematic";
}

function deriveDataPlane(
  nodes: Record<string, NodeRuntime>,
  stage: PresenceStage,
): string {
  const data = ACE_NODES.filter((n) => n.plane === "data");
  const hot = data.filter((n) => {
    const h = nodes[n.id]?.health;
    return h === "ready" || h === "hot";
  }).length;
  if (hot === 0) return "Data plane cold";
  if (hot < data.length) return `Data plane warming ${hot}/${data.length}`;
  return stage === "L2" ? "Data plane cinematic" : "Data plane ready";
}

export const usePresenceStore = create<PresenceState>((set, get) => ({
  stage: "L0",
  targetStage: "L0",
  ascending: false,
  warmProgress: 0.35,
  nodes: initialNodes(),
  selectedNodeId: null,
  micArmed: true,
  talking: false,
  autoWarm: true,
  stillChoice: "auto",
  adminOpen: false,
  presenceEnergy: 0.12,
  clock: globalSyncer.tick(),
  blend: emptyBlend(),
  events: [
    createBusEvent("system", "ok", "Instant Presence Standard online"),
    createBusEvent("presence", "ok", "L0 idle loop armed — control plane <1s"),
    createBusEvent("sync", "info", "Spatial Syncer media clock locked"),
  ],
  controlMs: 42,
  phaseLabel: "Phase B · Cortex",
  dataPlaneLabel: "Data plane cold",
  busLink: "offline",
  busDetail: "",
  lastTalk: null,
  talkPending: false,
  remoteBus: false,
  lastRemoteAt: 0,
  nvidiaConfigured: false,
  controllerMode: "",

  selectNode: (id) => set({ selectedNodeId: id }),

  setMicArmed: (v) => {
    set({ micArmed: v });
    get().pushEvent(
      createBusEvent("user", v ? "ok" : "warn", v ? "Mic armed" : "Mic muted"),
    );
  },

  setTalking: (v) => {
    const { micArmed, stage } = get();
    if (v && !micArmed) {
      get().pushEvent(createBusEvent("user", "warn", "Cannot talk — mic muted"));
      return;
    }
    set({ talking: v, presenceEnergy: v ? 0.55 : 0.14 });
    if (v && stage === "L0") {
      get().requestStage("L1");
    }
    get().pushEvent(
      createBusEvent(
        "presence",
        v ? "ok" : "info",
        v ? "Talk gate open — promoting toward L1" : "Talk gate closed",
      ),
    );
  },

  setAutoWarm: (v) => set({ autoWarm: v }),
  setStillChoice: (v) => set({ stillChoice: v }),
  setAdminOpen: (v) => set({ adminOpen: v }),

  pushEvent: (event) =>
    set((s) => ({ events: [event, ...s.events].slice(0, MAX_EVENTS) })),

  setBusLink: (link, detail) => {
    set({ busLink: link, busDetail: detail || "" });
    if (link === "live" || link === "mock") {
      get().pushEvent(
        createBusEvent("sync", "ok", `ACE bus ${link}${detail ? ` · ${detail}` : ""}`)
      );
    } else if (link === "error") {
      get().pushEvent(
        createBusEvent("sync", "warn", `ACE bus error${detail ? ` · ${detail}` : ""}`)
      );
    }
  },

  applyAceSnapshot: (snap: AceSnapshot) => {
    const now = performance.now();
    const prev = get();
    const nodes = { ...prev.nodes };
    if (snap.nodes) {
      for (const [id, n] of Object.entries(snap.nodes)) {
        const prevN = nodes[id];
        nodes[id] = {
          id,
          health: (n.health as NodeHealth) || prevN?.health || "cold",
          latencyMs: n.latencyMs ?? prevN?.latencyMs ?? 0,
          load: n.load ?? prevN?.load ?? 0,
          lastBeatMs: now,
          message: n.message || n.health || prevN?.message || "",
        };
      }
    }
    if (nodes.mic) {
      nodes.mic = {
        ...nodes.mic,
        health: prev.micArmed
          ? prev.talking || prev.talkPending
            ? "hot"
            : "ready"
          : "degraded",
      };
    }
    const stage = (snap.stage as PresenceStage) || prev.stage;
    const targetStage = (snap.targetStage as PresenceStage) || prev.targetStage;
    const warmProgress =
      typeof snap.warmProgress === "number" ? snap.warmProgress : prev.warmProgress;
    const controlMs = typeof snap.controlMs === "number" ? snap.controlMs : prev.controlMs;
    let lastTalk = prev.lastTalk;
    if (snap.lastTalk) {
      lastTalk = {
        userText: snap.lastTalk.userText || "",
        reply: snap.lastTalk.reply || "",
        ok: Boolean(snap.lastTalk.ok),
        model: snap.lastTalk.model,
        latencyMs: snap.lastTalk.latencyMs,
        error: snap.lastTalk.error,
        at: snap.lastTalk.at,
      };
    }
    const readiness = stageReadiness(stage, nodes);
    set({
      stage,
      targetStage,
      warmProgress,
      nodes,
      controlMs,
      lastTalk,
      remoteBus: true,
      lastRemoteAt: Date.now(),
      nvidiaConfigured: Boolean(snap.nvidia?.configured),
      controllerMode: snap.mode || prev.controllerMode,
      phaseLabel: derivePhase(stage, warmProgress, readiness.ready),
      dataPlaneLabel: deriveDataPlane(nodes, stage),
    });
    const msg = snap.event?.message;
    if (msg && msg !== prev.events[0]?.message) {
      const level = snap.event?.level || "info";
      get().pushEvent(createBusEvent("system", level as any, msg));
    }
  },

  beginListenGate: () => {
    const { micArmed, stage } = get();
    if (!micArmed) {
      get().pushEvent(createBusEvent("user", "warn", "Cannot talk — mic muted"));
      return;
    }
    set({ talking: true, presenceEnergy: 0.5 });
    if (stage === "L0") get().requestStage("L1");
  },

  endListenGate: () => {
    if (!get().talkPending) {
      set({ talking: false, presenceEnergy: 0.14 });
    }
  },

  async sendTalk(text: string) {
    const clean = text.trim();
    if (!clean) {
      return { ok: false, reply: "", error: "empty" };
    }
    if (!get().micArmed) {
      get().pushEvent(createBusEvent("user", "warn", "Cannot talk — mic muted"));
      return { ok: false, reply: "", error: "mic muted" };
    }
    set({ talkPending: true, talking: true, presenceEnergy: 0.62 });
    if (get().stage === "L0") get().requestStage("L1");
    get().pushEvent(createBusEvent("user", "ok", `You: ${clean.slice(0, 120)}`));
    try {
      const result = await aceController.talk(clean, { promote: true });
      const lastTalk: LastTalkState = {
        userText: clean,
        reply: result.reply || "",
        ok: Boolean(result.ok),
        model: result.model,
        latencyMs: result.latencyMs,
        error: result.error,
        at: Date.now() / 1000,
      };
      set({
        lastTalk,
        talkPending: false,
        talking: Boolean(result.reply),
        presenceEnergy: result.reply ? 0.7 : 0.14,
      });
      if (result.reply) {
        get().pushEvent(
          createBusEvent(
            "presence",
            result.ok ? "ok" : "warn",
            `EVE: ${result.reply.slice(0, 160)}${result.model ? ` · ${result.model}` : ""}${result.latencyMs != null ? ` · ${result.latencyMs}ms` : ""
            }`
          )
        );
      } else {
        get().pushEvent(
          createBusEvent("system", "error", result.error || "Talk path returned no reply")
        );
      }
      return {
        ok: Boolean(result.ok),
        reply: result.reply || "",
        error: result.error ?? undefined,
        model: result.model ?? undefined,
        latencyMs: result.latencyMs,
      };
    } catch (err) {
      const error = err instanceof Error ? err.message : "talk failed";
      set({ talkPending: false, talking: false, presenceEnergy: 0.14 });
      get().pushEvent(createBusEvent("system", "error", error));
      return { ok: false, reply: "", error };
    }
  },

  requestStage: (stage) => {
    const current = get().stage;
    if (stage === current && stage === get().targetStage) return;

    if (!controlPlaneReady(get().nodes)) {
      get().pushEvent(
        createBusEvent("system", "error", "Stage blocked — control plane not ready"),
      );
      return;
    }

    // Instant Presence: demote is always allowed (never trap user in L2)
    if (stage === "L0" || stage === "L1" || stage === "L2") {
      set({
        targetStage: stage,
        ascending: stage > current,
        warmProgress: stage === current ? get().warmProgress : stage < current ? 1 : 0.08,
      });
      get().pushEvent(globalSyncer.stageMessage(current, stage));
    }
  },

  promote: () => {
    const n = nextStage(get().stage);
    if (n) get().requestStage(n);
  },

  demote: () => {
    const p = prevStage(get().stage);
    if (p) get().requestStage(p);
    else get().requestStage("L0");
  },

  bootstrap: () => {
    globalSyncer.reset();
    get().pushEvent(createBusEvent("system", "info", "Session bootstrap — warm path engaged"));
  },

  ingestVisemes: (frames, durationMs, source = "phoneme-direct") => {
    if (!frames.length) return;
    activeVisemeTrack = { frames, durationMs, startedAt: performance.now(), source };
    set({ talking: true });
    get().pushEvent(
      createBusEvent(
        "presence",
        "ok",
        `Pipe 3 · ${source} — ${frames.length} visemes / ${(durationMs / 1000).toFixed(1)}s, zero-drift`,
      ),
    );
  },

  tick: (dtMs) => {
    const state = get();
    const slip =
      state.stage === "L2" && state.warmProgress < 1
        ? Math.random() * 2
        : Math.random() < 0.05
          ? 0.4
          : 0;
    const clock = globalSyncer.tick(slip);

    let { warmProgress, stage, targetStage, ascending, presenceEnergy, talking, micArmed, autoWarm } =
      state;

    // Auto-warm data plane gently while on L0
    if (autoWarm && stage === "L0" && targetStage === "L0") {
      warmProgress = Math.min(0.62, warmProgress + dtMs * 0.00004);
    }

    // Drive toward target stage
    if (targetStage !== stage) {
      const dir = targetStage > stage ? 1 : -1;
      ascending = dir > 0;
      warmProgress = Math.min(1, Math.max(0, warmProgress + dir * dtMs * 0.00055));

      if (dir > 0 && warmProgress >= 0.97) {
        const readiness = stageReadiness(targetStage, state.nodes);
        // Instant Presence exception: allow L1 if control ready + a2f warming enough
        const allow =
          readiness.ready ||
          (targetStage === "L1" && readiness.score >= 0.75) ||
          (targetStage === "L2" && readiness.score >= 0.85);
        if (allow) {
          stage = targetStage;
          warmProgress = 1;
          get().pushEvent(
            createBusEvent("stage", "ok", `Holding ${stage} — ${getContract(stage).title}`),
          );
        }
      }
      if (dir < 0 && warmProgress <= 0.05) {
        stage = targetStage;
        warmProgress = stage === "L0" ? 0.4 : 1;
        get().pushEvent(createBusEvent("stage", "info", `Settled at ${stage}`));
      }
    } else if (warmProgress < 1) {
      warmProgress = Math.min(1, warmProgress + dtMs * 0.00035);
    }

    // When remoteBus is active and recent, preserve remote node states
    const useRemoteNodes =
      state.remoteBus && Date.now() - state.lastRemoteAt < 4000;

    const nodes: Record<string, NodeRuntime> = { ...state.nodes };
    const now = performance.now();

    if (!useRemoteNodes) {
      for (const def of ACE_NODES) {
        let health = healthForStageRole(def.id, stage, warmProgress);

        // Target stage pulls warm nodes forward
        if (targetStage !== stage && getContract(targetStage).hotNodes.includes(def.id)) {
          if (warmProgress > 0.2 && health === "cold") health = "warming";
          if (warmProgress > 0.6 && (health === "warming" || health === "cold")) health = "ready";
          if (warmProgress > 0.9) health = "hot";
        }

        // Control plane always stays ready+
        if (def.plane === "control") {
          health = talking ? "hot" : "ready";
        }

        // Mic reflects arm state
        if (def.id === "mic") {
          health = micArmed ? (talking ? "hot" : "ready") : "degraded";
        }

        const loadBase =
          health === "hot" ? 0.55 : health === "ready" ? 0.28 : health === "warming" ? 0.4 : 0.05;
        const load =
          Math.min(1, Math.max(0, loadBase + Math.sin(now / 400 + def.id.length) * 0.05));

        nodes[def.id] = {
          id: def.id,
          health,
          latencyMs: latencyForHealth(def.latencyBudgetMs, health),
          load,
          lastBeatMs: now,
          message:
            health === "hot"
              ? "streaming"
              : health === "ready"
                ? "ready"
                : health === "warming"
                  ? "warming NIM path"
                  : health === "degraded"
                    ? "degraded"
                    : "cold",
        };
      }
    } else {
      // Preserve remote state, overlay control plane
      for (const def of ACE_NODES) {
        if (def.plane === "control") {
          const health = talking ? ("hot" as NodeHealth) : ("ready" as NodeHealth);
          nodes[def.id] = {
            ...nodes[def.id],
            health,
            lastBeatMs: now,
          };
        }
        if (def.id === "mic") {
          const health = micArmed
            ? (talking ? ("hot" as NodeHealth) : ("ready" as NodeHealth))
            : ("degraded" as NodeHealth);
          nodes[def.id] = {
            ...nodes[def.id],
            health,
            lastBeatMs: now,
          };
        }
      }
    }

    // Presence energy
    const targetEnergy = talking ? 0.5 + Math.random() * 0.35 : 0.1 + Math.random() * 0.06;
    presenceEnergy = presenceEnergy * 0.85 + targetEnergy * 0.15;

    let blend = synthesizeArkitFrame(clock.tMediaMs, {
      talking: talking && micArmed,
      energy: presenceEnergy,
      stageLive: stage !== "L0" || talking,
    });

    // Pipe 3 override: when a phoneme-direct track is live, the mouth obeys
    // the text-derived timeline — idle synthesis keeps eyes/brows/breath.
    const visemeFrame = sampleVisemeTrack(now);
    if (visemeFrame) {
      talking = true;
      blend = {
        ...blend,
        viseme: visemeFrame.viseme,
        energy: Math.max(blend.energy, visemeFrame.energy),
        weights: { ...blend.weights, ...visemeFrame.weights },
      };
    } else if (talking && activeVisemeTrack === null && state.talking && !state.talkPending) {
      // track just ended — settle back to idle presence
      talking = false;
    }

    const readiness = stageReadiness(stage, nodes);
    const controlMs = Math.round(
      (nodes.presence?.latencyMs ?? 40) + (nodes.syncer?.latencyMs ?? 8) + Math.random() * 6,
    );

    set({
      stage,
      targetStage,
      ascending,
      warmProgress,
      nodes,
      presenceEnergy,
      talking,
      clock,
      blend,
      controlMs,
      phaseLabel: derivePhase(stage, warmProgress, readiness.ready),
      dataPlaneLabel: deriveDataPlane(nodes, stage),
    });
  },
}));
