import type { LiveMeterReading } from "@/lib/metrics/types";

type Props = {
  reading: LiveMeterReading;
};

/** Same physical size/shape as the classic warm meter — tone reacts live. */
export function LiveMeter({ reading }: Props) {
  const pct = Math.round(reading.value01 * 100);
  return (
    <div
      className={`live-meter tone-${reading.tone}`}
      title={reading.detail}
      role="meter"
      aria-label={reading.detail}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct}
    >
      <div className="live-meter-fill" style={{ width: `${pct}%` }} />
      <span>
        {reading.label} {reading.displayPct}
        {reading.unit}
      </span>
    </div>
  );
}
