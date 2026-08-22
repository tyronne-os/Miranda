/**
 * UPON LOAD — Time-to-Presence metric.
 *
 * The Instant Presence Standard says a guest must feel her within one second.
 * This measures whether that promise was actually kept, on this machine, on
 * this network, this time.
 *
 * Two numbers, because one number would be a lie:
 *
 *   WALL   — what the guest actually experienced, start of navigation to fully
 *            interactive. Includes DNS, TLS, transfer, image download. This is
 *            the honest human truth, and it moves with network conditions.
 *
 *   ENGINE — WALL minus measured network transfer. This is the part OUR code
 *            owns and the only part we can be graded on fairly. A guest on
 *            hotel wifi should not fail our engineering.
 *
 * ENGINE is graded against the <1s Instant Presence contract. WALL is reported
 * with the live network condition so the number is always readable in context.
 */

export type UponLoadGrade = "instant" | "pass" | "warn" | "fail" | "pending";

export interface UponLoadMarks {
  /** Portrait decoded and painted — she is visible. */
  portraitMs: number | null;
  /** First idle-presence frame written — she is breathing. */
  motionMs: number | null;
  /** ACE bus connected — the cortex is reachable. */
  busMs: number | null;
  /** All of the above — she is fully interactive. */
  interactiveMs: number | null;
}

export interface UponLoadReading extends UponLoadMarks {
  wallMs: number | null;
  engineMs: number | null;
  networkMs: number;
  grade: UponLoadGrade;
  /** e.g. "4g · 12ms rtt" — context for the wall number. */
  connection: string;
  complete: boolean;
}

/** Instant Presence contract thresholds, in ms, measured on ENGINE time. */
const GRADE_INSTANT = 400;
const GRADE_PASS = 1000; // the contract
const GRADE_WARN = 2000;

type Listener = (reading: UponLoadReading) => void;

class UponLoadMeter {
  private marks: UponLoadMarks = {
    portraitMs: null,
    motionMs: null,
    busMs: null,
    interactiveMs: null,
  };
  private listeners = new Set<Listener>();
  private settled = false;

  /** Record a checkpoint once; later calls for the same mark are ignored. */
  mark(key: keyof UponLoadMarks) {
    if (this.settled || this.marks[key] != null) return;
    this.marks[key] = Math.round(performance.now());
    this.evaluate();
  }

  private evaluate() {
    const { portraitMs, motionMs, busMs } = this.marks;
    if (portraitMs != null && motionMs != null && busMs != null && this.marks.interactiveMs == null) {
      // Fully interactive = the last of the three prerequisites landed.
      this.marks.interactiveMs = Math.max(portraitMs, motionMs, busMs);
      this.settled = true;
    }
    this.emit();
  }

  /**
   * Network cost we should not be graded on: server think time plus the
   * transfer of every resource that gated first interaction.
   */
  private networkPenalty(): number {
    try {
      const nav = performance.getEntriesByType("navigation")[0] as
        | PerformanceNavigationTiming
        | undefined;
      if (!nav) return 0;

      // Request sent → first byte back. Pure round-trip + server time.
      const ttfb = Math.max(0, nav.responseStart - nav.requestStart);

      // The portrait is the heaviest gating asset — count its transfer only.
      let assetMs = 0;
      const staff = performance
        .getEntriesByType("resource")
        .filter((r) => r.name.includes("/staff/")) as PerformanceResourceTiming[];
      for (const r of staff) {
        if (r.responseStart > 0) assetMs = Math.max(assetMs, r.responseEnd - r.requestStart);
      }

      return Math.round(ttfb + assetMs);
    } catch {
      return 0;
    }
  }

  private connectionLabel(): string {
    const c = (
      navigator as Navigator & {
        connection?: { effectiveType?: string; rtt?: number; downlink?: number };
      }
    ).connection;
    if (!c) return "unknown link";
    const parts: string[] = [];
    if (c.effectiveType) parts.push(c.effectiveType);
    if (typeof c.rtt === "number") parts.push(`${c.rtt}ms rtt`);
    if (typeof c.downlink === "number") parts.push(`${c.downlink}Mb`);
    return parts.join(" · ") || "unknown link";
  }

  read(): UponLoadReading {
    const networkMs = this.networkPenalty();
    const wallMs = this.marks.interactiveMs;
    const engineMs = wallMs == null ? null : Math.max(0, wallMs - networkMs);

    let grade: UponLoadGrade = "pending";
    if (engineMs != null) {
      if (engineMs < GRADE_INSTANT) grade = "instant";
      else if (engineMs < GRADE_PASS) grade = "pass";
      else if (engineMs < GRADE_WARN) grade = "warn";
      else grade = "fail";
    }

    return {
      ...this.marks,
      wallMs,
      engineMs,
      networkMs,
      grade,
      connection: this.connectionLabel(),
      complete: this.settled,
    };
  }

  subscribe(fn: Listener): () => void {
    this.listeners.add(fn);
    fn(this.read());
    return () => this.listeners.delete(fn);
  }

  private emit() {
    const reading = this.read();
    for (const fn of this.listeners) fn(reading);
  }
}

export const uponLoad = new UponLoadMeter();

/** Exposed for the ECC-5 certification harness to read from outside React. */
declare global {
  interface Window {
    __EVE_UPON_LOAD__?: () => UponLoadReading;
  }
}
if (typeof window !== "undefined") {
  window.__EVE_UPON_LOAD__ = () => uponLoad.read();
}
