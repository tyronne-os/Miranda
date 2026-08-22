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

// WO-5 T1: rewired for the real Miranda-Engine NodeKind set. TypeScript's
// Record<NodeKind, string> makes this a compile error if a kind is ever
// added or removed without a matching glyph — that's load-bearing, not
// incidental.
const KIND_GLYPH: Record<NodeKind, string> = {
  ingress: "IN",
  "native-capture": "CAP",
  "ipc-bus": "IPC",
  kinematics: "KIN",
  supervisor: "SUP",
  transport: "NET",
  "cloud-bridge": "AWS",
  renderer: "GPU",
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
