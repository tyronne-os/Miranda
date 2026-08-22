/** Live presence quality indicators — same meter family as warm. */

export type LiveMeterId = "warm" | "lag" | "continuity" | "presence" | "gpu";

export type MeterTone = "good" | "warn" | "bad" | "neutral";

export interface LiveMeterReading {
  id: LiveMeterId;
  /** Display label */
  label: string;
  /** 0..1 fill amount (always 0–100% of meter width) */
  value01: number;
  /** Primary numeric shown in the chip */
  displayPct: number;
  /** Short unit suffix after the number */
  unit: string;
  tone: MeterTone;
  /** One-line science gloss for title/tooltip */
  detail: string;
}

export interface ContinuityBreakdown {
  cognitive: number;
  conversational: number;
  cultural: number;
  cohesive: number;
}

export interface GpuCostBreakdown {
  /** Normalized 0..1 against full L2 envelope */
  index01: number;
  /** Estimated relative watts vs idle shell */
  wattsEst: number;
  /** Session integral of gpu index * seconds */
  sessionCost: number;
  byPlane: { control: number; data: number };
  heavyNodes: Array<{ id: string; share: number }>;
}

export interface LiveMetricsSnapshot {
  tMediaMs: number;
  wallIso: string;
  meters: Record<LiveMeterId, LiveMeterReading>;
  continuity: ContinuityBreakdown;
  gpu: GpuCostBreakdown;
  /** Control-plane lag sources used by LAG meter */
  lagSources: {
    controlRatio: number;
    nodeLatencyRatio: number;
    driftRatio: number;
    slipRatio: number;
  };
}

export interface LiveMetricsState {
  meters: Record<LiveMeterId, LiveMeterReading>;
  continuity: ContinuityBreakdown;
  gpu: GpuCostBreakdown;
  lagSources: LiveMetricsSnapshot["lagSources"];
  /** EMA internals / rolling science state */
  _smooth: Record<LiveMeterId, number>;
  _blendBaseline: Record<string, number> | null;
  _sessionGpuIntegral: number;
  _talkSeconds: number;
  _interactionSeconds: number;
  _lastReportAt: number;
}
