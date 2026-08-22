import { memo } from "react";
import { Handle, Position, type NodeProps } from "@xyflow/react";
import clsx from "clsx";
import type { NodeHealth, NodeKind, PlaneMode } from "@/lib/stageMachine/types";

export type AceFlowNodeData = {
  label: string;
  kind: NodeKind;
  plane: PlaneMode;
  health: NodeHealth;
  latencyMs: number;
  load: number;
  message: string;
  selected: boolean;
  /** Soft highlight while educational tooltip is open on this node */
  hovered?: boolean;
};

const KIND_GLYPH: Record<NodeKind, string> = {
  ingress: "IN",
  asr: "ASR",
  agent: "LLM",
  tts: "TTS",
  a2f: "A2F",
  animgraph: "AG",
  omniverse: "OV",
  bus: "BUS",
  presence: "L0",
};

function AceNodeCardImpl({ data }: NodeProps & { data: AceFlowNodeData }) {
  return (
    <div
      className={clsx(
        "ace-node",
        `health-${data.health}`,
        data.plane,
        data.selected && "is-selected",
        data.hovered && "is-hovered",
      )}
    >
      <Handle type="target" position={Position.Left} className="ace-handle" />
      <div className="ace-node-top">
        <span className="ace-glyph">{KIND_GLYPH[data.kind]}</span>
        <span className={clsx("ace-plane-tag", data.plane)}>{data.plane}</span>
      </div>
      <div className="ace-node-label">{data.label}</div>
      <div className="ace-node-meta">
        <span className={clsx("ace-health", data.health)}>{data.health}</span>
        <span className="mono">{data.latencyMs}ms</span>
      </div>
      <div className="ace-load">
        <div className="ace-load-bar" style={{ width: `${Math.round(data.load * 100)}%` }} />
      </div>
      <div className="ace-node-msg">{data.message}</div>
      <Handle type="source" position={Position.Right} className="ace-handle" />
    </div>
  );
}

export const AceNodeCard = memo(AceNodeCardImpl);
