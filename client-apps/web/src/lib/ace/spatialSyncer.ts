import type { BusEvent, MediaClockSnapshot, PresenceStage } from "@/lib/stageMachine/types";

let seq = 0;

export function createBusEvent(
  kind: BusEvent["kind"],
  level: BusEvent["level"],
  message: string,
  meta?: BusEvent["meta"],
): BusEvent {
  seq += 1;
  return {
    id: `evt-${Date.now().toString(36)}-${seq}`,
    t: performance.now(),
    kind,
    level,
    message,
    meta,
  };
}

/**
 * Spatial Syncer — Instant Presence media clock.
 * Control plane ticks freely; data plane may lag without blocking L0.
 */
export class SpatialSyncer {
  private started = performance.now();
  private mediaOffset = 0;
  private lastWall = performance.now();
  private pulses = 0;
  private ppsWindowStart = performance.now();
  private pps = 0;

  reset() {
    this.started = performance.now();
    this.mediaOffset = 0;
    this.lastWall = this.started;
    this.pulses = 0;
    this.ppsWindowStart = this.started;
    this.pps = 0;
  }

  /** Advance media clock; optional intentional slip for degraded links */
  tick(slipMs = 0): MediaClockSnapshot {
    const now = performance.now();
    this.mediaOffset += Math.max(0, slipMs);
    this.pulses += 1;

    if (now - this.ppsWindowStart >= 1000) {
      this.pps = this.pulses;
      this.pulses = 0;
      this.ppsWindowStart = now;
    }

    const tMediaMs = now - this.started - this.mediaOffset;
    const wallDelta = now - this.lastWall;
    this.lastWall = now;
    const expected = 1000 / 30;
    const driftMs = wallDelta - expected;

    return {
      tMediaMs,
      wallIso: new Date().toISOString(),
      driftMs: Math.round(driftMs * 10) / 10,
      pps: this.pps,
    };
  }

  stageMessage(from: PresenceStage, to: PresenceStage): BusEvent {
    return createBusEvent(
      "stage",
      to === "L0" ? "info" : "ok",
      `Stage ${from} → ${to}`,
      { from, to },
    );
  }
}

export const globalSyncer = new SpatialSyncer();
