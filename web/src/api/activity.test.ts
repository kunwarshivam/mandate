import { describe, expect, it } from "vitest";
import { activityRead, journalPageRead } from "./activity";
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

describe("a journal page, without its bodies (§4.8.1, API-15)", () => {
  const STREAM = `agent:ws_1:${AGENT}`;
  const line = (seq: number) => ({ seq, event_id: "01JBR2C4D6E8F0G2H4J6K8M0N2", event_type: "OrderSubmitted", recorded_at: "2026-10-26T14:00:00.000000000Z", prev_hash: "0".repeat(64), hash: "1".repeat(64), body: "e30=" });
  const page = (patch: Record<string, unknown> = {}) => ({ stream_id: STREAM, head: { seq: 3, hash: "1".repeat(64), recorded_at: "2026-10-26T14:00:00.000000000Z" }, events: [line(2), line(3)], next_after_seq: 3, at_head: true, ...patch });
  const decode = journalPageRead(STREAM);

  it("reads each event's sequence, type and time, and keeps no body", () => {
    const read = decode(page(), "");
    expect(read).toMatchObject({ ok: true, value: { head_seq: 3, events: [{ seq: 2 }, { seq: 3 }] } });
    if (read.ok) expect(JSON.stringify(read.value)).not.toContain("e30=");
  });

  it("refuses a page for another stream, out of order, or past its head", () => {
    expect(decode(page({ stream_id: `agent:ws_1:agt_other` }), "").ok).toBe(false);
    expect(decode(page({ events: [line(3), line(2)] }), "").ok).toBe(false);
    expect(decode(page({ events: [line(2), line(2)] }), "").ok).toBe(false);
    expect(decode(page({ events: [line(4)] }), "").ok).toBe(false);
    expect(decode(page({ head: undefined }), "").ok).toBe(false);
  });

  it("reads the mock's agent stream through the documented route and query", async () => {
    const ws = "ws_01J9ZQ4M0000000000000000AB";
    const server = createMockServer({ workspaceId: ws, scenario: "normal" });
    const client = createWorkspaceClient({ workspaceId: ws, fetch: server.fetch });
    const stream = `agent:${ws}:${AGENT_IDS.swing}`;
    const read = await client.read(`/journal/streams/${encodeURIComponent(stream)}/events?after_seq=0&limit=2`, journalPageRead(stream));
    expect(read).toMatchObject({ ok: true, value: { events: [{ seq: 1, event_type: "AgentDeployed" }, { seq: 2 }] } });
  });
});
