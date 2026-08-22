import { describe, expect, it } from "vitest";
import type { NodeRuntime } from "./types";
import {
  controlPlaneReady,
  getContract,
  healthForStageRole,
  isHotOrAlternative,
  stageReadiness,
} from "./transitions";

/**
 * WO-5 T3 — coverage for the two-pipeline stage-readiness logic.
 *
 * Miranda-Engine runs two coexisting pipelines that both fulfil the same
 * "speech is being processed" role: Pipeline 1 (`cloud-bridge`) and
 * Pipeline 2 (`native-capture` → ... → `kinematics`). Before this fix,
 * `STAGE_CONTRACTS.L1.hotNodes` required BOTH to be hot, which would make
 * a Pipeline-1-only session — the live pipeline today — permanently unable
 * to reach L1, since `native-capture`/`kinematics` never warm in that
 * session. These tests exist specifically to pin that behavior down, not
 * just to exercise the code.
 */

function rt(health: NodeRuntime["health"]): NodeRuntime {
  return { id: "x", health, latencyMs: 10, load: 0.1, lastBeatMs: 0, message: "" };
}

function baseNodes(): Record<string, NodeRuntime> {
  return {
    mic: rt("ready"),
    presence: rt("ready"),
  };
}

describe("controlPlaneReady", () => {
  it("is true when mic and presence are both ready", () => {
    expect(controlPlaneReady(baseNodes())).toBe(true);
  });

  it("is false when mic is cold", () => {
    const nodes = { ...baseNodes(), mic: rt("cold") };
    expect(controlPlaneReady(nodes)).toBe(false);
  });

  it("does not require syncer — that node no longer exists in the real topology", () => {
    // Deliberately omit any "syncer" key entirely; must still pass.
    expect(controlPlaneReady(baseNodes())).toBe(true);
  });
});

describe("stageReadiness — L1 with alternative pipelines", () => {
  it("reaches L1 with ONLY Pipeline 1 (cloud-bridge) hot, kinematics cold", () => {
    const nodes: Record<string, NodeRuntime> = {
      ...baseNodes(),
      "cloud-bridge": rt("hot"),
      kinematics: rt("cold"),
      "native-capture": rt("cold"),
      "ipc-bus": rt("cold"),
      supervisor: rt("cold"),
      transport: rt("cold"),
    };
    const result = stageReadiness("L1", nodes);
    expect(result.ready).toBe(true);
    expect(result.missing).toHaveLength(0);
  });

  it("reaches L1 with ONLY Pipeline 2 (kinematics) hot, cloud-bridge cold", () => {
    const nodes: Record<string, NodeRuntime> = {
      ...baseNodes(),
      "cloud-bridge": rt("cold"),
      kinematics: rt("hot"),
      "native-capture": rt("cold"),
      "ipc-bus": rt("cold"),
      supervisor: rt("cold"),
      transport: rt("cold"),
    };
    const result = stageReadiness("L1", nodes);
    expect(result.ready).toBe(true);
  });

  it("does NOT reach L1 when both cloud-bridge and kinematics are cold", () => {
    const nodes: Record<string, NodeRuntime> = {
      ...baseNodes(),
      "cloud-bridge": rt("cold"),
      kinematics: rt("cold"),
    };
    const result = stageReadiness("L1", nodes);
    expect(result.ready).toBe(false);
    expect(result.missing).toContain("cloud-bridge|kinematics");
  });

  it("reports the alternative group as warming, not missing, when one member is warming", () => {
    const nodes: Record<string, NodeRuntime> = {
      ...baseNodes(),
      "cloud-bridge": rt("warming"),
      kinematics: rt("cold"),
    };
    const result = stageReadiness("L1", nodes);
    expect(result.missing).not.toContain("cloud-bridge|kinematics");
    expect(result.warming).toContain("cloud-bridge|kinematics");
    expect(result.ready).toBe(false); // still not ready — warming, not hot
  });

  it("counts an alternatives group as exactly one readiness unit in the score", () => {
    // mic + presence (hard) + one alternatives group = 3 units total.
    // With mic+presence ready and cloud-bridge hot, score should be 3/3 = 1,
    // not 4/4 or 2/2 — i.e. the group must not be double- or under-counted.
    const nodes: Record<string, NodeRuntime> = {
      ...baseNodes(),
      "cloud-bridge": rt("hot"),
      kinematics: rt("cold"),
    };
    const result = stageReadiness("L1", nodes);
    expect(result.score).toBe(1);
  });
});

describe("stageReadiness — L2 requires transport+renderer unconditionally", () => {
  it("does not reach L2 on cloud-bridge alone if transport/renderer are cold", () => {
    const nodes: Record<string, NodeRuntime> = {
      ...baseNodes(),
      "cloud-bridge": rt("hot"),
      transport: rt("cold"),
      renderer: rt("cold"),
    };
    const result = stageReadiness("L2", nodes);
    expect(result.ready).toBe(false);
    expect(result.missing).toEqual(expect.arrayContaining(["transport", "renderer"]));
  });

  it("reaches L2 with cloud-bridge + transport + renderer hot (native-capture cold)", () => {
    const nodes: Record<string, NodeRuntime> = {
      ...baseNodes(),
      "cloud-bridge": rt("hot"),
      kinematics: rt("cold"),
      transport: rt("hot"),
      renderer: rt("hot"),
    };
    const result = stageReadiness("L2", nodes);
    expect(result.ready).toBe(true);
  });
});

describe("isHotOrAlternative", () => {
  it("returns true for a plain hotNodes member", () => {
    const contract = getContract("L2");
    expect(isHotOrAlternative(contract, "transport")).toBe(true);
  });

  it("returns true for a member of an alternatives group", () => {
    const contract = getContract("L1");
    expect(isHotOrAlternative(contract, "cloud-bridge")).toBe(true);
    expect(isHotOrAlternative(contract, "kinematics")).toBe(true);
  });

  it("returns false for a node in neither hotNodes nor any alternatives group", () => {
    const contract = getContract("L1");
    expect(isHotOrAlternative(contract, "renderer")).toBe(false);
  });
});

describe("healthForStageRole — alternatives group members warm like hard requirements", () => {
  it("treats an alternatives-group member the same as a hotNodes member as progress rises", () => {
    // Both "cloud-bridge" (alternatives-group member for L1) and "presence"
    // (plain hotNodes member) must follow the identical progress curve.
    for (const progress of [0.1, 0.3, 0.6, 0.95]) {
      expect(healthForStageRole("cloud-bridge", "L1", progress)).toBe(
        healthForStageRole("presence", "L1", progress),
      );
    }
  });

  it("a node required at L1 but not yet at an earlier stage warms rather than errors", () => {
    // "renderer" is only required (hard) at L2, not L1. At L0 with decent
    // progress it should read "warming", never an error state.
    const health = healthForStageRole("renderer", "L0", 0.8);
    expect(["cold", "warming"]).toContain(health);
  });
});
