import { useState, useRef, useEffect, useCallback } from "react";
import {
  Activity,
  Camera,
  Clock3,
  FlaskConical,
  Mic,
  MicOff,
  Radio,
  RotateCcw,
  Save,
  SlidersHorizontal,
  Sparkles,
  Triangle,
  X,
  Zap,
  Send,
} from "lucide-react";
import { STAGE_CONTRACTS } from "@/data/aceTopology";
import { BlendshapeMeter } from "@/components/eve/BlendshapeMeter";
import { listenOnce, speakText, canListen } from "@/lib/ace/eveVoice";
import { usePresenceStore, type StillChoice } from "@/store/presenceStore";

interface Props {
  open: boolean;
  onClose: () => void;
}

const STILL_OPTIONS: Array<{ id: StillChoice; label: string; hint: string }> = [
  { id: "auto", label: "Auto by stage", hint: "Portrait idle · close-up on L1 talk / L2" },
  { id: "natural", label: "Portrait · natural", hint: "eve-natural.png only" },
  { id: "closeup", label: "Close-up · detail", hint: "eve-closeup.jpg only" },
];

const CONTROLLER = import.meta.env.VITE_ACE_HTTP_URL || "http://127.0.0.1:8100";

interface LabSettings {
  model: string;
  temperature: number;
  topP: number;
  maxTokens: number;
  memoryTurns: number;
}

/**
 * Persona Lab — who she IS, as an editable dial.
 *
 * This is the bench where a real lab builds a person: instruction file,
 * sampling behaviour, memory depth. It lives in the backend drawer by
 * design — the front must stay free of anything that colours the judgment
 * of how close she came to being real.
 */
function PersonaLab() {
  const [text, setText] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [meta, setMeta] = useState<{ source: string; chars: number; memoryTurns: number } | null>(
    null,
  );
  const [lab, setLab] = useState<LabSettings | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [p, s] = await Promise.all([
        fetch(`${CONTROLLER}/v1/persona`).then((r) => r.json()),
        fetch(`${CONTROLLER}/v1/settings`).then((r) => r.json()),
      ]);
      setText(p.text || "");
      setMeta({ source: p.source, chars: p.chars, memoryTurns: p.memoryTurns });
      setLab({
        model: s.model,
        temperature: s.temperature,
        topP: s.topP,
        maxTokens: s.maxTokens,
        memoryTurns: s.memoryTurns,
      });
      setLoaded(true);
      setDirty(false);
    } catch {
      setStatus("controller offline");
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const save = async () => {
    setStatus("saving…");
    try {
      const res = await fetch(`${CONTROLLER}/v1/persona`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ text }),
      });
      const data = await res.json();
      if (data.ok) {
        setStatus(`saved · ${data.chars} chars · hot-reloaded`);
        setDirty(false);
        void refresh();
      } else {
        setStatus(data.error || "save failed");
      }
    } catch {
      setStatus("save failed — controller unreachable");
    }
  };

  const patchLab = async (patch: Partial<LabSettings>) => {
    if (!lab) return;
    const next = { ...lab, ...patch };
    setLab(next);
    try {
      await fetch(`${CONTROLLER}/v1/settings`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(patch),
      });
    } catch {
      setStatus("settings not applied");
    }
  };

  const resetMemory = async () => {
    try {
      await fetch(`${CONTROLLER}/v1/memory/reset`, { method: "POST" });
      setStatus("conversational memory cleared");
      void refresh();
    } catch {
      setStatus("reset failed");
    }
  };

  return (
    <>
      <div className="side-card">
        <div className="side-card-title">
          <FlaskConical size={13} /> Persona Lab
        </div>

        <textarea
          className="persona-editor"
          value={text}
          spellCheck={false}
          placeholder={loaded ? "" : "loading instruction file…"}
          onChange={(e) => {
            setText(e.target.value);
            setDirty(true);
          }}
          rows={14}
        />

        <div className="persona-actions">
          <button
            type="button"
            className="ctrl-btn primary"
            onClick={save}
            disabled={!dirty || text.trim().length < 50}
          >
            <Save size={13} />
            Save &amp; hot-reload
          </button>
          <button type="button" className="ctrl-btn" onClick={() => void refresh()}>
            <RotateCcw size={13} />
            Revert
          </button>
        </div>

        {meta && (
          <div className="persona-meta mono">
            <span>{meta.source}</span>
            <span>{meta.chars} chars</span>
            <span>{meta.memoryTurns} turns held</span>
            {dirty && <span className="persona-dirty">unsaved</span>}
          </div>
        )}
        {status && <p className="admin-note">{status}</p>}
        <p className="admin-note">
          Who she is, as a file. Saving hot-reloads the instruction on the next turn — no restart.
          Never surfaced on the front; the mirror stays uncoloured.
        </p>
      </div>

      {lab && (
        <div className="side-card">
          <div className="side-card-title">
            <SlidersHorizontal size={13} /> Cognition dials
          </div>

          <label className="lab-field">
            <span>Model</span>
            <input
              type="text"
              className="mono"
              value={lab.model}
              onChange={(e) => setLab({ ...lab, model: e.target.value })}
              onBlur={(e) => void patchLab({ model: e.target.value })}
            />
          </label>

          <label className="lab-field">
            <span>
              Temperature <b className="mono">{lab.temperature.toFixed(2)}</b>
            </span>
            <input
              type="range"
              min={0}
              max={1.4}
              step={0.05}
              value={lab.temperature}
              onChange={(e) => setLab({ ...lab, temperature: Number(e.target.value) })}
              onPointerUp={(e) =>
                void patchLab({ temperature: Number((e.target as HTMLInputElement).value) })
              }
            />
          </label>

          <label className="lab-field">
            <span>
              Top-p <b className="mono">{lab.topP.toFixed(2)}</b>
            </span>
            <input
              type="range"
              min={0.1}
              max={1}
              step={0.05}
              value={lab.topP}
              onChange={(e) => setLab({ ...lab, topP: Number(e.target.value) })}
              onPointerUp={(e) =>
                void patchLab({ topP: Number((e.target as HTMLInputElement).value) })
              }
            />
          </label>

          <label className="lab-field">
            <span>
              Reply ceiling <b className="mono">{lab.maxTokens} tok</b>
            </span>
            <input
              type="range"
              min={64}
              max={1024}
              step={32}
              value={lab.maxTokens}
              onChange={(e) => setLab({ ...lab, maxTokens: Number(e.target.value) })}
              onPointerUp={(e) =>
                void patchLab({ maxTokens: Number((e.target as HTMLInputElement).value) })
              }
            />
          </label>

          <label className="lab-field">
            <span>
              Memory depth <b className="mono">{lab.memoryTurns} turns</b>
            </span>
            <input
              type="range"
              min={0}
              max={30}
              step={1}
              value={lab.memoryTurns}
              onChange={(e) => setLab({ ...lab, memoryTurns: Number(e.target.value) })}
              onPointerUp={(e) =>
                void patchLab({ memoryTurns: Number((e.target as HTMLInputElement).value) })
              }
            />
          </label>

          <button type="button" className="ctrl-btn edge-wide" onClick={resetMemory}>
            <RotateCcw size={13} />
            Clear conversational memory
          </button>
          <p className="admin-note">
            Memory depth 0 makes her amnesiac between turns — useful for isolating whether presence
            or continuity is doing the work.
          </p>
        </div>
      )}
    </>
  );
}

