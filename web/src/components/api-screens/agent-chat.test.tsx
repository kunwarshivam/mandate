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
 * The agent page's chat: the deployment's model answers, and its one typed card is shown as a line
 * this page writes, sent only on Send through the app's own paths (DEC-2151, owner commands, exits).
 */
const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";
function swing() {
  const found = buildWorkspace("normal").agents.find((a) => a.agent_id === AGENT_IDS.swing);
  if (!found) throw new Error("the fixture has the swing agent");
  return found;
}
const SWING = swing();
/** The name the API serves: the mandate document's. */
const NAME = SWING.mandate.name;

type Answer = { status: number; body: unknown };

function mount(answer: Answer) {
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const posts: Array<{ path: string; body: Record<string, unknown> }> = [];
  const fetch: Fetch = (input, init) => {
    const path = new URL(input).pathname.slice(`/v1/workspaces/${WS}`.length);
    if ((init.method ?? "GET") === "POST") posts.push({ path, body: JSON.parse(String(init.body ?? "{}")) as Record<string, unknown> });
    if (path === `/agents/${SWING.agent_id}/chat`)
      return Promise.resolve(new Response(JSON.stringify(answer.body), { status: answer.status, headers: { "Content-Type": answer.status === 200 ? "application/json" : "application/problem+json" } }));
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

const reply = (text: string, card: unknown = null): Answer => ({ status: 200, body: { api_version: "v1", as_of: [], reply: { text, author: "agent" }, card } });

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
  it("sends the owner's words to the chat route, shows the model's reply, and an ask card it writes itself, sent only on Send", async () => {
    const posts = mount(reply("I'll ask it to buy 10 XYZ.", { action: "ask_to_buy", summary: { instrument_id: INSTRUMENTS.XYZ.asset_id, symbol: "XYZ", size: "10" } }));
    await say("buy 10 xyz please");
    const card = await proposal();
    expect(posts.find((p) => p.path.endsWith("/chat"))?.body).toEqual({ message: "buy 10 xyz please" });
    expect(last()?.querySelector('[data-slot="agent-chat-reply"]')?.textContent).toBe("I'll ask it to buy 10 XYZ.");
    expect(card.textContent).toContain(`Ask ${NAME} to buy 10 XYZ.`);
    expect(posts.filter((p) => p.path.endsWith("/requests"))).toEqual([]);
    fireEvent.click(within(card).getByRole("button", { name: "Send" }));
    await waitFor(() => expect(card.querySelector('[data-slot="agent-chat-outcome"]')?.textContent).toMatch(/^Your agent took the request/), { timeout: 5_000 });
    expect(posts.filter((p) => p.path.endsWith("/requests"))).toEqual([{ path: `/agents/${SWING.agent_id}/requests`, body: { instrument: INSTRUMENTS.XYZ.asset_id, side: "buy", size: "10" } }]);
  });

  it("sends nothing when the owner cancels", async () => {
    const posts = mount(reply("Pausing it.", { action: "pause", summary: { agent_id: SWING.agent_id } }));
    await say("pause");
    const card = await proposal();
    fireEvent.click(within(card).getByRole("button", { name: "Cancel" }));
    expect(card.textContent).toContain("Canceled. Nothing was sent.");
    expect(posts.filter((p) => p.path.endsWith("/pause"))).toEqual([]);
  });

  it("pauses through the owner command, with the app's own wording, once Send is tapped", async () => {
    const posts = mount(reply("Pausing it.", { action: "pause", summary: { agent_id: SWING.agent_id } }));
    await say("hold on, pause");
    const card = await proposal();
    expect(card.textContent).toContain(`Pause ${NAME}: no new orders; resting protection stays.`);
    fireEvent.click(within(card).getByRole("button", { name: "Send" }));
    await waitFor(() => expect(posts.some((p) => p.path === `/agents/${SWING.agent_id}/pause`)).toBe(true), { timeout: 5_000 });
  });

  it("sends one owner exit per instrument the card names", async () => {
    const posts = mount(reply("Selling it.", { action: "owner_exit", summary: { instruments: [{ instrument_id: INSTRUMENTS.XYZ.asset_id, symbol: "XYZ", qty: "8" }] } }));
    await say("sell everything");
    const card = await proposal();
    expect(card.textContent).toContain(`Sell all of ${NAME}'s 8 XYZ`);
    fireEvent.click(within(card).getByRole("button", { name: "Send" }));
    await waitFor(() => expect(card.querySelector('[data-slot="agent-chat-outcome"]')).not.toBeNull(), { timeout: 5_000 });
    expect(posts.filter((p) => p.path === `/agents/${SWING.agent_id}/exits`).map((p) => p.body.instrument)).toEqual([INSTRUMENTS.XYZ.asset_id]);
  });

  it("shows the reply alone, with nothing to send, for an answer or a card it cannot read", async () => {
    const posts = mount(reply("It holds 8 XYZ.", { action: "place_order", summary: {} }));
    await say("how am I doing");
    await waitFor(() => expect(last()?.querySelector('[data-slot="agent-chat-reply"]')?.textContent).toBe("It holds 8 XYZ."), { timeout: 5_000 });
    expect(last()?.querySelector('[data-slot="agent-chat-proposal"]')).toBeNull();
    expect(posts.filter((p) => !p.path.endsWith("/chat"))).toEqual([]);
  });

  it("says nothing was done when the deployment does not answer", async () => {
    mount({ status: 503, body: { type: "about:blank", status: 503, title: "x", code: "gateway_unavailable", effect: "none", event_id: null, retryable: true } });
    await say("buy 10 XYZ");
    await waitFor(() => expect(last()?.textContent).toContain("Nothing was done."), { timeout: 5_000 });
    expect(last()?.querySelector('[data-slot="agent-chat-proposal"]')).toBeNull();
  });
});
