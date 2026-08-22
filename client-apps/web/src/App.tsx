import { useCallback, useEffect, useState } from "react";
import { AppHeader } from "./components/layout/AppHeader";
import { AppFooter } from "./components/layout/AppFooter";
import { AceCortexPane } from "./components/pipeline/AceCortexPane";
import { EveStudioPane } from "./components/eve/EveStudioPane";
import { UponLoadBadge } from "./components/metrics/UponLoadBadge";
import { usePresenceLoop } from "./lib/stageMachine/usePresenceLoop";

const SPLIT_KEY = "eve-ecc.split";
const DEFAULT_SPLIT = 58;

function clampSplit(value: number) {
  return Math.min(72, Math.max(38, value));
}

export default function App() {
  usePresenceLoop(true);

  const [split, setSplit] = useState(() => {
    const saved = Number(localStorage.getItem(SPLIT_KEY));
    return Number.isFinite(saved) ? clampSplit(saved) : DEFAULT_SPLIT;
  });
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    localStorage.setItem(SPLIT_KEY, String(split));
    document.documentElement.style.setProperty("--split", `${split}%`);
  }, [split]);

  const onPointerDown = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    setDragging(true);
    event.currentTarget.setPointerCapture(event.pointerId);
  }, []);

  const onPointerMove = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (!dragging) return;
      const workspace = event.currentTarget.parentElement;
      if (!workspace) return;
      const rect = workspace.getBoundingClientRect();
      const next = ((event.clientX - rect.left) / rect.width) * 100;
      setSplit(clampSplit(next));
    },
    [dragging],
  );

  const onPointerUp = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    setDragging(false);
    try {
      event.currentTarget.releasePointerCapture(event.pointerId);
    } catch {
      /* already released */
    }
  }, []);

  return (
    <div className="app-shell">
      <AppHeader />
      <main className="workspace" aria-label="EVE ECC dual-pane workspace">
        <section className="pane pane-left" aria-label="ACE cortex">
          <AceCortexPane />
        </section>
        <div
          className="splitter"
          role="separator"
          aria-orientation="vertical"
          aria-valuenow={Math.round(split)}
          aria-label="Resize panes"
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
        />
        <section className="pane pane-right" aria-label="EVE live studio">
          <EveStudioPane />
        </section>
      </main>
      <AppFooter />
      <UponLoadBadge />
    </div>
  );
}
