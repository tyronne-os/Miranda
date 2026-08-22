import { useEffect, useRef } from "react";
import { SlidersHorizontal } from "lucide-react";
import { StudioAdminDrawer } from "@/components/admin/StudioAdminDrawer";
import { usePresenceStore } from "@/store/presenceStore";
import { listenOnce, speakText, canListen } from "@/lib/ace/eveVoice";
import { EvePresenceViewport } from "./EvePresenceViewport";

/**
 * Vanity front surface — mirror-clear.
 * All tools, meters, stage, transport, and advanced settings live in
 * Studio Admin (backend drawer) and ACE Cortex (left pane).
 */
export function EveStudioPane() {
  const stage = usePresenceStore((s) => s.stage);
  const blend = usePresenceStore((s) => s.blend);
  const talking = usePresenceStore((s) => s.talking);
  const energy = usePresenceStore((s) => s.presenceEnergy);
  const warmProgress = usePresenceStore((s) => s.warmProgress);
  const stillChoice = usePresenceStore((s) => s.stillChoice);
  const adminOpen = usePresenceStore((s) => s.adminOpen);
  const micArmed = usePresenceStore((s) => s.micArmed);
  const setAdminOpen = usePresenceStore((s) => s.setAdminOpen);
  const beginListenGate = usePresenceStore((s) => s.beginListenGate);
  const endListenGate = usePresenceStore((s) => s.endListenGate);
  const sendTalk = usePresenceStore((s) => s.sendTalk);

  const stillOverride = stillChoice === "auto" ? undefined : stillChoice;
  const listenStopRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    const isTypingTarget = (t: EventTarget | null) => {
      if (!(t instanceof HTMLElement)) return false;
      const tag = t.tagName;
      return tag === "INPUT" || tag === "TEXTAREA" || t.isContentEditable;
    };

    const onKeyDown = (e: KeyboardEvent) => {
      if (isTypingTarget(e.target)) return;

      if (e.key === "Escape") {
        setAdminOpen(false);
        return;
      }

      if ((e.key === "," || e.key === "a" || e.key === "A") && !e.metaKey && !e.ctrlKey && !e.altKey) {
        e.preventDefault();
        setAdminOpen(true);
        return;
      }

      if (e.code === "Space" && !e.repeat) {
        e.preventDefault();
        if (micArmed && canListen()) {
          beginListenGate();
          listenStopRef.current = listenOnce({
            onFinal: async (text) => {
              if (text) {
                const result = await sendTalk(text);
                if (result.reply) {
                  speakText(result.reply, {
                    onStart: () => { },
                    onEnd: () => endListenGate(),
                  });
                } else {
                  endListenGate();
                }
              } else {
                endListenGate();
              }
            },
            onError: (err) => {
              console.warn("[listen]", err);
              endListenGate();
            },
            onEnd: () => {
              listenStopRef.current = null;
            },
          })?.stop || null;
        }
      }
    };

    const onKeyUp = (e: KeyboardEvent) => {
      if (isTypingTarget(e.target)) return;
      if (e.code === "Space") {
        e.preventDefault();
        if (listenStopRef.current) {
          listenStopRef.current();
          listenStopRef.current = null;
        }
      }
    };

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
    };
  }, [micArmed, setAdminOpen, beginListenGate, endListenGate, sendTalk]);

  return (
    <div className="pane-fill studio-hero">
      <div className="studio-stage-area">
        <EvePresenceViewport
          stage={stage}
          blend={blend}
          talking={talking}
          energy={energy}
          warmProgress={warmProgress}
          still={stillOverride}
        />

        <button
          type="button"
          className={`studio-backend-fab${adminOpen ? " active" : ""}`}
          onClick={() => setAdminOpen(true)}
          title="Open backend studio admin (A or ,)"
          aria-label="Open backend studio admin"
          aria-expanded={adminOpen}
        >
          <SlidersHorizontal size={15} />
          <span>Backend</span>
        </button>
      </div>

      <StudioAdminDrawer open={adminOpen} onClose={() => setAdminOpen(false)} />
    </div>
  );
}
