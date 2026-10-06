// @vitest-environment node
import { describe, expect, it } from "vitest";
import { oracle, versionInvariants } from "@/test/version-invariants";
import { SCENARIOS, buildWorkspace } from "./workspace";

versionInvariants(
  SCENARIOS.flatMap(({ id }) => {
    const ws = buildWorkspace(id);
    return ws.agents.map((a) => [id, a.label, a, ws] as const);
  }),
);

describe("the oracle", () => {
  it("catches a fixture that calls a raised limit reducing", () => {
    expect(oracle({ path: "/risk/max_gross_exposure_usd", from: "1500", to: "2000" })).toBe("risk_increasing");
    expect(oracle({ path: "/autonomy/approval/two_approver_above_usd", from: "400", to: null })).toBe("risk_increasing");
    expect(oracle({ path: "/capital/allocation_usd", from: "10000", to: null })).toBe("risk_increasing");
    expect(oracle({ path: "/autonomy/approval/timeout_s", from: 600, to: 900 })).toBe("risk_increasing");
    expect(oracle({ path: "/autonomy/approval/timeout_s", from: 600, to: 300 })).toBe("risk_increasing");
    expect(oracle({ path: "/protection/stop_distance", from: "0.05", to: "0.06" })).toBe("risk_increasing");
    expect(() => oracle({ path: "/behavior/description", from: "a", to: "b" })).toThrow(/no §9.2 row/);
  });
});
