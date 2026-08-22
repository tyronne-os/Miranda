import { STAGE_CONTRACTS } from "@/data/aceTopology";
import type {
  NodeHealth,
  NodeRuntime,
  PresenceStage,
  StageContract,
} from "./types";
import { STAGE_ORDER } from "./types";

export function stageIndex(stage: PresenceStage): number {
  return STAGE_ORDER.indexOf(stage);
}

export function canAscend(from: PresenceStage, to: PresenceStage): boolean {
  return stageIndex(to) === stageIndex(from) + 1;
}

export function canDescend(from: PresenceStage, to: PresenceStage): boolean {
  return stageIndex(to) < stageIndex(from);
}

export function nextStage(stage: PresenceStage): PresenceStage | null {
  const i = stageIndex(stage);
  return i < STAGE_ORDER.length - 1 ? STAGE_ORDER[i + 1]! : null;
}

export function prevStage(stage: PresenceStage): PresenceStage | null {
  const i = stageIndex(stage);
  return i > 0 ? STAGE_ORDER[i - 1]! : null;
}

export function getContract(stage: PresenceStage): StageContract {
  return STAGE_CONTRACTS[stage];
}

/** Instant Presence gate: control nodes must be ready/hot before any stage holds. */
export function controlPlaneReady(nodes: Record<string, NodeRuntime>): boolean {
  const controlIds = ["mic", "presence", "syncer"];
  return controlIds.every((id) => {
    const h = nodes[id]?.health;
    return h === "ready" || h === "hot";
  });
}

export function stageReadiness(
  stage: PresenceStage,
  nodes: Record<string, NodeRuntime>,
): { ready: boolean; missing: string[]; warming: string[]; score: number } {
  const contract = getContract(stage);
  const missing: string[] = [];
  const warming: string[] = [];
  let hot = 0;

  for (const id of contract.hotNodes) {
    const health = nodes[id]?.health ?? "cold";
    if (health === "ready" || health === "hot") {
      hot += 1;
    } else if (health === "warming" || health === "degraded") {
      warming.push(id);
    } else {
      missing.push(id);
    }
  }

  const score = contract.hotNodes.length
    ? hot / contract.hotNodes.length
    : 1;

  return {
    ready: missing.length === 0 && warming.length === 0 && controlPlaneReady(nodes),
    missing,
    warming,
    score,
  };
}

export function healthForStageRole(
  nodeId: string,
  stage: PresenceStage,
  progress: number,
): NodeHealth {
  const contract = getContract(stage);
  if (contract.hotNodes.includes(nodeId)) {
    if (progress >= 0.92) return "hot";
    if (progress >= 0.55) return "ready";
    if (progress >= 0.15) return "warming";
    return "cold";
  }
  if (contract.warmNodes.includes(nodeId)) {
    if (progress >= 0.7) return "warming";
    if (progress >= 0.35) return "cold";
    return "cold";
  }
  // Below required stage — keep cold but not error
  const required = Object.values(STAGE_CONTRACTS).find((c) =>
    c.hotNodes.includes(nodeId),
  );
  if (required && stageIndex(stage) < stageIndex(required.stage)) {
    return progress > 0.5 ? "warming" : "cold";
  }
  return "cold";
}

export function latencyForHealth(budgetMs: number, health: NodeHealth): number {
  switch (health) {
    case "hot":
      return Math.round(budgetMs * 0.55);
    case "ready":
      return Math.round(budgetMs * 0.85);
    case "warming":
      return Math.round(budgetMs * 1.6);
    case "degraded":
      return Math.round(budgetMs * 2.4);
    case "error":
      return Math.round(budgetMs * 4);
    default:
      return Math.round(budgetMs * 3);
  }
}
