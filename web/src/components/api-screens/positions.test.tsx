import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import PositionsPage from "@/app/(app)/positions/page";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import type { Scenario } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { RoleProvider } from "@/lib/roles";
import { setPathname } from "@/test/navigation";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";

function mount(scenario: Scenario = "normal", wrap: (send: Fetch) => Fetch = (send) => send) {
  setPathname("/positions");
  const server = createMockServer({ workspaceId: WS, scenario });
  const reads: string[] = [];
  const fetch: Fetch = (input, init) => {
    if ((init.method ?? "GET") === "GET") reads.push(new URL(input).pathname);
    return server.fetch(input, init);
  };
  render(
    <RoleProvider initial="owner">
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={wrap(fetch)} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        <main>
          <PositionsPage />
        </main>
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
  return { reads };
}

afterEach(() => cleanup());

describe("positions over the workspace API (§4.7 D3, DEC-2111)", () => {
  it("shows each agent's own positions from GET /agents/{id}/positions, and the cost basis, risk mark and protection as not reported", async () => {
    const { reads } = mount();
    const fixture = buildWorkspace("normal");
    const holding = fixture.agents.find((a) => a.positions.length > 0)!;
    const table = await screen.findByRole("table", { name: `${holding.mandate.name}'s positions` });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows).toHaveLength(holding.positions.length);
    for (const row of rows) {
      const cells = within(row).getAllByRole("cell").map((c) => c.textContent);
      expect(cells.slice(2)).toEqual(["Not reported", "Not reported", "Not reported"]);
    }
    expect(reads).toContain(`/v1/workspaces/${WS}/agents/${holding.agent_id}/positions`);
    expect(reads.some((r) => r.endsWith("/orders"))).toBe(false);
    expect(screen.getByRole("table", { name: "The account's holdings" })).toBeInTheDocument();
    expect(document.querySelector("[data-slot=fixture-tag]")).toBeNull();
    expect(document.querySelector("[data-slot=api-not-yet]")).toBeNull();
  });

  it("never shows a list that could not be read as empty: it names the agent as not reported", async () => {
    const fixture = buildWorkspace("normal");
    const failing = fixture.agents[0];
    mount("normal", (send) => (input, init) =>
      new URL(input).pathname.endsWith(`/agents/${failing.agent_id}/positions`) ? Promise.reject(new TypeError("down")) : send(input, init),
    );
    await waitFor(() => expect(screen.getByRole("main")).toHaveTextContent(`Positions not reported: ${failing.mandate.name}.`));
    expect(document.querySelector(`[data-slot=agent-positions][data-agent="${failing.agent_id}"]`)).toBeNull();
  });

  it("takes the account's holdings from the same positions reads, not the agent read refreshed on another cadence", async () => {
    mount("normal", (send) => async (input, init) => {
      const answer = await send(input, init);
      if (!/\/agents\/[^/]+$/.test(new URL(input).pathname)) return answer;
      const body = (await answer.json()) as { agent: Record<string, unknown> };
      return new Response(JSON.stringify({ ...body, agent: { ...body.agent, account_positions: null } }), { status: answer.status, headers: answer.headers });
    });
    const table = await screen.findByRole("table", { name: "The account's holdings" });
    expect(within(table).getAllByRole("row").length).toBeGreaterThan(1);
    expect(within(table).getAllByRole("columnheader").every((h) => h.getAttribute("scope") === "col")).toBe(true);
  });

  it("says the deployment reports no agents, never the fixtures' empty board", async () => {
    mount("empty");
    await waitFor(() => expect(screen.getByRole("main")).toHaveTextContent("Your deployment reports no agents."));
    expect(screen.queryByText("No agents yet")).toBeNull();
  });
});
