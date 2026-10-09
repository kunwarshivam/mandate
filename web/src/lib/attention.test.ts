import { describe, expect, it } from "vitest";
import type { Agent, OrderState, Scenario, Workspace } from "@/fixtures/types";
import { AGENT_IDS, SCENARIOS, buildWorkspace } from "@/fixtures/workspace";
import { IN_FLIGHT, alertLines, attentionText, needsYouLines, stopAttention } from "./attention";
import { agentLimits, nearLossLimits } from "./limits";
import { agentHref, orderHref } from "./screens";

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
    expect(stopAttention(ws)).toEqual(["Agent 3: selling only"]);
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
    expect(needsYouLines(buildWorkspace("stale")).map((a) => a.text), "Needs you: no feed, and one line per agent and condition").toEqual(["Agent 2: stale price (XYZ, QRS)"]);
    expect(needsYouLines(buildWorkspace("normal"))).toEqual([]);
  });
});

/**
 * A loud Stop says why on Home (critique C-25, rule 13, the Control Rule): an order whose state is
 * unknown turns Stop loud, so Needs you carries it as one condition of its agent, as drawdown and
 * reconciliation do. The expected rows are read straight off the fixture's orders, not through
 * `IN_FLIGHT` or `stopAttention`.
 */
describe("a loud Stop has its reason in Needs you", () => {
  /** Each agent holding an order in state `Unknown`, read off the raw fixture. */
  function unknownOrders(ws: Workspace): Array<{ agentId: string; label: string; ids: string[] }> {
    return ws.agents
      .map((a) => ({ agentId: a.agent_id, label: a.label, ids: a.orders.filter((o) => o.state === "Unknown").map((o) => o.client_order_id) }))
      .filter((a) => a.ids.length > 0);
  }

  it("in the unknown-order scenario: Agent 2's condition, with no instrument, opening the order's record", () => {
    const ws = buildWorkspace("unknown-order");
    expect(stopAttention(ws), "Stop is loud for the order").toEqual(["1 order the broker has not confirmed"]);
    expect(unknownOrders(ws).map((a) => [a.agentId, a.label, a.ids.length])).toEqual([[AGENT_IDS.swing, "Agent 2", 1]]);
    const [held] = unknownOrders(ws);
    expect(needsYouLines(ws).map(({ text, href }) => ({ text, href }))).toEqual([
      { text: "Agent 2: an order's state is unknown", href: orderHref(AGENT_IDS.swing, held.ids[0]) },
    ]);
  });

  it("in every scenario Home shows, a loud Stop has a condition in Needs you, and each unknown order's agent has its line", () => {
    for (const { id } of SCENARIOS) {
      const ws = buildWorkspace(id);
      if (ws.status !== "ready") continue;
      if (stopAttention(ws).length > 0) expect(needsYouLines(ws).length, `${id}: loud with nothing in Needs you`).toBeGreaterThan(0);
      for (const { agentId, label } of unknownOrders(ws)) {
        expect(
          needsYouLines(ws).filter((l) => l.href.startsWith(`/agents/${agentId}`) && l.text.startsWith(`${label}: `) && /unknown/.test(l.text)),
          `${id}: ${label}`,
        ).toHaveLength(1);
      }
    }
  });

  it("for any agent and any of its orders gone unknown, one line for that agent, opening that order", () => {
    for (const agent of buildWorkspace("normal").agents) {
      for (let i = 0; i < agent.orders.length; i++) {
        const ws = buildWorkspace("normal");
        const order = agentIn(ws, agent.agent_id).orders[i];
        order.state = "Unknown";
        expect(stopAttention(ws), `${agent.label} order ${i}`).not.toEqual([]);
        expect(needsYouLines(ws).map((l) => [l.text, l.href]), `${agent.label} order ${i}`).toEqual([
          [`${agent.label}: an order's state is unknown`, orderHref(agent.agent_id, order.client_order_id)],
        ]);
      }
    }
  });

  it("one line for an agent with several unknown orders, opening the agent's orders", () => {
    const ws = buildWorkspace("normal");
    const btc = agentIn(ws, AGENT_IDS.btc);
    expect(btc.orders.length).toBeGreaterThan(1);
    for (const o of btc.orders) o.state = "Unknown";
    expect(needsYouLines(ws).map(({ text, href }) => ({ text, href }))).toEqual([
      { text: `Agent 1: ${btc.orders.length} orders' states are unknown`, href: agentHref(AGENT_IDS.btc, "orders") },
    ]);
  });

  it("after the agent's restrictions, as one more of its conditions", () => {
    const ws = buildWorkspace("unknown-order");
    agentIn(ws, AGENT_IDS.swing).restrictions = [{ code: "startup_reconciliation", since: ws.now }];
    expect(needsYouLines(ws).map((l) => l.text)).toEqual(["Agent 2: checking with the broker", "Agent 2: an order's state is unknown"]);
  });

  it("no such line while every order is in any other state", () => {
    for (const state of Object.keys(IN_FLIGHT) as OrderState[]) {
      if (state === "Unknown") continue;
      const ws = buildWorkspace("normal");
      agentIn(ws, AGENT_IDS.btc).orders[0].state = state;
      expect(needsYouLines(ws), state).toEqual([]);
    }
  });
});
