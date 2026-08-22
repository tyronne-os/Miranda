import { usePresenceStore } from "@/store/presenceStore";

/** Minimal footer — event bus detail stays in Cortex / Backend. */
export function AppFooter() {
  const events = usePresenceStore((s) => s.events);
  const latest = events[0];

  return (
    <footer
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 12,
        padding: "0 14px",
        borderTop: "1px solid var(--panel-border)",
        background: "rgba(4,5,6,0.94)",
        fontSize: 11,
        color: "var(--muted)",
      }}
    >
      <div className="event-strip" title="Latest presence event">
        {latest ? (
          <span className={`event-pill ${latest.level}`}>
            <i />
            {latest.message}
          </span>
        ) : (
          <span className="faint">Ready</span>
        )}
      </div>
      <span className="mono faint">Backend · A or , · Space talk · Esc close</span>
    </footer>
  );
}
