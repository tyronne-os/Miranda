import { useCallback, useEffect, useMemo, type CSSProperties } from "react";
import {
  Background,
  BackgroundVariant,
  Controls,
  ReactFlow,
  type Edge,
  type Node,
  type NodeTypes,
  useEdgesState,
  useNodesState,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { Radar, Zap } from "lucide-react";
import { ACE_EDGES, ACE_NODES, NODE_POSITIONS, STAGE_CONTRACTS } from "@/data/aceTopology";
import { stageReadiness } from "@/lib/stageMachine/transitions";
import { usePresenceStore } from "@/store/presenceStore";
import { AceNodeCard, type AceFlowNodeData } from "./AceNodeCard";
import { AceNodeTooltip, type AceNodeTooltipModel } from "./AceNodeTooltip";

const nodeTypes = { ace: AceNodeCard } as unknown as NodeTypes;

function edgeClass(kind: string, active: boolean) {
  return `ace-edge kind-${kind}${active ? " is-active" : ""}`;
}

function edgeStyle(kind: string, active: boolean): CSSProperties {
  if (!active) {
    return { stroke: "rgba(50, 150, 255, 0.48)", strokeWidth: 1.85 };
  }
  switch (kind) {
    case "audio":
      return { stroke: "#4ec4ff", strokeWidth: 2.35 };
    case "blendshape":
      return { stroke: "#8eb6ff", strokeWidth: 2.35 };
    case "pixel":
      return { stroke: "#f0c14d", strokeWidth: 2.35 };
    case "clock":
      return { stroke: "#3db4ff", strokeWidth: 1.9 };
    default:
      return { stroke: "#3db4ff", strokeWidth: 2.2 };
  }
}

function countEdges(nodeId: string) {
  let incoming = 0;
  let outgoing = 0;
  for (const e of ACE_EDGES) {
    if (e.target === nodeId) incoming += 1;
    if (e.source === nodeId) outgoing += 1;
  }
  return { incoming, outgoing };
}

export function AceCortexPane() {
  const stage = usePresenceStore((s) => s.stage);
  const targetStage = usePresenceStore((s) => s.targetStage);
  const nodesRuntime = usePresenceStore((s) => s.nodes);
  const selectedNodeId = usePresenceStore((s) => s.selectedNodeId);
  const warmProgress = usePresenceStore((s) => s.warmProgress);
  const selectNode = usePresenceStore((s) => s.selectNode);
  const requestStage = usePresenceStore((s) => s.requestStage);

  const initialNodes = useMemo<Node[]>(
    () =>
      ACE_NODES.map((def) => ({
        id: def.id,
        type: "ace",
        position: NODE_POSITIONS[def.id] ?? { x: 0, y: 0 },
        data: {
          label: def.label,
          kind: def.kind,
          plane: def.plane,
          health: "cold",
          latencyMs: def.latencyBudgetMs,
          load: 0,
          message: def.description,
          selected: false,
          hovered: false,
        } satisfies AceFlowNodeData,
      })),
    [],
  );

  const initialEdges = useMemo<Edge[]>(
    () =>
      ACE_EDGES.map((e) => ({
        id: e.id,
        source: e.source,
        target: e.target,
        label: e.label,
        className: edgeClass(e.kind, false),
        style: edgeStyle(e.kind, false),
        animated: false,
      })),
    [],
  );

  const [nodes, setNodes, onNodesChange] = useNodesState(initialNodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState(initialEdges);

  useEffect(() => {
    setNodes((prev) =>
      prev.map((n) => {
        const rt = nodesRuntime[n.id];
        const def = ACE_NODES.find((d) => d.id === n.id);
        if (!rt || !def) return n;
        return {
          ...n,
          data: {
            label: def.label,
            kind: def.kind,
            plane: def.plane,
            health: rt.health,
            latencyMs: rt.latencyMs,
            load: rt.load,
            message: rt.message,
            selected: selectedNodeId === n.id,
            hovered: false,
          } satisfies AceFlowNodeData,
        };
      }),
    );

    const hot = new Set(
      Object.entries(nodesRuntime)
        .filter(([, r]) => r.health === "hot" || r.health === "ready")
        .map(([id]) => id),
    );

    setEdges((prev) =>
      prev.map((e) => {
        const def = ACE_EDGES.find((d) => d.id === e.id);
        const kind = def?.kind ?? "control";
        const active = hot.has(e.source) && hot.has(e.target);
        return {
          ...e,
          animated: active && (kind === "audio" || kind === "blendshape" || kind === "pixel"),
          className: edgeClass(kind, active),
          style: edgeStyle(kind, active),
        };
      }),
    );
  }, [nodesRuntime, selectedNodeId, setNodes, setEdges]);

  const onNodeClick = useCallback(
    (_: React.MouseEvent, node: Node) => {
      selectNode(selectedNodeId === node.id ? null : node.id);
    },
    [selectNode, selectedNodeId],
  );

  const onPaneClick = useCallback(() => {
    selectNode(null);
  }, [selectNode]);

  const readiness = stageReadiness(stage, nodesRuntime);
  const contract = STAGE_CONTRACTS[stage];

  const detailModel = useMemo<AceNodeTooltipModel | null>(() => {
    if (!selectedNodeId) return null;
    const def = ACE_NODES.find((n) => n.id === selectedNodeId);
    if (!def) return null;
    const rt = nodesRuntime[selectedNodeId];
    const counts = countEdges(selectedNodeId);
    return {
      def,
      health: rt?.health,
      latencyMs: rt?.latencyMs ?? def.latencyBudgetMs,
      load: rt?.load,
      incoming: counts.incoming,
      outgoing: counts.outgoing,
    };
  }, [selectedNodeId, nodesRuntime]);

  return (
    <div className="pane-fill cortex-pane">
      <div className="pane-header">
        <div className="pane-title">
          <strong>ACE Cortex</strong>
          <span>Understand-Anything topology · All-NVIDIA path</span>
        </div>
        <div className="pane-actions">
          <span className="badge live">
            <span className="dot" />
            {contract.title}
          </span>
          <span className="badge">
            <Radar size={12} />
            Ready {Math.round(readiness.score * 100)}%
          </span>
          {targetStage !== stage && (
            <span className="badge warn">
              <span className="dot" />
              → {targetStage}
            </span>
          )}
        </div>
      </div>

      <div className="cortex-toolbar">
        {(["L0", "L1", "L2"] as const).map((s) => (
          <button
            key={s}
            type="button"
            className={`stage-chip${stage === s ? " active" : ""}${targetStage === s && stage !== s ? " target" : ""}`}
            onClick={() => requestStage(s)}
          >
            <Zap size={12} />
            {s}
            <em>{STAGE_CONTRACTS[s].title}</em>
          </button>
        ))}
        <div className="warm-meter" title="Warm path progress">
          <div className="warm-meter-fill" style={{ width: `${Math.round(warmProgress * 100)}%` }} />
          <span>warm {Math.round(warmProgress * 100)}%</span>
        </div>
      </div>

      <div className="pane-body cortex-body">
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onNodeClick={onNodeClick}
          onPaneClick={onPaneClick}
          nodeTypes={nodeTypes}
          fitView
          fitViewOptions={{ padding: 0.22, includeHiddenNodes: false }}
          minZoom={0.3}
          maxZoom={1.6}
          proOptions={{ hideAttribution: true }}
          colorMode="dark"
          nodesDraggable
          nodesConnectable={false}
          elementsSelectable
        >
          <Background variant={BackgroundVariant.Dots} gap={20} size={1.1} color="rgba(61,180,255,0.16)" />
          <Controls showInteractive={false} />
        </ReactFlow>

        {/* Select-only — never mounts education chrome without an explicit node pick */}
        {detailModel && (
          <AceNodeTooltip
            model={detailModel}
            visible
            onClose={() => selectNode(null)}
          />
        )}
      </div>
    </div>
  );
}
