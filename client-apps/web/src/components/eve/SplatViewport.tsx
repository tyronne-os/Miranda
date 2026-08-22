import { useEffect, useRef, useState } from "react";
import { SplatRenderer } from "@/lib/splat/SplatRenderer";
import { mirandaTransport } from "@/lib/ace/mirandaTransport";

interface Props {
  /** Called once on successful WebGPU init so the parent can show/hide fallback. */
  onReady?: () => void;
  /** Called if WebGPU is unavailable or initialization fails. */
  onUnavailable?: (reason: string) => void;
}

/**
 * WO-5: mounts a WebGPU canvas and drives SplatRenderer with live frames
 * from miranda-transport's `/data` binary WebSocket hub.
 *
 * Falls back gracefully when WebGPU is unavailable — the parent
 * `EveStudioPane` keeps showing `EvePresenceViewport` at L0/L1; this overlay
 * only activates at L2 when a real GPU pipeline is confirmed present.
 *
 * # No React state on the hot path
 *
 * This component does NOT pipe network frames through `useState`. Doing so
 * would force a re-render on every incoming packet (up to 60 Hz), which is
 * exactly the cost `EvePresenceViewport`'s own module docs identify as
 * something to avoid on this project's dual-core target. Instead, feeding
 * frames into the renderer happens in a small `requestAnimationFrame` loop
 * owned by this component, reading `mirandaTransport.getLastFrame()`
 * directly and writing into `SplatRenderer` — no component re-render
 * involved. React only re-renders this component for its own lifecycle
 * events (mount, WebGPU ready/unavailable), never for frame data.
 */
export function SplatViewport({ onReady, onUnavailable }: Props) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const rendererRef = useRef<SplatRenderer | null>(null);
  const [gpuReady, setGpuReady] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    let cancelled = false;
    let feedRafId: number | null = null;

    SplatRenderer.create(canvas)
      .then((renderer) => {
        if (cancelled) {
          renderer.destroy();
          return;
        }
        rendererRef.current = renderer;
        renderer.startRenderLoop();
        setGpuReady(true);
        onReady?.();

        // Connect the data-plane WebSocket. Idempotent: if a sibling
        // consumer already called start(), this is a no-op — the singleton
        // client tracks its own connection state.
        mirandaTransport.start();

        // Feed loop: pull the latest decoded frame every animation frame
        // and hand it to the renderer directly. This is deliberately a
        // SECOND rAF loop, independent of the renderer's own — the
        // renderer's loop must keep running even if this feed loop is
        // ever removed or replaced, so the two are not merged into one.
        const feed = () => {
          if (cancelled) return;
          const { frame, receivedAtMs } = mirandaTransport.getLastFrame();
          if (frame) {
            rendererRef.current?.setWeights(frame, receivedAtMs);
          }
          feedRafId = requestAnimationFrame(feed);
        };
        feedRafId = requestAnimationFrame(feed);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        const message = err instanceof Error ? err.message : String(err);
        setError(message);
        onUnavailable?.(message);
      });

    return () => {
      cancelled = true;
      if (feedRafId !== null) cancelAnimationFrame(feedRafId);
      rendererRef.current?.destroy();
      rendererRef.current = null;
      // Do NOT call mirandaTransport.stop() here: other consumers (e.g. a
      // telemetry panel) may share the same singleton connection. Ownership
      // of start/stop lifetime belongs to whoever mounts the transport
      // connection at the app level, not to this one viewport.
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (error) {
    // Invisible — parent falls back to the CSS presence layer.
    return null;
  }

  return (
    <canvas
      ref={canvasRef}
      className={`splat-canvas${gpuReady ? " splat-canvas--ready" : ""}`}
      aria-label="EVE — WebGPU Gaussian splat render"
      aria-hidden={!gpuReady}
    />
  );
}
