import { describe, expect, it } from "vitest";
import { AGENT_IDS, APPROVAL_IDS, SCENARIOS, buildWorkspace } from "@/fixtures/workspace";
import { canOpen } from "./access";
import { ROLES } from "./roles";
import { WIRE_MAX, wireItems } from "./wire";

const lines = (scenario: Parameters<typeof buildWorkspace>[0], now?: string) => {
  const ws = buildWorkspace(scenario);
  return wireItems(ws, now ?? ws.now, "owner").map((i) => `${i.agent} ${i.phrase} · ${i.state}`);
};

describe("the agent wire", () => {
  it("puts requests waiting for you first, then today's decisions, newest first, in the journal's own words", () => {
    expect(lines("normal")).toEqual([
      "Agent 2 asked you to buy 2 XYZ at $141.30 · waiting_for_you",
      "Agent 1 blocked: orders are at most $1,000.00 · blocked",
      "Agent 1 placed an order to buy 0.01 BTC/USD at $55,900.00 · done",
      "Agent 3 blocked: no re-entry within 1 h of an exit in the same instrument · blocked",
      "Agent 2 bought 5 QRS at $98.76 · done",
      "Agent 2 blocked: opening orders only in the regular session · blocked",
      "Agent 2 holds sell 2 QRS at $97.90: waiting for the regular session · waiting",
    ]);
  });

  it("keeps a request first even when a decision is newer", () => {
    expect(lines("unknown-order").slice(0, 2)).toEqual([
      "Agent 2 asked you to buy 2 XYZ at $141.30 · waiting_for_you",
      "Agent 2 holds sell 5 QRS at $97.60: an order in this instrument has an unknown state; nothing else is sent in it until the broker answers · held",
    ]);
  });

  it("links a request to its page and a decision to the page recent activity opens", () => {
    const ws = buildWorkspace("normal");
    const [request, decision] = wireItems(ws, ws.now, "owner");
    expect(request.href).toBe(`/approvals/${APPROVAL_IDS.swingXyz}`);
    expect(decision.href).toBe(`/agents/${AGENT_IDS.btc}/decisions/01JBWPQ5E6EYCNDY0YP57RCYBV`);
  });

  it("names the outcome of a request no longer waiting, and drops one past its deadline from the top", () => {
    expect(lines("reconciliation")[0]).toBe("Agent 2 asked you to buy 2 XYZ at $141.30: canceled · done");
    const ws = buildWorkspace("normal");
    const later = "2026-09-28T14:30:00-04:00";
    expect(wireItems(ws, later, "owner")[0]).toMatchObject({ phrase: "asked you to buy 2 XYZ at $141.30: skipped at the deadline", state: "done" });
  });

  it("says when you paused or stopped an agent, in ink words", () => {
    expect(lines("paused")[0]).toBe("Agent 2 paused by you · paused");
    const ws = buildWorkspace("normal");
    const btc = ws.agents.find((a) => a.agent_id === AGENT_IDS.btc)!;
    btc.mode = "stopped";
    btc.restrictions = [{ code: "stopped", since: "2026-09-28T14:05:00-04:00" }];
    expect(wireItems(ws, ws.now, "owner").find((i) => i.state === "stopped")).toMatchObject({ agent: "Agent 1", phrase: "stopped by you", href: `/agents/${AGENT_IDS.btc}` });
  });

  it("carries only today's activity", () => {
    const ws = buildWorkspace("normal");
    ws.approvals = [];
    ws.decisions = ws.decisions.map((d) => ({ ...d, at: d.at.replace("2026-09-28", "2026-09-27") }));
    expect(wireItems(ws, ws.now, "owner")).toEqual([]);
  });

  it(`carries at most ${WIRE_MAX} items`, () => {
    const ws = buildWorkspace("normal");
    ws.decisions = Array.from({ length: 20 }, (_, i) => ({ ...ws.decisions[1], event_id: `evt_${i}` }));
    expect(wireItems(ws, ws.now, "owner")).toHaveLength(WIRE_MAX);
  });

  it.each(SCENARIOS.map((s) => s.id))("in %s, never states an amount won or lost", (scenario) => {
    for (const line of lines(scenario)) expect(line).not.toMatch(/[+−-]\$|P&L|profit|\bgain|\bloss|unrealized|realized/i);
  });

  it.each(ROLES.map((r) => r.id))("as %s, links only where the role may go", (role) => {
    const ws = buildWorkspace("approvals");
    for (const item of wireItems(ws, ws.now, role)) expect(canOpen(role, item.href), item.href).toBe(true);
  });
});
