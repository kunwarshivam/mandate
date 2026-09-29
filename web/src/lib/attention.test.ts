import { describe, expect, it } from "vitest";
import type { Agent, OrderState, Scenario, Workspace } from "@/fixtures/types";
import { AGENT_IDS, buildWorkspace } from "@/fixtures/workspace";
import { IN_FLIGHT, alertLines, attentionText, stopAttention } from "./attention";
import { agentLimits, nearLossLimits } from "./limits";

function agentIn(ws: Workspace, id: string): Agent {
  const agent = ws.agents.find((a) => a.agent_id === id);
  if (!agent) throw new Error(`no agent ${id}`);
  return agent;
}

/** The calm workspace with one agent's equity moved, the rest of its state as the fixture has it. */
function withEquity(id: string, equity: string, dayStart?: string, high?: string): Workspace {
  const ws = buildWorkspace("normal");
  const agent = agentIn(ws, id);
  agent.state = {
    ...agent.state,
    equity,
    equity_day_start: dayStart ?? agent.state.equity_day_start,
    high_water_mark: high ?? agent.state.high_water_mark,
  };
  return ws;
}

describe("the Stop control stays quiet", () => {
  it.each<Scenario>(["normal", "empty", "loading", "paused", "approvals", "result-unknown"])("in the %s scenario", (scenario) => {
    expect(stopAttention(buildWorkspace(scenario))).toEqual([]);
  });

  it("with requests waiting for approval and nothing else", () => {
    const ws = buildWorkspace("approvals");
    expect(ws.approvals.filter((a) => a.status === "delivered").length).toBe(3);
    expect(stopAttention(ws)).toEqual([]);
  });

  it("with resting orders and protective stops the broker has accepted", () => {
    const ws = buildWorkspace("normal");
    expect(ws.agents.flatMap((a) => a.orders).map((o) => o.state)).toEqual(["Accepted", "Accepted", "Accepted", "Accepted"]);
    expect(stopAttention(ws)).toEqual([]);
  });

  it("when the owner paused or stopped an agent themselves", () => {
    const ws = buildWorkspace("normal");
    const btc = agentIn(ws, AGENT_IDS.btc);
    btc.mode = "stopped";
    btc.restrictions = [{ code: "stopped", since: ws.now }];
    btc.state = { ...btc.state, equity: "9000" };
    const swing = agentIn(ws, AGENT_IDS.swing);
    swing.mode = "paused";
    swing.restrictions = [{ code: "owner_pause", since: ws.now }];
    expect(alertLines(ws).map((a) => a.text)).toEqual(["Agent 1: stopped", "Agent 2: paused by you"]);
    expect(stopAttention(ws)).toEqual([]);
  });
});

describe("the Stop control turns loud", () => {
  it.each<[Scenario, string[]]>([
    ["stale", ["4 alerts"]],
    ["drawdown", ["2 alerts", "Agent 1 has reached its drawdown 6% level"]],
    ["reconciliation", ["1 alert"]],
    ["unknown-order", ["1 order the broker has not confirmed"]],
    ["unreachable", ["the deployment is unreachable", "2 alerts"]],
  ])("in the %s scenario", (scenario, reasons) => {
    expect(stopAttention(buildWorkspace(scenario))).toEqual(reasons);
  });

  it("for an agent in exits only, even without a restriction listed", () => {
    const ws = buildWorkspace("normal");
    agentIn(ws, AGENT_IDS.lmn).mode = "exits_only";
    expect(stopAttention(ws)).toEqual(["Agent 3: exits only"]);
  });

  it("for a restriction from the mandate, the account or market data", () => {
    for (const code of ["drawdown_scale_sizes", "external_activity", "stale_mark"] as const) {
      const ws = buildWorkspace("normal");
      agentIn(ws, AGENT_IDS.swing).restrictions = [{ code, since: ws.now, symbol: "XYZ" }];
      expect(stopAttention(ws), code).toEqual(["1 alert"]);
    }
  });

  it("for each degraded feed", () => {
    for (const feed of ["market_data", "broker", "deployment", "relay"] as const) {
      for (const state of ["stale", "down"] as const) {
        const ws = buildWorkspace("normal");
        ws.health[feed] = { state, as_of: ws.now };
        expect(stopAttention(ws), `${feed} ${state}`).toEqual(["1 alert"]);
      }
    }
  });

  it("for an order the broker has not confirmed, and only then", () => {
    const unconfirmed: OrderState[] = ["Intent", "Submitting", "PendingCancel", "PendingReplace", "Unknown"];
    for (const state of Object.keys(IN_FLIGHT) as OrderState[]) {
      const ws = buildWorkspace("normal");
      agentIn(ws, AGENT_IDS.btc).orders[0].state = state;
      expect(stopAttention(ws), state).toEqual(unconfirmed.includes(state) ? ["1 order the broker has not confirmed"] : []);
    }
  });

  it("builds the description the way the founder's example reads", () => {
    const ws = withEquity(AGENT_IDS.btc, "9880", undefined, "10050");
    ws.health.relay = { state: "down", as_of: ws.now };
    expect(attentionText(stopAttention(ws))).toBe("Needs attention: 1 alert, Agent 1 is near its daily loss limit");
  });
});

