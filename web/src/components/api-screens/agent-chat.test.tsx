import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ApiAgent } from "./api-screens";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import { INSTRUMENTS } from "@/fixtures/mandates";
import { AGENT_IDS, buildWorkspace } from "@/fixtures/workspace";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { RoleProvider } from "@/lib/roles";

/**
 * The agent page's plain-words box: each phrase becomes one previewed action, sent only on Send,
 * through the same paths as the rest of the app (DEC-2151 for an ask, the owner commands, owner exits).
 */
const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";
function swing() {
  const found = buildWorkspace("normal").agents.find((a) => a.agent_id === AGENT_IDS.swing);
  if (!found) throw new Error("the fixture has the swing agent");
  return found;
}
const SWING = swing();
/** The name the API serves: the mandate document's, not the fixture's label. */
const NAME = SWING.mandate.name;

function mount() {
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const posts: Array<{ path: string; body: Record<string, unknown> }> = [];
  const fetch: Fetch = (input, init) => {
    if ((init.method ?? "GET") === "POST") posts.push({ path: new URL(input).pathname.slice(`/v1/workspaces/${WS}`.length), body: JSON.parse(String(init.body ?? "{}")) as Record<string, unknown> });
    return server.fetch(input, init);
  };
  render(
    <RoleProvider>
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        <ApiAgent agentId={SWING.agent_id} />
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
  return posts;
}

async function say(text: string) {
  const field = await screen.findByLabelText(`Tell ${NAME} what to do`, {}, { timeout: 5_000 });
  fireEvent.change(field, { target: { value: text } });
  fireEvent.keyDown(field, { key: "Enter" });
}

const last = () => [...document.querySelectorAll<HTMLElement>('[data-slot="agent-chat-entry"]')].at(-1);
const proposal = async () => {
  await waitFor(() => expect(last()?.querySelector('[data-slot="agent-chat-proposal"]')).not.toBeNull(), { timeout: 5_000 });
  return last()?.querySelector<HTMLElement>('[data-slot="agent-chat-proposal"]') as HTMLElement;
};

afterEach(() => cleanup());

describe("telling an agent what to do in plain words", () => {
  it("previews an ask to buy and sends nothing until Send, then sends the owner's request", async () => {
    const posts = mount();
    await say("buy 10 XYZ");
    const card = await proposal();
    expect(card.textContent).toContain(`Ask ${NAME} to buy 10 XYZ.`);
    expect(posts.filter((p) => p.path.endsWith("/requests"))).toEqual([]);
    expect(within(card).queryByRole("button", { name: /^buy$/i })).toBeNull();
    fireEvent.click(within(card).getByRole("button", { name: "Send" }));
    await waitFor(() => expect(card.querySelector('[data-slot="agent-chat-outcome"]')?.textContent).toMatch(/^Your agent took the request/), { timeout: 5_000 });
    expect(posts.filter((p) => p.path.endsWith("/requests"))).toEqual([{ path: `/agents/${SWING.agent_id}/requests`, body: { instrument: INSTRUMENTS.XYZ.asset_id, side: "buy", size: "10" } }]);
    expect(within(card).getByRole("link", { name: "Approvals" }).getAttribute("href")).toBe("/approvals");
  });

  it("sends nothing when the owner cancels", async () => {
    const posts = mount();
    await say("pause");
    const card = await proposal();
    fireEvent.click(within(card).getByRole("button", { name: "Cancel" }));
    expect(card.textContent).toContain("Canceled. Nothing was sent.");
    expect(posts.filter((p) => p.path.includes("/agents/"))).toEqual([]);
  });

  it("pauses through the owner command once Send is tapped", async () => {
    const posts = mount();
    await say("pause it");
    const card = await proposal();
    expect(card.textContent).toContain(`Pause ${NAME}: no new orders; resting protection stays.`);
    fireEvent.click(within(card).getByRole("button", { name: "Send" }));
    await waitFor(() => expect(posts.some((p) => p.path === `/agents/${SWING.agent_id}/pause`)).toBe(true), { timeout: 5_000 });
  });

  it("previews selling everything the agent holds, then sends one owner exit per holding", async () => {
    const posts = mount();
    await say("sell everything");
    const card = await proposal();
    expect(card.textContent).toMatch(/^Sell all of .+'s /);
    expect(posts.filter((p) => p.path.endsWith("/exits"))).toEqual([]);
    fireEvent.click(within(card).getByRole("button", { name: "Send" }));
    await waitFor(() => expect(card.querySelector('[data-slot="agent-chat-outcome"]')).not.toBeNull(), { timeout: 5_000 });
    const exits = posts.filter((p) => p.path === `/agents/${SWING.agent_id}/exits`);
    expect(exits.length).toBeGreaterThan(0);
    expect(new Set(exits.map((e) => e.body.instrument)).size).toBe(exits.length);
  });

  it("says a symbol the deployment has not registered is not registered, and offers nothing to send", async () => {
    mount();
    await say("buy 5 NOPE");
    await waitFor(() => expect(last()?.textContent).toContain("NOPE isn't registered on your deployment."), { timeout: 5_000 });
    expect(last()?.querySelector('[data-slot="agent-chat-proposal"]')).toBeNull();
  });

  it("refuses to advise, and lists what it can do for words it does not understand", async () => {
    mount();
    await say("should I buy XYZ?");
    await waitFor(() => expect(last()?.textContent).toContain("I can't advise on what to trade."), { timeout: 5_000 });
    await say("make me rich");
    await waitFor(() => expect(last()?.textContent).toMatch(/I didn't catch that\.Try: “buy 10 AAPL”/), { timeout: 5_000 });
    expect(document.querySelector('[data-slot="agent-chat-proposal"]')).toBeNull();
  });

  it("answers how it is doing from its positions, with P&L said to be not reported, and sends nothing", async () => {
    const posts = mount();
    await say("how am I doing");
    await waitFor(() => expect(last()?.textContent).toContain("P&L is not reported by your deployment yet."), { timeout: 5_000 });
    expect(within(last() as HTMLElement).getByRole("link", { name: "Positions" }).getAttribute("href")).toBe("/positions");
    expect(posts.filter((p) => p.path.includes("/agents/"))).toEqual([]);
  });
});
