import { describe, expect, it } from "vitest";
import { type Fetch, createWorkspaceClient } from "./client";
import { apiChat, cardOf, sendRoute, summaryLines, threadRoute } from "./chat";
import { createMockServer } from "./mock-server";
import { AGENT_IDS } from "@/fixtures/workspace";
import { FIXTURE_ANSWER } from "@/lib/fixture-chat";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";

function chatOver() {
  const server = createMockServer({ workspaceId: WS, scenario: "approvals" });
  const calls: Array<{ method: string; path: string; headers: Record<string, string> }> = [];
  const fetch: Fetch = (input, init) => {
    const url = new URL(input);
    calls.push({ method: init.method ?? "GET", path: url.pathname.slice(`/v1/workspaces/${WS}`.length) + url.search, headers: { ...(init.headers as Record<string, string>) } });
    return server.fetch(input, init);
  };
  const client = createWorkspaceClient({ workspaceId: WS, fetch, baseUrl: ORIGIN });
  return { chat: apiChat(client, fetch, ORIGIN), calls };
}

describe("the chat's cards are the server's typed data (DEC-2293)", () => {
  it("reads each kind it knows, and any other kind, or one missing what it shows, as unknown", () => {
    expect(cardOf(null)).toBeNull();
    expect(cardOf({ kind: "fill", side: "buy", symbol: "AAPL", qty: "10", price: "190.12" })).toEqual({ kind: "fill", side: "buy", symbol: "AAPL", qty: "10", price: "190.12" });
    expect(cardOf({ kind: "approval", approval_id: "01JB", symbol: "AAPL", order: { side: "buy", instrument: "b0b6", qty: "5", limit: "191" } })).toEqual({
      kind: "approval",
      approval_id: "01JB",
      side: "buy",
      symbol: "AAPL",
      qty: "5",
      limit: "191",
    });
    expect(cardOf({ kind: "approval", order: {} })).toEqual({ kind: "unknown" });
    expect(cardOf({ kind: "proposal", side: "buy", symbol: "NVDA", qty: null, reason: "Momentum" })).toEqual({ kind: "proposal", side: "buy", symbol: "NVDA", qty: null, reason: "Momentum" });
    expect(cardOf({ kind: "brief", date: "2026-10-29" })).toEqual({ kind: "unknown" });
    expect(cardOf("a model's words")).toEqual({ kind: "unknown" });
  });

  it("shows a draft's envelope in plain words, from the server's summary only", () => {
    const card = cardOf({
      kind: "confirm_mandate",
      version: "sha256:ab",
      summary: { name: "Steady SPY", mandate: { allocation_usd: "1000", max_daily_loss: "0.02", max_drawdown: "0.08", max_orders_per_day: 50, asset_classes: ["us_equity"], goal: "continuous" } },
    });
    expect(card).toMatchObject({ kind: "confirm_mandate", version: "sha256:ab", name: "Steady SPY" });
    expect(summaryLines({ allocation_usd: "1000", max_daily_loss: "0.02", max_drawdown: "0.08", max_orders_per_day: 50, asset_classes: ["us_equity"], goal: "continuous", unknown_member: "x" })).toEqual([
      { label: "Capital", value: "$1,000.00" },
      { label: "Goal", value: "continuous" },
      { label: "Orders a day, at most", value: "50" },
      { label: "Daily loss limit", value: "2%" },
      { label: "Drawdown limit", value: "8%" },
      { label: "Trades", value: "us equity" },
    ]);
  });
});

describe("the chat's routes (§4.6.2)", () => {
  it("names an agent's thread, a new setup, and a setup already started", () => {
    expect(threadRoute({ agent: "agt_1" })).toBe("/agents/agt_1/messages");
    expect(sendRoute({ setup: null })).toBe("/setup/messages");
    expect(threadRoute({ setup: "setup_1" })).toBe("/setup/setup_1/messages");
    expect(sendRoute({ setup: "setup_1" })).toBe("/setup/messages?setup_id=setup_1");
  });

  it("sends a message as a browser command, with the CSRF header and a key, and reads it back from the thread", async () => {
    const { chat, calls } = chatOver();
    const sent = await chat.send({ agent: AGENT_IDS.swing }, "how am I doing?");
    expect(sent.ok).toBe(true);
    const post = calls.find((c) => c.method === "POST")!;
    expect(post.path).toBe(`/agents/${AGENT_IDS.swing}/messages`);
    expect(post.headers["X-Mandate-Request"]).toBe("1");
    expect(post.headers["Idempotency-Key"]).toMatch(/^[A-Za-z0-9_-]{32}$/);
    const thread = await chat.thread({ agent: AGENT_IDS.swing }, 0);
    expect(thread.ok && thread.value.messages.slice(-2).map((m) => [m.from, m.text])).toEqual([
      ["owner", "how am I doing?"],
      ["agent", FIXTURE_ANSWER],
    ]);
  });

  it("starts a setup on its first message and keeps to it after", async () => {
    const { chat, calls } = chatOver();
    const first = await chat.send({ setup: null }, "trade SPY with $1,000");
    expect(first.ok).toBe(true);
    const setup = first.ok ? first.value.setup_id : null;
    expect(setup).toMatch(/^setup_/);
    await chat.send({ setup }, "careful");
    expect(calls.filter((c) => c.method === "POST").map((c) => c.path)).toEqual(["/setup/messages", `/setup/messages?setup_id=${setup}`]);
    const thread = await chat.thread({ setup }, 0);
    expect(thread.ok && thread.value.messages.filter((m) => m.from === "owner").map((m) => m.text)).toEqual(["trade SPY with $1,000", "careful"]);
  });

  it("refuses a cap that is not a positive amount, with nothing recorded", async () => {
    const { chat } = chatOver();
    expect(await chat.saveSettings(AGENT_IDS.swing, { chat_model: "llm.chat_haiku", daily_cap_usd: "0" })).toEqual({ ok: false, reason: "invalid", effect: "none" });
    const saved = await chat.saveSettings(AGENT_IDS.swing, { chat_model: "llm.chat_haiku", daily_cap_usd: "2.50" });
    expect(saved).toEqual({ ok: true, value: { chat_model: "llm.chat_haiku", daily_cap_usd: "2.50" } });
    expect(await chat.settings(AGENT_IDS.swing)).toEqual({ ok: true, value: { chat_model: "llm.chat_haiku", daily_cap_usd: "2.50" } });
  });

  it("calls a lost answer to a send unknown, never sent or not", async () => {
    const client = createWorkspaceClient({ workspaceId: WS, fetch: () => Promise.reject(new TypeError("lost")), baseUrl: ORIGIN });
    const chat = apiChat(client, () => Promise.reject(new TypeError("lost")), ORIGIN);
    expect(await chat.send({ agent: AGENT_IDS.swing }, "hi")).toEqual({ ok: false, reason: "network", effect: "unknown" });
  });
});