/**
 * Near a loss limit: 80% of the limit's allowance used, so the headroom is a fifth of it or less.
 * Each boundary below is worked out by hand from the fixture's mandate, not by the code under test.
 */
describe("near a loss limit", () => {
  it("daily: Agent 1 starts the day at its $10,050.00 high with a 2% budget of $201.00, a limit at $9,849.00, so near at $9,889.20", () => {
    expect(stopAttention(withEquity(AGENT_IDS.btc, "9889.2", undefined, "10050"))).toEqual(["Agent 1 is near its daily loss limit"]);
    expect(stopAttention(withEquity(AGENT_IDS.btc, "9889.21", undefined, "10050"))).toEqual([]);
  });

  it("names the deepest limit it is near when there are two", () => {
    expect(stopAttention(withEquity(AGENT_IDS.btc, "9889.2"))).toEqual(["Agent 1 is near its drawdown 3% level"]);
  });

  it("drawdown: Agent 1's 3% rung is $9,845.50 below a $10,150.00 high, $304.50 deep, so near at $9,906.40", () => {
    expect(stopAttention(withEquity(AGENT_IDS.btc, "9906.4", "9906.4"))).toEqual(["Agent 1 is near its drawdown 3% level"]);
    expect(stopAttention(withEquity(AGENT_IDS.btc, "9906.41", "9906.41"))).toEqual([]);
  });

  it("floor: Agent 3 may lose $500.00 of $5,000.00, a floor at $4,500.00, so near at $4,600.00", () => {
    const near = withEquity(AGENT_IDS.lmn, "4600", "4600");
    expect(nearLossLimits(agentLimits(agentIn(near, AGENT_IDS.lmn))).map((l) => l.key)).toEqual(["floor", "rung-2", "rung-1", "rung-0"]);
    expect(stopAttention(near)).toEqual(["Agent 3 is near its lifetime floor"]);
    const clear = withEquity(AGENT_IDS.lmn, "4600.01", "4600.01");
    expect(nearLossLimits(agentLimits(agentIn(clear, AGENT_IDS.lmn))).map((l) => l.key)).toEqual(["rung-2", "rung-1", "rung-0"]);
  });

  it("never counts the high-water mark or a profit stop", () => {
    const ws = withEquity(AGENT_IDS.swing, "11000", "11000");
    agentIn(ws, AGENT_IDS.swing).state.high_water_mark = "11000";
    const levels = agentLimits(agentIn(ws, AGENT_IDS.swing)).levels;
    expect(levels.find((l) => l.kind === "profit_stop")?.reached).toBe(true);
    expect(nearLossLimits(agentLimits(agentIn(ws, AGENT_IDS.swing)))).toEqual([]);
    expect(stopAttention(ws)).toEqual([]);
  });
});

describe("alert lines", () => {
  it("read as the dashboard's alerts summary always has", () => {
    expect(alertLines(buildWorkspace("stale")).map((a) => a.text)).toEqual([
      "Market data stale",
      "Push relay down",
      "Agent 2: stale price (XYZ)",
      "Agent 2: stale price (QRS)",
    ]);
    expect(alertLines(buildWorkspace("normal"))).toEqual([]);
  });
});