/**
 * Backend studio — every tool, meter, and advanced setting lives here.
 * Front Live Studio stays mirror-clear for beauty measurement.
 */
export function StudioAdminDrawer({ open, onClose }: Props) {
  const stage = usePresenceStore((s) => s.stage);
  const targetStage = usePresenceStore((s) => s.targetStage);
  const blend = usePresenceStore((s) => s.blend);
  const energy = usePresenceStore((s) => s.presenceEnergy);
  const clock = usePresenceStore((s) => s.clock);
  const warmProgress = usePresenceStore((s) => s.warmProgress);
  const controlMs = usePresenceStore((s) => s.controlMs);
  const dataPlaneLabel = usePresenceStore((s) => s.dataPlaneLabel);
  const phaseLabel = usePresenceStore((s) => s.phaseLabel);
  const autoWarm = usePresenceStore((s) => s.autoWarm);
  const micArmed = usePresenceStore((s) => s.micArmed);
  const talking = usePresenceStore((s) => s.talking);
  const stillChoice = usePresenceStore((s) => s.stillChoice);
  const setAutoWarm = usePresenceStore((s) => s.setAutoWarm);
  const setMicArmed = usePresenceStore((s) => s.setMicArmed);
  const setStillChoice = usePresenceStore((s) => s.setStillChoice);
  const requestStage = usePresenceStore((s) => s.requestStage);
  const promote = usePresenceStore((s) => s.promote);
  const demote = usePresenceStore((s) => s.demote);
  const busLink = usePresenceStore((s) => s.busLink);
  const busDetail = usePresenceStore((s) => s.busDetail);
  const lastTalk = usePresenceStore((s) => s.lastTalk);
  const talkPending = usePresenceStore((s) => s.talkPending);
  const sendTalk = usePresenceStore((s) => s.sendTalk);
  const beginListenGate = usePresenceStore((s) => s.beginListenGate);
  const endListenGate = usePresenceStore((s) => s.endListenGate);

  const [talkInput, setTalkInput] = useState("");
  const listenStopRef = useRef<(() => void) | null>(null);

  const contract = STAGE_CONTRACTS[stage];

  const handleSendTalk = async () => {
    const text = talkInput.trim();
    if (!text) return;
    setTalkInput("");
    const result = await sendTalk(text);
    if (result.reply && canListen()) {
      speakText(result.reply, {
        onStart: () => { },
        onEnd: () => { },
      });
    }
  };

  const handleHoldTalkStart = () => {
    if (!micArmed || !canListen()) return;
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
  };

  const handleHoldTalkEnd = () => {
    if (listenStopRef.current) {
      listenStopRef.current();
      listenStopRef.current = null;
    }
  };

  const getBusLinkColor = (link: string) => {
    switch (link) {
      case "live":
        return "var(--success-color, #0f0)";
      case "mock":
        return "var(--warn-color, #fa0)";
      case "connecting":
        return "var(--info-color, #0af)";
      case "error":
        return "var(--error-color, #f00)";
      default:
        return "var(--dim-color, #666)";
    }
  };

  return (
    <>
      <button
        type="button"
        className={`studio-drawer-scrim${open ? " open" : ""}`}
        aria-label="Close backend admin"
        tabIndex={open ? 0 : -1}
        onClick={onClose}
      />
      <aside
        className={`studio-admin-drawer${open ? " open" : ""}`}
        aria-hidden={!open}
        aria-label="Backend studio admin"
      >
        <div className="studio-admin-head">
          <div>
            <strong>Backend Studio</strong>
            <span>Advanced settings · telemetry · transport</span>
          </div>
          <button type="button" className="icon-btn" onClick={onClose} aria-label="Close">
            <X size={16} />
          </button>
        </div>

        <div className="studio-admin-body">
          <div className="side-card">
            <div className="side-card-title">
              <Radio size={13} /> Transport
            </div>
            <div className="admin-btn-col">
              <button
                type="button"
                className={`ctrl-btn edge-wide${micArmed ? " on" : ""}`}
                onClick={() => setMicArmed(!micArmed)}
              >
                {micArmed ? <Mic size={16} /> : <MicOff size={16} />}
                {micArmed ? "Mic armed" : "Mic muted"}
              </button>
              <button
                type="button"
                className={`ctrl-btn talk edge-wide${talking ? " active" : ""}`}
                onMouseDown={handleHoldTalkStart}
                onMouseUp={handleHoldTalkEnd}
                onMouseLeave={handleHoldTalkEnd}
                onTouchStart={(e) => {
                  e.preventDefault();
                  handleHoldTalkStart();
                }}
                onTouchEnd={handleHoldTalkEnd}
              >
                <Radio size={16} />
                {talkPending ? "Thinking…" : "Hold to talk"}
              </button>
            </div>
            <p className="admin-note">
              Front is mirror-clear. Space = hold talk · A or , = open this panel · Esc = close.
            </p>
          </div>

          <div className="side-card">
            <div className="side-card-title">
              <Send size={13} /> Live Talk
            </div>
            <div className="talk-bus-pill">
              <span
                className="bus-link-badge"
                style={{
                  backgroundColor: getBusLinkColor(busLink),
                  opacity: busLink === "offline" ? 0.3 : 0.9,
                }}
              />
              <span className="bus-link-label">
                ACE: {busLink.toUpperCase()}{busDetail ? ` · ${busDetail}` : ""}
              </span>
            </div>
            <div className="talk-compose">
              <textarea
                placeholder="Type message or hold Space / 'Hold to talk' button…"
                value={talkInput}
                onChange={(e) => setTalkInput(e.target.value)}
                onKeyDown={(e) => {
                  if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
                    handleSendTalk();
                  }
                }}
                disabled={!micArmed}
                rows={3}
              />
              <button
                type="button"
                className="ctrl-btn primary talk-send"
                onClick={handleSendTalk}
                disabled={!talkInput.trim() || !micArmed || talkPending}
              >
                <Send size={14} />
                Send
              </button>
            </div>
            {lastTalk && (
              <div className="last-talk-box">
                <div className="talk-reply">
                  <strong>EVE:</strong> {lastTalk.reply || "(no reply)"}
                </div>
                <div className="talk-meta">
                  {lastTalk.model && <span className="meta-item">Model: {lastTalk.model}</span>}
                  {lastTalk.latencyMs != null && <span className="meta-item">{"⏱ " + lastTalk.latencyMs}ms</span>}
                  {lastTalk.error && <span className="meta-item error">Error: {lastTalk.error}</span>}
                </div>
              </div>
            )}
            <p className="admin-note">
              Speech Recognition (browser) + NVIDIA Nemotron + Text-to-Speech. Instant Presence L1 data plane.
            </p>
          </div>

          <PersonaLab />

          <div className="side-card">
            <div className="side-card-title">
              <Zap size={13} /> Presence stage
            </div>
            <div className="admin-stage-grid">
              {(["L0", "L1", "L2"] as const).map((s) => (
                <button
                  key={s}
                  type="button"
                  className={`edge-item admin-stage-item${stage === s ? " active" : ""}${targetStage === s && stage !== s ? " target" : ""
                    }`}
                  onClick={() => requestStage(s)}
                >
                  <strong>{s}</strong>
                  <small>{STAGE_CONTRACTS[s].title}</small>
                </button>
              ))}
            </div>
            <div className="edge-flyout-row">
              <button type="button" className="ctrl-btn" onClick={demote}>
                <Triangle size={12} style={{ transform: "rotate(-90deg)" }} />
                Down
              </button>
              <button type="button" className="ctrl-btn primary" onClick={promote}>
                <Triangle size={12} style={{ transform: "rotate(90deg)" }} />
                Up
              </button>
            </div>
          </div>

          <div className="side-card">
            <div className="side-card-title">
              <Camera size={13} /> Photoreal still binding
            </div>
            <div className="admin-btn-col">
              {STILL_OPTIONS.map((opt) => (
                <button
                  key={opt.id}
                  type="button"
                  className={`edge-item${stillChoice === opt.id ? " active" : ""}`}
                  onClick={() => setStillChoice(opt.id)}
                >
                  <strong>{opt.label}</strong>
                  <small>{opt.hint}</small>
                </button>
              ))}
            </div>
            <p className="admin-note">
              Zero Placeholders: only user-approved staff stills. No stock, SVG, or drawn fallbacks.
            </p>
          </div>

          <div className="side-card">
            <div className="side-card-title">
              <Sparkles size={13} /> Presence contract
            </div>
            <p>{contract.subtitle}</p>
            <ul className="contract-list">
              <li>
                Phase <strong>{phaseLabel}</strong>
              </li>
              <li>
                Control budget{" "}
                <strong className="mono">&lt;{contract.controlBudgetMs}ms</strong>
              </li>
              <li>
                Control plane <strong className="mono">{controlMs}ms</strong>
              </li>
              <li>
                Data plane <strong>{dataPlaneLabel}</strong>
              </li>
              <li>
                Warm path <strong className="mono">{Math.round(warmProgress * 100)}%</strong>
              </li>
              <li>
                ARKit blendshapes{" "}
                <strong>{contract.requiresBlendshapes ? "required" : "still idle"}</strong>
              </li>
              <li>
                Omniverse stream{" "}
                <strong>{contract.requiresPixelStream ? "cinematic on" : "not required"}</strong>
              </li>
            </ul>
            <label className="auto-warm drawer-toggle">
              <input
                type="checkbox"
                checked={autoWarm}
                onChange={(e) => setAutoWarm(e.target.checked)}
              />
              Auto-warm data plane
            </label>
          </div>

          <BlendshapeMeter frame={blend} />

          <div className="side-card clock-card">
            <div className="side-card-title">
              <Clock3 size={13} /> Spatial Syncer
            </div>
            <div className="clock-grid mono">
              <div>
                <span className="faint">tMedia</span>
                <strong>{(clock.tMediaMs / 1000).toFixed(2)}s</strong>
              </div>
              <div>
                <span className="faint">drift</span>
                <strong>{clock.driftMs.toFixed(1)}ms</strong>
              </div>
              <div>
                <span className="faint">pps</span>
                <strong>{clock.pps}</strong>
              </div>
              <div>
                <span className="faint">energy</span>
                <strong>{energy.toFixed(2)}</strong>
              </div>
            </div>
          </div>

          <div className="side-card">
            <div className="side-card-title">
              <Activity size={13} /> Vanity front rule
            </div>
            <p className="admin-note">
              Live Studio is a beauty mirror. Tools and indicators stay in this backend panel and
              the ACE Cortex side pane — never as permanent chrome over EVE.
            </p>
          </div>
        </div>
      </aside>
    </>
  );
}
