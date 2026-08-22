import { useEffect } from "react";
import { aceController } from "@/lib/ace/controllerClient";
import { usePresenceStore } from "@/store/presenceStore";
import { uponLoad } from "@/lib/metrics/uponLoad";
import { loadRealnessProfile } from "@/lib/metrics/realness";

/** 30 Hz Instant Presence tick — control plane never waits on data plane. */
export function usePresenceLoop(enabled = true) {
  useEffect(() => {
    if (!enabled) return;
    // Pull the lab's realness profile before the first motion frame lands.
    void loadRealnessProfile();
    aceController.configure({
      onSnapshot: (snap) => usePresenceStore.getState().applyAceSnapshot(snap),
      onLink: (state, detail) => {
        // UPON LOAD: cortex reachable — she can answer, not just exist.
        if (state === "live" || state === "mock") uponLoad.mark("busMs");
        usePresenceStore.getState().setBusLink(state, detail);
      },
      onVisemes: (frames, durationMs, source) =>
        usePresenceStore.getState().ingestVisemes(frames, durationMs, source),
    });
    aceController.start();
    return () => aceController.stop();
  }, [enabled]);

  useEffect(() => {
    if (!enabled) return;
    usePresenceStore.getState().bootstrap();
    let frame = 0;
    let last = performance.now();
    let raf = 0;

    const loop = (now: number) => {
      const dt = Math.min(64, now - last);
      last = now;
      frame += 1;
      // Cap store writes ~30fps
      if (frame % 2 === 0) {
        usePresenceStore.getState().tick(dt * 2);
      }
      raf = requestAnimationFrame(loop);
    };

    raf = requestAnimationFrame(loop);

    // rAF starves when the pane is hidden — the control plane must not.
    // Instant Presence rule: state machine keeps ticking in background.
    const fallback = window.setInterval(() => {
      const now = performance.now();
      if (now - last > 200) {
        const dt = Math.min(200, now - last);
        last = now;
        usePresenceStore.getState().tick(dt);
      }
    }, 66);

    return () => {
      cancelAnimationFrame(raf);
      window.clearInterval(fallback);
    };
  }, [enabled]);
}
