import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { InstrumentScreen, InstrumentSearch, matching } from "./instruments";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import { INSTRUMENTS } from "@/fixtures/mandates";
import type { Scenario } from "@/fixtures/types";
import { AGENT_IDS, buildWorkspace } from "@/fixtures/workspace";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { RuntimeProvider } from "@/lib/mock-runtime";
import { RoleProvider } from "@/lib/roles";

/**
 * Search, the instrument's page, and the owner's ask to an agent (DEC-2151, DEC-528, PRD FR-8.6):
 * over the API only, the registered instruments in symbol order, and one request per press that the
 * agent's builder sizes. Never a bare "Buy", and never an example instrument over the fixtures.
 */
const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";
const SWING = buildWorkspace("normal").agents.find((a) => a.agent_id === AGENT_IDS.swing);
if (!SWING) throw new Error("the fixture has the swing agent");

interface Sent {
  path: string;
  body: Record<string, unknown>;
}

function mount(node: React.ReactNode, scenario: Scenario = "normal") {
  const server = createMockServer({ workspaceId: WS, scenario });
  const sent: Sent[] = [];
  const fetch: Fetch = (input, init) => {
    const url = new URL(input);
    if ((init.method ?? "GET") === "POST" && url.pathname.endsWith("/requests")) sent.push({ path: url.pathname, body: JSON.parse(String(init.body)) as Record<string, unknown> });
    return server.fetch(input, init);
  };
  render(
    <RoleProvider>
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        {node}
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
  return sent;
}

const askFor = async (agentId: string) => {
  const items = await screen.findAllByRole("listitem", {}, { timeout: 5_000 });
  const item = items.find((li) => li.dataset.agent === agentId);
  if (!item) throw new Error(`no ask for ${agentId}`);
  return item;
};

afterEach(() => cleanup());

describe("instrument search (DEC-528 item 2: display only)", () => {
  it("filters by symbol, ignoring case, in symbol order: a filter, never a ranking", () => {
    const all = Object.values(INSTRUMENTS).map((i) => ({ instrument_id: i.asset_id, symbol: i.symbol, asset_class: i.asset_class, snapshot: `sha256:${"0".repeat(64)}` as const, registered_at: "2026-10-29T00:00:00.000000000Z" }));
    expect(matching(all, "").map((i) => i.symbol)).toEqual(["ABC", "BTC/USD", "LMN", "QRS", "XYZ"]);
    expect(matching(all, " b ").map((i) => i.symbol)).toEqual(["ABC", "BTC/USD"]);
    expect(matching(all, "nope")).toEqual([]);
  });

  it("lists the deployment's registered instruments, each linking to its page, and narrows as the owner types", async () => {
    mount(<InstrumentSearch />);
    await waitFor(() => expect(document.querySelectorAll('[data-slot="instrument"]')).toHaveLength(Object.keys(INSTRUMENTS).length), { timeout: 5_000 });
    fireEvent.change(screen.getByLabelText("Search by symbol"), { target: { value: "xy" } });
    const rows = document.querySelectorAll<HTMLElement>('[data-slot="instrument"]');
    expect(rows).toHaveLength(1);
    expect(within(rows[0]).getByRole("link", { name: "XYZ" }).getAttribute("href")).toBe(`/instruments/${INSTRUMENTS.XYZ.asset_id}`);
    expect(document.body.textContent).not.toMatch(/recommend|suggest|top pick|trending/i);
  });

  it("shows nothing to search over the fixtures, rather than the example instruments", () => {
    render(
      <RuntimeProvider initial={buildWorkspace("normal")} tick={false}>
        <InstrumentSearch />
      </RuntimeProvider>,
    );
    expect(screen.getByText(/nothing to search/)).toBeTruthy();
    expect(document.body.textContent).not.toContain("XYZ");
  });
});

describe("asking an agent to buy (DEC-2151)", () => {
  it("sends one request with the instrument and the typed quantity, and says the agent took it", async () => {
    const sent = mount(<InstrumentScreen instrumentId={INSTRUMENTS.XYZ.asset_id} />);
    const ask = await askFor(SWING.agent_id);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("XYZ");
    expect(document.querySelector('[data-slot="quote"]')?.textContent).toBe("Quote not reported by your deployment.");
    expect(screen.queryAllByRole("button", { name: /^buy$/i })).toEqual([]);
    fireEvent.change(within(ask).getByLabelText("Quantity of XYZ"), { target: { value: " 2.5 " } });
    fireEvent.click(within(ask).getByRole("button", { name: "Ask agent to buy" }));
    await waitFor(() => expect(ask.querySelector('[data-slot="request-taken"]')).not.toBeNull(), { timeout: 5_000 });
    expect(sent).toEqual([{ path: `/v1/workspaces/${WS}/agents/${SWING.agent_id}/requests`, body: { instrument: INSTRUMENTS.XYZ.asset_id, side: "buy", size: "2.5" } }]);
    expect(within(ask).getByRole("link", { name: "Approvals" }).getAttribute("href")).toBe("/approvals");
  });

  it("says the agent took the request as soon as the command reads taken (§5.5), while its approval waits", async () => {
    const server = createMockServer({ workspaceId: WS, scenario: "normal" });
    let statusReads = 0;
    const fetch: Fetch = async (input, init) => {
      const url = new URL(input);
      if (/\/commands\/[0-9A-Z]{26}$/.test(url.pathname)) {
        statusReads += 1;
        return new Response(JSON.stringify({ api_version: "v1", as_of: [], phase: "taken", steps: [] }), { status: 200, headers: { "Content-Type": "application/json" } });
      }
      return server.fetch(input, init);
    };
    render(
      <RoleProvider>
        <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={50} retryMs={5}>
          <InstrumentScreen instrumentId={INSTRUMENTS.XYZ.asset_id} />
        </ApiRuntimeProvider>
      </RoleProvider>,
    );
    const ask = await askFor(SWING.agent_id);
    fireEvent.change(within(ask).getByLabelText("Quantity of XYZ"), { target: { value: "1" } });
    fireEvent.click(within(ask).getByRole("button", { name: "Ask agent to buy" }));
    await waitFor(() => expect(ask.querySelector('[data-slot="request-taken"]')?.textContent).toMatch(/^Your agent took the request/), { timeout: 1_000 });
    expect(statusReads).toBe(1);
  });

  it("shows each agent's mode, and says a paused or exits-only agent won't open a position", async () => {
    mount(<InstrumentScreen instrumentId={INSTRUMENTS.XYZ.asset_id} />, "paused");
    await askFor(SWING.agent_id);
    const running = buildWorkspace("paused").agents.filter((a) => a.mode !== "stopped");
    expect(running.some((a) => a.mode === "paused")).toBe(true);
    for (const agent of running) {
      const item = document.querySelector<HTMLElement>(`[data-agent="${agent.agent_id}"]`);
      expect(item, agent.agent_id).not.toBeNull();
      const held = item?.querySelector('[data-slot="ask-held"]');
      if (agent.mode === "paused") expect(held?.textContent).toMatch(/won't open new positions while paused/);
      else if (agent.mode === "exits_only") expect(held?.textContent).toMatch(/only exits positions/);
      else expect(held).toBeNull();
    }
  });

  it("sends nothing for a quantity that is not above zero, and says what to enter", async () => {
    const sent = mount(<InstrumentScreen instrumentId={INSTRUMENTS.XYZ.asset_id} />);
    const ask = await askFor(SWING.agent_id);
    for (const size of ["", "0", "-1", "two", "1e3"]) {
      fireEvent.change(within(ask).getByLabelText("Quantity of XYZ"), { target: { value: size } });
      fireEvent.click(within(ask).getByRole("button", { name: "Ask agent to buy" }));
      await waitFor(() => expect(ask.querySelector('[data-reason="invalid_size"]')).not.toBeNull());
    }
    expect(sent).toEqual([]);
  });

  it("says an agent whose mandate does not pin the instrument cannot be asked for it, as the deployment refuses", async () => {
    const other = buildWorkspace("normal").agents.find((a) => !a.mandate.universe.pinned_instruments.some((i) => i.asset_id === INSTRUMENTS.XYZ.asset_id) && a.mode !== "stopped");
    if (!other) throw new Error("the fixture has an agent that does not pin XYZ");
    const sent = mount(<InstrumentScreen instrumentId={INSTRUMENTS.XYZ.asset_id} />);
    const ask = await askFor(other.agent_id);
    fireEvent.change(within(ask).getByLabelText("Quantity of XYZ"), { target: { value: "1" } });
    fireEvent.click(within(ask).getByRole("button", { name: "Ask agent to buy" }));
    await waitFor(() => expect(ask.querySelector('[data-reason="instrument_not_pinned"]')).not.toBeNull(), { timeout: 5_000 });
    expect(ask.textContent).toContain("This agent's mandate doesn't include XYZ");
    expect(sent).toHaveLength(1);
  });

  it("says an instrument the deployment has not registered is not registered", async () => {
    mount(<InstrumentScreen instrumentId="7b4a1c2e-9999-4a2b-9c3d-000000000009" />);
    expect(await screen.findByText("Instrument not registered", {}, { timeout: 5_000 })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Ask agent to buy" })).toBeNull();
  });
});
