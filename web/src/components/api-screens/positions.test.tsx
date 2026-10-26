import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import PositionsPage from "@/app/(app)/positions/page";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import type { Scenario } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { type Role, RoleProvider } from "@/lib/roles";
import { setPathname } from "@/test/navigation";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";

function mount(scenario: Scenario = "normal", wrap: (send: Fetch) => Fetch = (send) => send, role: Role = "owner") {
  setPathname("/positions");
  const server = createMockServer({ workspaceId: WS, scenario });
  const reads: string[] = [];
  const posts: Array<{ path: string; body: Record<string, unknown> }> = [];
  const fetch: Fetch = (input, init) => {
    if ((init.method ?? "GET") === "GET") reads.push(new URL(input).pathname);
    else posts.push({ path: new URL(input).pathname, body: JSON.parse(String(init.body)) as Record<string, unknown> });
    return server.fetch(input, init);
  };
  render(
    <RoleProvider initial={role}>
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={wrap(fetch)} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        <main>
          <PositionsPage />
        </main>
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
  return { reads, posts };
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
      expect(cells.slice(2, 7)).toEqual(["Not reported", "Not reported", "Not reported", "Not reported", "Not reported"]);
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

describe("each position's figures as DEC-2138 serves them", () => {
  const fixture = buildWorkspace("normal");
  const holder = fixture.agents.find((a) => a.positions.length > 0)!;

  function serving(members: Record<string, unknown>) {
    mount("normal", (send) => async (input, init) => {
      const answer = await send(input, init);
      if (!new URL(input).pathname.endsWith(`/agents/${holder.agent_id}/positions`)) return answer;
      const body = (await answer.json()) as { positions: Array<Record<string, unknown>> };
      return new Response(JSON.stringify({ ...body, positions: body.positions.map((p) => ({ ...p, ...members })) }), { status: 200, headers: { "Content-Type": "application/json" } });
    });
  }
  const cells = async () => {
    const table = await screen.findByRole("table", { name: `${holder.mandate.name}'s positions` });
    return within(within(table).getAllByRole("row")[1]).getAllByRole("cell").map((c) => c.textContent);
  };

  it("shows the cost basis, a sane mark's bid, the market value, the unrealized P&L and the protection as served", async () => {
    serving({
      cost_basis: { qty: "10", basis: "1000.5" },
      risk_mark: { bid: "101.25", sane: true, session: "regular", marked_at: "2026-10-26T14:30:00.000000000Z" },
      market_value: "1012.5",
      unrealized_pnl: "12",
      protection: { orders: ["co_1"], covered_qty: "10", stop: "95", take_profit: "110", unprotected_since: null },
    });
    await waitFor(async () => expect((await cells()).slice(2, 7)).toEqual(["$1,000.50 for 10", "$101.25 bid, 14:30:00 UTC", "$1,012.50", "+$12.00", "stop $95.00, take-profit $110.00; covers 10"]));
  });

  it("shows no price for a mark that is not sane, and the open unprotected interval", async () => {
    serving({ cost_basis: null, risk_mark: { bid: "0.01", sane: false, session: "regular", marked_at: "2026-10-26T14:30:00.000000000Z" }, market_value: null, unrealized_pnl: null, protection: { orders: [], covered_qty: "0", stop: null, take_profit: null, unprotected_since: "2026-10-26T14:31:00.000000000Z" } });
    await waitFor(async () => expect((await cells()).slice(2, 7)).toEqual(["Not reported", "No sane mark", "Not reported", "Not reported", "Unprotected since 14:31:00 UTC"]));
    expect(screen.getByRole("main")).not.toHaveTextContent("0.01");
  });

  it("reads a position whose figures the deployment does not serve as not reported, and refuses a sane mark with no bid", async () => {
    serving({ risk_mark: { bid: null, sane: true, session: "regular", marked_at: "2026-10-26T14:30:00.000000000Z" } });
    await waitFor(() => expect(screen.getByRole("main")).toHaveTextContent(`Positions not reported: ${holder.mandate.name}.`));
  });
});

describe("the owner's exit from the positions screen (spec §5.4, rule 13)", () => {
  const fixture = buildWorkspace("normal");
  const holder = fixture.agents.find((a) => a.positions.length > 0)!;
  const held = holder.positions[0];
  const exitsRoute = `/v1/workspaces/${WS}/agents/${holder.agent_id}/exits`;

  async function openExit() {
    const table = await screen.findByRole("table", { name: `${holder.mandate.name}'s positions` });
    fireEvent.click(within(table).getByRole("button", { name: `Exit ${held.instrument.asset_id}` }));
    return screen.getByRole("group", { name: `Exit ${held.instrument.asset_id}` });
  }

  it("asks first, saying what sells and that an equity outside the regular session waits for it, and sends nothing until confirmed", async () => {
    const { posts } = mount();
    const confirm = await openExit();
    expect(confirm).toHaveTextContent(`Sell ${holder.mandate.name}'s whole position in ${held.instrument.asset_id}`);
    expect(confirm).toHaveTextContent("Outside the regular session, an equity sell waits for it: this screen has no bid you could confirm.");
    expect(confirm).toHaveTextContent("Your exit is never denied for the agent's limits or conduct controls; only participation caps pace it.");
    expect(posts).toEqual([]);
    fireEvent.click(within(confirm).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("group", { name: `Exit ${held.instrument.asset_id}` })).toBeNull();
    expect(posts).toEqual([]);
  });

  it("sends one owner exit for the asset, with the screen it showed, no bid and no step-up, then follows it to applied", async () => {
    const { posts } = mount();
    const confirm = await openExit();
    const shown = within(confirm).getAllByRole("listitem").map((li) => li.textContent);
    fireEvent.click(within(confirm).getByRole("button", { name: "Sell it" }));
    await waitFor(() => expect(screen.getByRole("status", { name: `Your exit from ${held.instrument.asset_id}` })).toHaveTextContent("Taken by the agent's runtime. Look at the agent's orders for its sell."));
    const sent = posts.filter((p) => p.path === exitsRoute);
    expect(sent).toHaveLength(1);
    expect(Object.keys(sent[0].body).sort()).toEqual(["bid", "bid_size", "floor", "instrument", "quoted_at", "record", "step_up"]);
    expect(sent[0].body).toMatchObject({ instrument: held.instrument.asset_id, bid: null, bid_size: null, quoted_at: null, floor: null, step_up: null });
    expect(shown.length).toBeGreaterThan(2);
    expect(posts.filter((p) => p.path.endsWith("/kill-switch"))).toEqual([]);
  });

  it("sends it once however often Sell it is pressed", async () => {
    const { posts } = mount();
    const confirm = await openExit();
    const sell = within(confirm).getByRole("button", { name: "Sell it" });
    act(() => {
      sell.click();
      sell.click();
    });
    await waitFor(() => expect(screen.getByRole("status", { name: `Your exit from ${held.instrument.asset_id}` })).toHaveTextContent("Taken by the agent's runtime"));
    expect(posts.filter((p) => p.path === exitsRoute)).toHaveLength(1);
  });

  it("says nothing was sold when the deployment refused it, never by its code", async () => {
    mount("normal", (send) => (input, init) =>
      init.method === "POST" && new URL(input).pathname === exitsRoute
        ? Promise.resolve(new Response(JSON.stringify({ type: "about:blank", status: 403, title: "Forbidden", code: "forbidden", effect: "none", event_id: null, retryable: false }), { status: 403, headers: { "Content-Type": "application/problem+json" } }))
        : send(input, init),
    );
    fireEvent.click(within(await openExit()).getByRole("button", { name: "Sell it" }));
    const status = await screen.findByRole("status", { name: `Your exit from ${held.instrument.asset_id}` });
    await waitFor(() => expect(status).toHaveTextContent("Not recorded: your deployment refused your exit. Nothing was sold."));
    expect(status).not.toHaveTextContent("forbidden");
  });

  it("says the result is unknown when the answer is lost, and to look at the orders before trying again", async () => {
    mount("normal", (send) => (input, init) => (init.method === "POST" && new URL(input).pathname === exitsRoute ? Promise.reject(new TypeError("lost")) : send(input, init)));
    fireEvent.click(within(await openExit()).getByRole("button", { name: "Sell it" }));
    const status = await screen.findByRole("status", { name: `Your exit from ${held.instrument.asset_id}` });
    await waitFor(() => expect(status).toHaveTextContent("The result is unknown; we are checking. Look at the agent's orders before you exit again."), { timeout: 3000 });
    await act(async () => {});
  });

  it("offers no exit for a position that names no asset id, saying why", async () => {
    mount("normal", (send) => async (input, init) => {
      const answer = await send(input, init);
      if (!new URL(input).pathname.endsWith(`/agents/${holder.agent_id}/positions`)) return answer;
      const body = (await answer.json()) as { positions: Array<Record<string, unknown>> };
      return new Response(JSON.stringify({ ...body, positions: body.positions.map((p) => ({ ...p, instrument_id: "XYZ" })) }), { status: 200, headers: { "Content-Type": "application/json" } });
    });
    const table = await screen.findByRole("table", { name: `${holder.mandate.name}'s positions` });
    expect(within(table).queryByRole("button", { name: /^Exit / })).toBeNull();
    expect(table).toHaveTextContent("No exit here: this position names no asset id. The agent's kill switch still sells it.");
  });

  it("offers no exit to a role the spec does not give it, and offers it to the operator", async () => {
    for (const role of ["viewer", "approver"] as const) {
      mount("normal", (send) => send, role);
      const table = await screen.findByRole("table", { name: `${holder.mandate.name}'s positions` });
      expect(within(table).queryByRole("button", { name: /^Exit / })).toBeNull();
      cleanup();
    }
    mount("normal", (send) => send, "operator");
    const table = await screen.findByRole("table", { name: `${holder.mandate.name}'s positions` });
    expect(within(table).getByRole("button", { name: `Exit ${held.instrument.asset_id}` })).toBeInTheDocument();
  });

  it("keeps the exit's outcome once the position sells to nothing and its row is gone", async () => {
    let sold = false;
    mount("normal", (send) => async (input, init) => {
      const path = new URL(input).pathname;
      if (init.method === "POST" && path === exitsRoute) sold = true;
      const answer = await send(input, init);
      if (!sold || !path.endsWith(`/agents/${holder.agent_id}/positions`)) return answer;
      const body = (await answer.json()) as Record<string, unknown>;
      return new Response(JSON.stringify({ ...body, positions: [] }), { status: 200, headers: { "Content-Type": "application/json" } });
    });
    fireEvent.click(within(await openExit()).getByRole("button", { name: "Sell it" }));
    await waitFor(() => expect(screen.getByRole("status", { name: `Your exit from ${held.instrument.asset_id}` })).toHaveTextContent("Taken by the agent's runtime"));
    await waitFor(() => expect(document.querySelector(`[data-slot=agent-positions][data-agent="${holder.agent_id}"]`)).toHaveTextContent("Its own fills net to nothing."), { timeout: 8000 });
    expect(screen.getByRole("status", { name: `Your exit from ${held.instrument.asset_id}` })).toHaveTextContent("Taken by the agent's runtime");
  }, 15_000);

  it("never says nothing was sold when an earlier attempt's answer was lost, even if a later one is refused", async () => {
    let attempts = 0;
    mount("normal", (send) => (input, init) => {
      if (init.method !== "POST" || new URL(input).pathname !== exitsRoute) return send(input, init);
      attempts += 1;
      return attempts === 1
        ? Promise.reject(new TypeError("lost"))
        : Promise.resolve(new Response(JSON.stringify({ type: "about:blank", status: 403, title: "Forbidden", code: "forbidden", effect: "none", event_id: null, retryable: false }), { status: 403, headers: { "Content-Type": "application/problem+json" } }));
    });
    fireEvent.click(within(await openExit()).getByRole("button", { name: "Sell it" }));
    const status = await screen.findByRole("status", { name: `Your exit from ${held.instrument.asset_id}` });
    await waitFor(() => expect(status).toHaveTextContent("The result is unknown; we are checking."), { timeout: 3000 });
    expect(status).not.toHaveTextContent("Nothing was sold");
    expect(attempts).toBe(2);
    expect(screen.queryByRole("button", { name: `Exit ${held.instrument.asset_id}` })).toBeNull();
  });

  it("lets the owner ask again, as a new exit, only once nothing was recorded", async () => {
    let refused = true;
    const keys: string[] = [];
    const { posts } = mount("normal", (send) => (input, init) =>
      init.method === "POST" && new URL(input).pathname === exitsRoute && (keys.push(new Headers(init.headers).get("Idempotency-Key") ?? ""), refused)
        ? ((refused = false), Promise.resolve(new Response(JSON.stringify({ type: "about:blank", status: 403, title: "Forbidden", code: "forbidden", effect: "none", event_id: null, retryable: false }), { status: 403, headers: { "Content-Type": "application/problem+json" } })))
        : send(input, init),
    );
    fireEvent.click(within(await openExit()).getByRole("button", { name: "Sell it" }));
    await waitFor(() => expect(screen.getByRole("status", { name: `Your exit from ${held.instrument.asset_id}` })).toHaveTextContent("Nothing was sold."));
    const table = screen.getByRole("table", { name: `${holder.mandate.name}'s positions` });
    fireEvent.click(within(table).getByRole("button", { name: `Exit ${held.instrument.asset_id}` }));
    fireEvent.click(within(screen.getByRole("group", { name: `Exit ${held.instrument.asset_id}` })).getByRole("button", { name: "Sell it" }));
    await waitFor(() => expect(screen.getByRole("status", { name: `Your exit from ${held.instrument.asset_id}` })).toHaveTextContent("Taken by the agent's runtime"));
    expect(posts.filter((p) => p.path === exitsRoute)).toHaveLength(1);
    expect(keys).toHaveLength(2);
    expect(new Set(keys).size).toBe(2);
  });
});
