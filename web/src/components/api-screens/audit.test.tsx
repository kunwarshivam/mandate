import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import { RegistryScreen } from "@/components/screens/account-screens";
import { buildWorkspace } from "@/fixtures/workspace";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { RoleProvider } from "@/lib/roles";
import { findScreen } from "@/lib/screens";
import { setPathname } from "@/test/navigation";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";

function mount(wrap: (send: Fetch) => Fetch = (send) => send) {
  setPathname("/audit/timeline");
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const reads: string[] = [];
  const fetch: Fetch = (input, init) => {
    if ((init.method ?? "GET") === "GET") reads.push(decodeURIComponent(new URL(input).pathname));
    return server.fetch(input, init);
  };
  render(
    <RoleProvider initial="owner">
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={wrap(fetch)} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        <main>
          <RegistryScreen screen={findScreen("/audit/timeline")!} />
        </main>
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
  return { reads };
}

afterEach(() => cleanup());

describe("the audit timeline over the workspace API (§4.8.1, API-15)", () => {
  it("merges the control stream, each agent's and the account's records, newest first, as the journal holds them", async () => {
    const { reads } = mount();
    const fixture = buildWorkspace("normal");
    const list = await screen.findByRole("list", { name: "Journal records" });
    const streams = [`ctl:${WS}`, ...fixture.agents.map((a) => `agent:${WS}:${a.agent_id}`), `acct:${WS}:${fixture.connection.connection_id}`];
    await waitFor(() => {
      const seen = new Set(within(list).getAllByRole("listitem").map((li) => li.getAttribute("data-stream")));
      for (const s of streams) expect(seen.has(s), s).toBe(true);
    });
    for (const s of streams) expect(reads.some((r) => r === `/v1/workspaces/${WS}/journal/streams/${s}/events`), s).toBe(true);
    const times = within(list)
      .getAllByRole("listitem")
      .map((li) => li.textContent ?? "");
    expect(times.length).toBeGreaterThan(fixture.agents.length);
    expect(document.querySelector("[data-slot=fixture-tag]")).toBeNull();
    expect(document.querySelector("[data-slot=api-not-yet]")).toBeNull();
  });

  it("names a stream it could not read as not reported, and still shows the others", async () => {
    mount((send) => (input, init) => (decodeURIComponent(new URL(input).pathname).includes(`/journal/streams/ctl:${WS}/`) ? Promise.reject(new TypeError("down")) : send(input, init)));
    await waitFor(() => expect(screen.getByRole("main")).toHaveTextContent("Not reported: Workspace."));
    expect(screen.getByRole("list", { name: "Journal records" })).toBeInTheDocument();
  });

  it("orders the records newest first by when the journal recorded them", async () => {
    mount();
    const list = await screen.findByRole("list", { name: "Journal records" });
    await waitFor(() => expect(within(list).getAllByRole("listitem").some((li) => li.getAttribute("data-type") === "FillApplied")).toBe(true));
    const at = within(list)
      .getAllByRole("listitem")
      .map((li) => li.getAttribute("data-at") ?? "");
    expect([...at].sort().reverse()).toEqual(at);
  });
});
