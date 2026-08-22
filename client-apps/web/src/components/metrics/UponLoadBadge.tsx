import { useEffect, useState } from "react";
import { uponLoad, type UponLoadReading } from "@/lib/metrics/uponLoad";

const GRADE_LABEL: Record<UponLoadReading["grade"], string> = {
  instant: "INSTANT",
  pass: "PASS",
  warn: "SLOW",
  fail: "FAIL",
  pending: "MEASURING",
};

function fmt(ms: number | null) {
  if (ms == null) return "—";
  return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(2)}s`;
}

/**
 * UPON LOAD — bottom-corner presence timer.
 *
 * Shows how long she took to become fully interactive. Collapsed it is one
 * number; hover for the breakdown that proves the number is honest.
 */
export function UponLoadBadge() {
  const [reading, setReading] = useState<UponLoadReading>(() => uponLoad.read());

  useEffect(() => uponLoad.subscribe(setReading), []);

  const headline = reading.engineMs ?? null;

  return (
    <div
      className={`upon-load grade-${reading.grade}`}
      role="status"
      aria-live="polite"
      aria-label={`Upon load: ${fmt(headline)} engine time, ${GRADE_LABEL[reading.grade]}`}
    >
      <div className="upon-load-head">
        <span className="upon-load-title">UPON LOAD</span>
        <span className="upon-load-grade">{GRADE_LABEL[reading.grade]}</span>
      </div>

      <div className="upon-load-value">{fmt(headline)}</div>

      <div className="upon-load-detail">
        <div className="upon-load-row">
          <span>engine</span>
          <b>{fmt(reading.engineMs)}</b>
        </div>
        <div className="upon-load-row">
          <span>wall</span>
          <b>{fmt(reading.wallMs)}</b>
        </div>
        <div className="upon-load-row">
          <span>network</span>
          <b>−{fmt(reading.networkMs)}</b>
        </div>
        <div className="upon-load-sep" />
        <div className="upon-load-row">
          <span>visible</span>
          <b>{fmt(reading.portraitMs)}</b>
        </div>
        <div className="upon-load-row">
          <span>breathing</span>
          <b>{fmt(reading.motionMs)}</b>
        </div>
        <div className="upon-load-row">
          <span>cortex</span>
          <b>{fmt(reading.busMs)}</b>
        </div>
        <div className="upon-load-link">{reading.connection}</div>
      </div>
    </div>
  );
}
