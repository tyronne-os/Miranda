import { useEffect, useState } from "react";
import type { AceNodeDef, NodeHealth } from "@/lib/stageMachine/types";

export type AceNodeTooltipModel = {
  def: AceNodeDef;
  health?: NodeHealth;
  latencyMs?: number;
  load?: number;
  incoming: number;
  outgoing: number;
};

type Props = {
  model: AceNodeTooltipModel | null;
  visible: boolean;
  onClose?: () => void;
};

/**
 * Educational detail card for ACE Cortex — select-only.
 * Pipeline stays clean until a node is chosen; card docks in-cortex, not on hover.
 */
export function AceNodeTooltip({ model, visible, onClose }: Props) {
  const [render, setRender] = useState(false);
  const [shown, setShown] = useState(false);

  useEffect(() => {
    if (visible && model) {
      setRender(true);
      const id = requestAnimationFrame(() => setShown(true));
      return () => cancelAnimationFrame(id);
    }
    setShown(false);
    const t = window.setTimeout(() => setRender(false), 160);
    return () => window.clearTimeout(t);
  }, [visible, model?.def.id]);

  if (!render || !model) return null;

  const { def, health, latencyMs, load, incoming, outgoing } = model;
  const intro =
    def.intro.length > 220 ? `${def.intro.slice(0, 217)}…` : def.intro;

  return (
    <div
      className={`ace-node-tooltip ace-node-tooltip-dock${shown ? " is-visible" : ""}`}
      role="dialog"
      aria-label={`${def.label} pipeline intro`}
      aria-hidden={!shown}
    >
      <div className="ace-node-tooltip-inner">
        <div className="ace-node-tooltip-head">
          <span className={`ace-plane-tag ${def.plane}`}>{def.plane}</span>
          <span className="ace-node-tooltip-stage">from {def.requiredFrom}</span>
          {health && <span className={`ace-health ${health}`}>{health}</span>}
          {onClose && (
            <button
              type="button"
              className="ace-node-tooltip-close"
              onClick={onClose}
              aria-label="Close node detail"
            >
              ×
            </button>
          )}
        </div>

        <h4 className="ace-node-tooltip-title">{def.label}</h4>
        <p className="ace-node-tooltip-role">{def.roleInPipeline}</p>

        <div className="ace-node-tooltip-stats">
          <span>
            <strong>{incoming}</strong> in
          </span>
          <span>
            <strong>{outgoing}</strong> out
          </span>
          <span className="mono">≤{latencyMs ?? def.latencyBudgetMs}ms</span>
          {typeof load === "number" && (
            <span className="mono">load {Math.round(load * 100)}%</span>
          )}
        </div>

        <p className="ace-node-tooltip-intro">{intro}</p>

        {def.tags.length > 0 && (
          <div className="ace-node-tooltip-tags">
            {def.tags.slice(0, 4).map((tag) => (
              <span key={tag} className="ace-node-tooltip-tag">
                {tag}
              </span>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
