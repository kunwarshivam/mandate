import { describe, expect, it } from "vitest";
import { activityRead } from "./activity";
import { createWorkspaceClient } from "./client";
import { createMockServer } from "./mock-server";
import { AGENT_IDS } from "@/fixtures/workspace";

const AGENT = "agt_01JB3K9P2H6SD4F8G1E3W7XYZB";
const order = {
  client_order_id: "cid_1",
  intent_id: null,
  purpose: "open",
  instrument_id: "XYZ",
  side: "buy",
  type: "limit",
  tif: "day",
  qty: "2",
  limit_price: "141.3",
  stop_price: null,
  state: "filled",
  filled_qty: "2",
  submitted_at: "2026-10-26T14:00:00.000000000Z",
  updated_at: "2026-10-26T14:00:05.000000000Z",
};

describe("an agent's activity (read-models/orders and /positions, DEC-2111)", () => {
  const { orders, holdings } = activityRead(AGENT);

  it("reads orders as served, with a state and filled quantity not yet journaled as not reported", () => {
    expect(orders({ agent_id: AGENT, account_ref: "acct_1", orders: [order] }, "")).toMatchObject({ ok: true, value: { account_ref: "acct_1", items: [{ state: "filled", qty: "2" }] } });
    expect(orders({ agent_id: AGENT, account_ref: "acct_1", orders: [{ ...order, state: null, filled_qty: null }] }, "")).toMatchObject({ ok: true, value: { items: [{ state: null, filled_qty: null }] } });
    expect(orders({ agent_id: AGENT, account_ref: "acct_1", orders: [{ ...order, state: "unknown" }] }, "").ok).toBe(true);
  });

  it("refuses an answer for another agent, an unlisted state, and a decimal sent as a number", () => {
    expect(orders({ agent_id: "agt_other", account_ref: null, orders: [] }, "")).toMatchObject({ ok: false, issue: { path: "/agent_id" } });
    expect(orders({ agent_id: AGENT, account_ref: "a", orders: [{ ...order, state: "done" }] }, "").ok).toBe(false);
    expect(orders({ agent_id: AGENT, account_ref: "a", orders: [{ ...order, qty: 2 }] }, "").ok).toBe(false);
  });

  it("keeps a missing account apart from an empty list", () => {
    expect(holdings({ agent_id: AGENT, account_ref: null, positions: [], account_positions: null }, "")).toEqual({ ok: true, value: { agent_id: AGENT, account_ref: null, items: [] } });
    expect(holdings({ agent_id: AGENT, account_ref: "a", positions: [{ instrument_id: "XYZ", filled_qty: "2", cost_basis: null, risk_mark: null, protection: null }] }, "")).toMatchObject({ ok: true, value: { items: [{ instrument_id: "XYZ", filled_qty: "2" }] } });
  });

  it("decodes what the mock serves for every fixture agent", async () => {
    const ws = "ws_01J9ZQ4M0000000000000000AB";
    const server = createMockServer({ workspaceId: ws, scenario: "normal" });
    const client = createWorkspaceClient({ workspaceId: ws, fetch: server.fetch });
    for (const id of Object.values(AGENT_IDS)) {
      const read = activityRead(id);
      expect((await client.read(`/agents/${id}/orders`, read.orders)).ok, id).toBe(true);
      expect((await client.read(`/agents/${id}/positions`, read.holdings)).ok, id).toBe(true);
    }
  });
});
