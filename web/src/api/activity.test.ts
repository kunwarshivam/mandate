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

/** A position as `read-models/positions` serves it with nothing the journal says beyond its quantity (DEC-2138). */
const POSITION: Record<string, unknown> = { instrument_id: "XYZ", filled_qty: "2", cost_basis: null, risk_mark: null, market_value: null, unrealized_pnl: null, protection: { orders: [], covered_qty: "0", stop: null, take_profit: null, unprotected_since: null } };

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
    expect(holdings({ agent_id: AGENT, account_ref: "a", positions: [{ ...POSITION, instrument_id: "XYZ", filled_qty: "2" }] }, "")).toMatchObject({ ok: true, value: { items: [{ instrument_id: "XYZ", filled_qty: "2" }] } });
  });

  it("requires every DEC-2138 member as the schema does, with protection always served, and refuses a sane mark with no bid", () => {
    const one = (over: Record<string, unknown>) => holdings({ agent_id: AGENT, account_ref: "a", positions: [{ ...POSITION, ...over }], account_positions: null }, "");
    expect(one({}).ok).toBe(true);
    for (const member of ["cost_basis", "risk_mark", "market_value", "unrealized_pnl", "protection"]) {
      const rest = Object.fromEntries(Object.entries(POSITION).filter(([name]) => name !== member));
      expect(holdings({ agent_id: AGENT, account_ref: "a", positions: [rest], account_positions: null }, ""), member).toMatchObject({ ok: false, issue: { path: `/positions/0/${member}`, problem: "missing" } });
    }
    expect(one({ protection: null }).ok).toBe(false);
    expect(one({ risk_mark: { bid: null, sane: true, session: "regular", marked_at: "2026-10-26T14:30:00.000000000Z" } }).ok).toBe(false);
    expect(one({ risk_mark: { bid: null, sane: false, session: "regular", marked_at: "2026-10-26T14:30:00.000000000Z" } }).ok).toBe(true);
    expect(one({ market_value: 1012.5 }).ok).toBe(false);
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

/**
 * The run that failed step 4 (2026-10-27, a363ddeef): the answers the API served the agent's screen
 * while it held its opening, as DR's replay of that run's account stream and the reads of its agent
 * stream gave them. The opening's own order record was adopted with no filled quantity; its mark was
 * sane, and its bracket's stop was placed. Each must decode, so the screen lists the position and
 * the journal rather than calling either not reported.
 */
describe("the agent's screen's answers from run 3's held window", () => {
  const AGENT_3 = "agt_01KDEM00000000000000000WEB";
  const STREAM_3 = `agent:01KDEM6526868180000000000D:${AGENT_3}`;
  const SPY_3 = "b28f4066-5c0d-4c5e-9d8a-1f2e3d4c5b6a";
  const envelope = { api_version: "v1", build: `sha256:${"0".repeat(64)}`, served_at: "2026-10-27T19:17:40.000000000Z", as_of: [] };

  it("decodes the held position with its mark and its placed bracket", () => {
    const served = {
      ...envelope,
      agent_id: AGENT_3,
      account_ref: "01KDEA6526868180000000000A",
      account_positions: [],
      positions: [
        {
          instrument_id: SPY_3,
          filled_qty: "1",
          cost_basis: { qty: "1", basis: "580.11" },
          risk_mark: { bid: "580.11", sane: true, session: "regular", marked_at: "2026-10-27T19:17:21.000000000Z" },
          market_value: "580.11",
          unrealized_pnl: "0",
          protection: { orders: ["md-2T80CW59J6E5JVBDQHW89M4E1W-p60VZ48DMXV4Z41MD9T05S9JWW5"], covered_qty: "1", stop: "551.11", take_profit: "638.13", unprotected_since: null },
        },
      ],
    };
    const read = activityRead(AGENT_3).holdings(served, "");
    expect(read).toMatchObject({ ok: true, value: { items: [{ instrument_id: SPY_3, filled_qty: "1" }] } });
  });

  it("decodes the adopted opening whose filled quantity is not served", () => {
    const served = {
      ...envelope,
      agent_id: AGENT_3,
      account_ref: "01KDEA6526868180000000000A",
      orders: [
        { client_order_id: "md-2T80CW59J6E5JVBDQHW89M4E1W", intent_id: "2T80CW59J6E5JVBDQHW89M4E1W", purpose: "open", instrument_id: SPY_3, side: "buy", type: "limit", tif: "gtc", qty: "1", limit_price: "580.11", stop_price: null, state: "filled", filled_qty: null, submitted_at: "2026-10-27T19:17:24.872727819Z", updated_at: "2026-10-27T19:17:24.872727819Z" },
      ],
    };
    expect(activityRead(AGENT_3).orders(served, "").ok).toBe(true);
  });

  it("decodes the agent stream's nine records, whatever their kinds", () => {
    const kinds = ["StreamOpened", "AgentModeChanged", "ObservationRecorded", "ModelOutputRecorded", "ApprovalRequested", "ApprovalDelivered", "ApprovalResponded", "ApprovalRevalidated", "IntentProposed"];
    const events = kinds.map((event_type, i) => ({ seq: i + 1, event_id: `01KDEM0000000000000000000${i}`, event_type, recorded_at: "2026-10-27T19:17:0" + i + ".000000000Z", prev_hash: "0".repeat(64), hash: "1".repeat(64), body: "e30=" }));
    const head = { seq: 9, hash: "1".repeat(64), recorded_at: "2026-10-27T19:17:24.000000000Z" };
    const decode = journalPageRead(STREAM_3);
    expect(decode({ stream_id: STREAM_3, head, events: events.slice(0, 1), next_after_seq: 1, at_head: false }, "").ok).toBe(true);
    const page = decode({ stream_id: STREAM_3, head, events, next_after_seq: 9, at_head: true }, "");
    expect(page.ok && page.value.events.map((e) => e.event_type)).toEqual(kinds);
  });
});
