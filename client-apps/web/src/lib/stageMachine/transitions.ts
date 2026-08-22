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
  // WO-5 T1: rewired for the real topology — "syncer" (Spatial Syncer) no
  // longer exists as a node; the L0 control plane is just mic capture + the
  // presence layer itself.
  const controlIds = ["mic", "presence"];
  return controlIds.every((id) => {
    const h = nodes[id]?.health;
    return h === "ready" || h === "hot";
  });
}

function isReadyOrHot(health: NodeHealth | undefined): boolean {
  return health === "ready" || health === "hot";
}

function isWarmingOrDegraded(health: NodeHealth | undefined): boolean {
  return health === "warming" || health === "degraded";
}

/**
 * WO-5 T3: evaluates one `hotNodeAlternatives` group. Readiness needs only
 * the BEST member of the group, not all of them — see the field's doc in
 * `types.ts` for why (two coexisting pipelines fulfilling one role).
 *
 * Returns "ready" if any member is ready/hot, "warming" if none are ready
 * but at least one is warming/degraded (so the UI can still show progress
 * rather than a flat "missing"), or "missing" if every member is cold.
 */
function evaluateAlternativeGroup(
  group: string[],
  nodes: Record<string, NodeRuntime>,
): "ready" | "warming" | "missing" {
  let anyWarming = false;
  for (const id of group) {
    const health = nodes[id]?.health;
    if (isReadyOrHot(health)) return "ready";
    if (isWarmingOrDegraded(health)) anyWarming = true;
  }
  return anyWarming ? "warming" : "missing";
}

export function stageReadiness(
  stage: PresenceStage,
  nodes: Record<string, NodeRuntime>,
): { ready: boolean; missing: string[]; warming: string[]; score: number } {
  const contract = getContract(stage);
  const missing: string[] = [];
  const warming: string[] = [];
  let hot = 0;
  let total = contract.hotNodes.length;

  for (const id of contract.hotNodes) {
    const health = nodes[id]?.health ?? "cold";
    if (isReadyOrHot(health)) {
      hot += 1;
    } else if (isWarmingOrDegraded(health)) {
      warming.push(id);
    } else {
      missing.push(id);
    }
  }

  // Each alternatives group counts as exactly one readiness unit, evaluated
  // by its best member — not one unit per member, which would make an
  // alternatives group easier to satisfy than an equivalent single hard
  // requirement and skew the score.
  for (const group of contract.hotNodeAlternatives ?? []) {
    total += 1;
    const status = evaluateAlternativeGroup(group, nodes);
    if (status === "ready") {
      hot += 1;
    } else if (status === "warming") {
      warming.push(group.join("|"));
    } else {
      missing.push(group.join("|"));
    }
  }

  const score = total ? hot / total : 1;

  return {
    ready: missing.length === 0 && warming.length === 0 && controlPlaneReady(nodes),
    missing,
    warming,
    score,
  };
}

/** True if `nodeId` is a hard requirement or belongs to one of the
 * contract's alternatives groups (WO-5 T3). Exported so other consumers
 * (e.g. `presenceStore.ts`'s warm-pull logic) don't re-implement this check
 * against `hotNodes` alone and silently miss the alternatives groups. */
export function isHotOrAlternative(contract: StageContract, nodeId: string): boolean {
  if (contract.hotNodes.includes(nodeId)) return true;
  return (contract.hotNodeAlternatives ?? []).some((group) => group.includes(nodeId));
}

export function healthForStageRole(
  nodeId: string,
  stage: PresenceStage,
  progress: number,
): NodeHealth {
  const contract = getContract(stage);
  if (isHotOrAlternative(contract, nodeId)) {
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
    isHotOrAlternative(c, nodeId),
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
