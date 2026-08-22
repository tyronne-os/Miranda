/**
 * Reporting Agent — consumes live indicator science every tick window
 * and emits structured session intelligence for operators / downstream agents.
 *
 * Design:
 * - Pure functions + small ring buffer (no network side effects)
 * - Threshold alerts with hysteresis
 * - Rolling averages for warm/lag/continuity/presence/gpu
 * - Narrative summary suitable for Cerebral Project run logs
 */

import type { PresenceStage } from "@/lib/stageMachine/types";
import type {
  LiveMeterId,
  LiveMetricsSnapshot,
} from "@/lib/metrics/types";
import { LIVE_METER_ORDER } from "@/lib/metrics/liveIndicators";

export type ReportSeverity = "info" | "ok" | "warn" | "critical";

export interface ReportAlert {
  id: string;
  severity: ReportSeverity;
  meter?: LiveMeterId;
  message: string;
  atMs: number;
}

export interface MeterTrend {
  id: LiveMeterId;
  current: number;
  avg: number;
  min: number;
  max: number;
  /** positive = rising */
  slope: number;
}

export interface SessionReport {
  id: string;
  generatedAtMs: number;
  wallIso: string;
  stage: PresenceStage;
  targetStage: PresenceStage;
  sampleCount: number;
  interactionSeconds: number;
  talkSeconds: number;
  trends: MeterTrend[];
  alerts: ReportAlert[];
  /** One-paragraph operator summary */
  narrative: string;
  /** Compact scores for dashboards */
  scores: {
    warm: number;
    lag: number;
    continuity: number;
    presence: number;
    gpu: number;
    gpuWattsEst: number;
    sessionGpuCost: number;
    continuityBreakdown: {
      cognitive: number;
      conversational: number;
      cultural: number;
      cohesive: number;
    };
  };
}

export interface ReportingAgentState {
  samples: LiveMetricsSnapshot[];
  alerts: ReportAlert[];
  latest: SessionReport | null;
  sampleEveryMs: number;
  maxSamples: number;
  lastSampleAt: number;
  alertCooldown: Record<string, number>;
  interactionSeconds: number;
  talkSeconds: number;
  reportSeq: number;
}

const ALERT_COOLDOWN_MS = 8000;

export function createReportingAgentState(): ReportingAgentState {
  return {
    samples: [],
    alerts: [],
    latest: null,
    sampleEveryMs: 500,
    maxSamples: 240,
    lastSampleAt: 0,
    alertCooldown: {},
    interactionSeconds: 0,
    talkSeconds: 0,
    reportSeq: 0,
  };
}

function pushAlert(
  state: ReportingAgentState,
  alert: Omit<ReportAlert, "id">,
  now: number,
): void {
  const key = `${alert.meter ?? "sys"}:${alert.message}`;
  const last = state.alertCooldown[key] ?? 0;
  if (now - last < ALERT_COOLDOWN_MS) return;
  state.alertCooldown[key] = now;
  state.alerts = [
    {
      id: `al-${now.toString(36)}-${state.alerts.length}`,
      ...alert,
    },
    ...state.alerts,
  ].slice(0, 40);
}

function evaluateAlerts(
  state: ReportingAgentState,
  snap: LiveMetricsSnapshot,
  now: number,
): void {
  const m = snap.meters;
  if (m.lag.value01 >= 0.72) {
    pushAlert(
      state,
      {
        severity: m.lag.value01 >= 0.88 ? "critical" : "warn",
        meter: "lag",
        message: `Lag elevated at ${m.lag.displayPct}% — control/node/drift pressure`,
        atMs: now,
      },
      now,
    );
  }
  if (m.continuity.value01 <= 0.42) {
    pushAlert(
      state,
      {
        severity: m.continuity.value01 <= 0.28 ? "critical" : "warn",
        meter: "continuity",
        message: `Continuity soft at ${m.continuity.displayPct}% — check cognitive/conversational/cohesive mix`,
        atMs: now,
      },
      now,
    );
  }
  if (m.presence.value01 <= 0.5) {
    pushAlert(
      state,
      {
        severity: "warn",
        meter: "presence",
        message: `Instant Presence quality ${m.presence.displayPct}% — control plane budget risk`,
        atMs: now,
      },
      now,
    );
  }
  if (m.gpu.value01 >= 0.8) {
    pushAlert(
      state,
      {
        severity: m.gpu.value01 >= 0.92 ? "critical" : "warn",
        meter: "gpu",
        message: `GPU cost ${m.gpu.displayPct}% (~${snap.gpu.wattsEst}W) — cinematic envelope pressure`,
        atMs: now,
      },
      now,
    );
  }
  if (snap.continuity.cohesive <= 0.4) {
    pushAlert(
      state,
      {
        severity: "warn",
        meter: "continuity",
        message: `Facial cohesive awareness ${Math.round(snap.continuity.cohesive * 100)}% — drift during long interaction`,
        atMs: now,
      },
      now,
    );
  }
}

