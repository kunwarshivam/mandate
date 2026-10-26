import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ApiAgent } from "./api-screens";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import { buildWorkspace } from "@/fixtures/workspace";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { RoleProvider } from "@/lib/roles";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";
const AGENT = buildWorkspace("normal").agents[0].agent_id;

/** The agent page over the mock, its agent read's `openings_held` set to `held`, or removed when `held` is `"absent"`. */
function mount(held: unknown) {
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const fetch: Fetch = async (input, init) => {
    const answer = await server.fetch(input, init);
    if ((init.method ?? "GET") !== "GET" || !new URL(input).pathname.endsWith(`/agents/${AGENT}`)) return answer;
    const body = (await answer.json()) as { agent: Record<string, unknown> };
    const agent = { ...body.agent };
    if (held === "absent") delete agent.openings_held;
    else agent.openings_held = held;
    return new Response(JSON.stringify({ ...body, agent }), { status: 200, headers: { "Content-Type": "application/json" } });
  };
  render(
    <RoleProvider>
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        <ApiAgent agentId={AGENT} />
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
}

const note = () => document.querySelector("[data-slot=openings-held]");

afterEach(() => cleanup());

describe("why an agent's openings are held, on its page (DEC-2143)", () => {
  it("says a research agent's openings wait for the multi-instrument risk state, and that exits still run", async () => {
    mount({ reason: "research_agent_traded" });
    await waitFor(() => expect(note()).toHaveTextContent("Openings held: this research agent's account has traded, and a further opening waits for the multi-instrument risk state, which is not built yet. Exits still run."));
    expect(note()?.textContent).not.toMatch(/research_agent_traded|DEC-/);
  });

  it("says a traded account's openings wait for the account's risk to be read", async () => {
    mount({ reason: "account_traded" });
    await waitFor(() => expect(note()).toHaveTextContent("Openings held: this account has traded, and your deployment does not yet read the account's risk for a new opening. Exits still run."));
  });

  it("says openings may be held when the deployment could not judge it, or does not serve it at all, never nothing", async () => {
    for (const held of [{ reason: "unknown" }, "absent"]) {
      mount(held);
      await waitFor(() => expect(note()).toHaveTextContent("Openings may be held; status unavailable. Exits still run."));
      cleanup();
    }
  });

  it("shows nothing when no standing reason applies", async () => {
    mount(null);
    await screen.findByRole("heading", { level: 1 });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(note()).toBeNull();
  });

  it("never reads an unlisted reason as no hold: the agent is not reported", async () => {
    mount({ reason: "stalled" });
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(note()).toBeNull();
    expect(screen.queryByRole("heading", { level: 1, name: buildWorkspace("normal").agents[0].mandate.name })).toBeNull();
    cleanup();
    mount({ reason: "research_agent_traded" });
    expect(await screen.findByRole("heading", { level: 1, name: buildWorkspace("normal").agents[0].mandate.name })).toBeInTheDocument();
  });
});
