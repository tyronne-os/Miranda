import { Activity, Sparkles } from "lucide-react";
import { usePresenceStore } from "@/store/presenceStore";

/** Slim vanity chrome — dense telemetry lives in Cortex + Backend Studio. */
export function AppHeader() {
  const stage = usePresenceStore((s) => s.stage);
  const controlMs = usePresenceStore((s) => s.controlMs);
  const phaseLabel = usePresenceStore((s) => s.phaseLabel);

  const controlLive = controlMs < 1000;

  return (
    <header
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 16,
        padding: "0 16px",
        borderBottom: "1px solid var(--panel-border)",
        background:
          "linear-gradient(180deg, rgba(6,8,12,0.98), rgba(2,3,4,0.96))",
        backdropFilter: "blur(10px)",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <div
          style={{
            width: 34,
            height: 34,
            borderRadius: 10,
            display: "grid",
            placeItems: "center",
            background:
              "radial-gradient(circle at 30% 30%, rgba(61,180,255,0.28), transparent 55%), linear-gradient(135deg, #0b1a2e, #05080f)",
            border: "1px solid rgba(61,180,255,0.45)",
            boxShadow: "0 0 22px rgba(61,180,255,0.18)",
          }}
        >
          <Sparkles size={16} color="var(--spark)" />
        </div>
        <div>
          <div style={{ fontWeight: 700, letterSpacing: "0.03em", fontSize: 14 }}>
            The Cerebral Project
          </div>
          <div className="muted" style={{ fontSize: 11 }}>
            Cognitive research · LLM presence systems
          </div>
        </div>
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <span className="badge spark" title="Cognitive research surface">
          Research
        </span>
        <span className={`badge${controlLive ? " live" : " warn"}`}>
          <span className="dot" />
          {stage}
        </span>
        <span className="badge">
          <Activity size={12} />
          {phaseLabel}
        </span>
      </div>
    </header>
  );
}