function trendFor(
  id: LiveMeterId,
  samples: LiveMetricsSnapshot[],
): MeterTrend {
  const values = samples.map((s) => s.meters[id].value01);
  if (!values.length) {
    return { id, current: 0, avg: 0, min: 0, max: 0, slope: 0 };
  }
  const current = values[values.length - 1]!;
  const sum = values.reduce((a, b) => a + b, 0);
  const avg = sum / values.length;
  const min = Math.min(...values);
  const max = Math.max(...values);
  const n = Math.min(12, values.length);
  const recent = values.slice(-n);
  const first = recent[0]!;
  const last = recent[recent.length - 1]!;
  const slope = (last - first) / Math.max(1, n - 1);
  return { id, current, avg, min, max, slope };
}

function buildNarrative(
  stage: PresenceStage,
  trends: MeterTrend[],
  alerts: ReportAlert[],
  snap: LiveMetricsSnapshot,
): string {
  const get = (id: LiveMeterId) => trends.find((t) => t.id === id);
  const warm = get("warm");
  const lag = get("lag");
  const cont = get("continuity");
  const pres = get("presence");
  const gpu = get("gpu");
  const alertBit =
    alerts.length === 0
      ? "No active threshold alerts."
      : `${alerts.length} alert(s); latest: ${alerts[0]!.message}`;

  return [
    `Stage ${stage}.`,
    `Warm ${Math.round((warm?.current ?? 0) * 100)}% (avg ${Math.round((warm?.avg ?? 0) * 100)}%).`,
    `Lag ${Math.round((lag?.current ?? 0) * 100)}%${(lag?.slope ?? 0) > 0.01 ? " rising" : (lag?.slope ?? 0) < -0.01 ? " easing" : ""}.`,
    `Continuity ${Math.round((cont?.current ?? 0) * 100)}% (cog ${Math.round(snap.continuity.cognitive * 100)} / conv ${Math.round(snap.continuity.conversational * 100)} / cult ${Math.round(snap.continuity.cultural * 100)} / cohere ${Math.round(snap.continuity.cohesive * 100)}).`,
    `Presence ${Math.round((pres?.current ?? 0) * 100)}%.`,
    `GPU ${Math.round((gpu?.current ?? 0) * 100)}% (~${snap.gpu.wattsEst}W, session ${snap.gpu.sessionCost.toFixed(1)} s·u).`,
    alertBit,
  ].join(" ");
}

export interface ReportingTickInput {
  nowMs: number;
  stage: PresenceStage;
  targetStage: PresenceStage;
  snapshot: LiveMetricsSnapshot;
  interactionSeconds: number;
  talkSeconds: number;
  /** Force rebuild even if sample window not elapsed */
  force?: boolean;
}

/**
 * Ingest metrics; sample on interval; always refresh latest report when sampled.
 */
export function tickReportingAgent(
  state: ReportingAgentState,
  input: ReportingTickInput,
): ReportingAgentState {
  const next: ReportingAgentState = {
    ...state,
    alertCooldown: { ...state.alertCooldown },
    interactionSeconds: input.interactionSeconds,
    talkSeconds: input.talkSeconds,
  };

  evaluateAlerts(next, input.snapshot, input.nowMs);

  const due =
    input.force ||
    input.nowMs - next.lastSampleAt >= next.sampleEveryMs ||
    next.samples.length === 0;

  if (!due) {
    // Still refresh scores on latest report shell if we have one
    if (next.latest) {
      next.latest = {
        ...next.latest,
        scores: {
          ...next.latest.scores,
          warm: input.snapshot.meters.warm.value01,
          lag: input.snapshot.meters.lag.value01,
          continuity: input.snapshot.meters.continuity.value01,
          presence: input.snapshot.meters.presence.value01,
          gpu: input.snapshot.meters.gpu.value01,
          gpuWattsEst: input.snapshot.gpu.wattsEst,
          sessionGpuCost: input.snapshot.gpu.sessionCost,
          continuityBreakdown: { ...input.snapshot.continuity },
        },
      };
    }
    return next;
  }

  next.lastSampleAt = input.nowMs;
  next.samples = [...next.samples, input.snapshot].slice(-next.maxSamples);
  next.reportSeq += 1;

  const trends = LIVE_METER_ORDER.map((id) => trendFor(id, next.samples));
  const recentAlerts = next.alerts.filter((a) => input.nowMs - a.atMs < 60_000);

  next.latest = {
    id: `rpt-${next.reportSeq}`,
    generatedAtMs: input.nowMs,
    wallIso: input.snapshot.wallIso,
    stage: input.stage,
    targetStage: input.targetStage,
    sampleCount: next.samples.length,
    interactionSeconds: input.interactionSeconds,
    talkSeconds: input.talkSeconds,
    trends,
    alerts: recentAlerts.slice(0, 12),
    narrative: buildNarrative(input.stage, trends, recentAlerts, input.snapshot),
    scores: {
      warm: input.snapshot.meters.warm.value01,
      lag: input.snapshot.meters.lag.value01,
      continuity: input.snapshot.meters.continuity.value01,
      presence: input.snapshot.meters.presence.value01,
      gpu: input.snapshot.meters.gpu.value01,
      gpuWattsEst: input.snapshot.gpu.wattsEst,
      sessionGpuCost: input.snapshot.gpu.sessionCost,
      continuityBreakdown: { ...input.snapshot.continuity },
    },
  };

  return next;
}
