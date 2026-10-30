import { describe, expect, it } from "vitest";
import type { RestrictionCode } from "@/fixtures/types";
import { AGENT_IDS, SCENARIOS, buildWorkspace, findAgent } from "@/fixtures/workspace";
import { dec, sub, toDecimalString } from "./decimal";
import { AGENT_ID, APPROVAL_ID } from "./ids";
import { agentLimits, nextLevel } from "./limits";
import { effectiveMode } from "./mock-runtime";
import { RESTRICTIONS, describeRestriction } from "./restrictions";

const ws = buildWorkspace("normal");
const swing = findAgent(ws, AGENT_IDS.swing)!;

describe("limits in dollars (mandate spec §5.2–§5.7)", () => {
  const limits = agentLimits(swing);
  const level = (key: string) => toDecimalString(limits.levels.find((l) => l.key === key)!.at);
  const rail = (key: string) => limits.rails.find((r) => r.key === key)!;

  it("caps a position at the lower of the dollar cap and the equity fraction", () => {
    expect(toDecimalString(rail("position-XYZ").cap)).toBe("1500");
  });

  it("caps gross holdings at the lower of the dollar cap and equity", () => {
    expect(toDecimalString(rail("gross").cap)).toBe("2000");
  });

  it("sets the daily budget from day-start equity and its trigger level below it", () => {
    expect(toDecimalString(rail("daily").cap)).toBe("199");
    expect(level("daily")).toBe("9751");
  });

  it("places ladder rungs below the high-water mark and the floor at capital less the allowed loss", () => {
    expect([level("rung-0"), level("rung-1"), level("rung-2")]).toEqual(["9738.8", "9437.6", "9236.8"]);
    expect(level("floor")).toBe("9000");
  });

  it("treats the profit stop as a level, never as a rail to fill", () => {
    expect(level("profit-stop")).toBe("11000");
    expect(limits.rails.some((r) => r.key.includes("profit"))).toBe(false);
  });

  it("sorts levels from lowest to highest", () => {
    const at = limits.levels.map((l) => l.at);
    expect([...at].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0))).toEqual(at);
  });

  it("names the closest level below equity as the next one, with its distance in dollars", () => {
    const next = nextLevel(limits)!;
    expect(next).toMatchObject({ side: "below", level: { key: "daily" } });
    expect(toDecimalString(next.distance)).toBe(toDecimalString(sub(limits.equity, dec("9751"))));
  });

  it("falls back to the profit stop above once every level below is reached, and skips the high-water mark", () => {
    const reached = { ...limits, levels: limits.levels.map((l) => (l.kind === "profit_stop" ? l : { ...l, reached: true })) };
    const next = nextLevel(reached)!;
    expect(next).toMatchObject({ side: "above", level: { key: "profit-stop" } });
    expect(toDecimalString(next.distance)).toBe(toDecimalString(sub(dec("11000"), limits.equity)));
  });
});

describe("restrictions (brief §4.3)", () => {
  it("every restriction says what it blocks, how it ends, and who acts", () => {
    for (const [code, text] of Object.entries(RESTRICTIONS)) {
      expect(text.label, code).not.toBe("");
      expect(text.blocks, code).not.toBe("");
      expect(text.endsWhen, code).not.toBe("");
      expect(text.whoActs, code).not.toBe("");
    }
  });

  it("names the instrument for instrument restrictions", () => {
    expect(describeRestriction({ code: "stale_mark", since: ws.now, symbol: "QRS" }).title).toContain("QRS");
  });

  it("derives the effective mode as the most severe restriction", () => {
    const at = ws.now;
    const r = (code: RestrictionCode) => ({ code, since: at });
    expect(effectiveMode([])).toBe("normal");
    expect(effectiveMode([r("drawdown_scale_sizes")])).toBe("normal");
    expect(effectiveMode([r("drawdown_exits_only"), r("owner_pause")])).toBe("paused");
    expect(effectiveMode([r("owner_pause"), r("stopped")])).toBe("stopped");
  });
});

describe("fixtures", () => {
  it.each(SCENARIOS.map((s) => s.id))("%s uses opaque ULID route IDs and paper only", (scenario) => {
    const w = buildWorkspace(scenario);
    expect(w.environment).toBe("paper");
    for (const a of w.agents) {
      expect(a.agent_id).toMatch(AGENT_ID);
      expect(a.mandate.environment).toBe("paper");
    }
    for (const a of w.approvals) expect(a.approval_id).toMatch(APPROVAL_ID);
  });

  it("keeps agent labels free of tickers", () => {
    for (const a of ws.agents) expect(a.label).toMatch(/^Agent \d+$/);
  });
});
