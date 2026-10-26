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

function mount(wrap: (send: Fetch) => Fetch = (send) => send) {
  setPathname("/audit/decisions");
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  render(
    <RoleProvider initial="owner">
      <ApiRuntimeProvider source={{ origin: "https://api.owlhead.ai", workspaceId: WS }} fetch={wrap((input, init) => server.fetch(input, init))} refreshMs={60_000}>
        <main>
          <RegistryScreen screen={findScreen("/audit/decisions")!} />
        </main>
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
}

afterEach(() => cleanup());

describe("gate decisions over the workspace API (journal spec §9.1 DecisionMade)", () => {
  it("lists every decision the agents' streams hold, newest first, with the dry run and autonomy in words", async () => {
    mount();
    const fixture = buildWorkspace("normal");
    const list = await screen.findByRole("list", { name: "Gate decisions" });
    await waitFor(() => expect(within(list).getAllByRole("listitem")).toHaveLength(fixture.decisions.length));
    const verdicts = within(list)
      .getAllByRole("listitem")
      .map((li) => li.getAttribute("data-dry-run"));
    expect(verdicts.sort()).toEqual(fixture.decisions.map((d) => d.verdict).sort());
    expect(list.textContent).toMatch(/Allowed by the dry run|Denied by the dry run/);
    expect(document.querySelector("[data-slot=fixture-tag]")).toBeNull();
  });

  it("shows a record whose body does not match its hash as that, and reads nothing from it", async () => {
    mount((send) => async (input, init) => {
      const answer = await send(input, init);
      if (!new URL(input).pathname.includes("/journal/streams/")) return answer;
      const body = (await answer.json()) as { events: Array<Record<string, unknown>> };
      const events = body.events.map((e) => (e.event_type === "DecisionMade" ? { ...e, hash: "f".repeat(64) } : e));
      return new Response(JSON.stringify({ ...body, events }), { status: 200, headers: { "Content-Type": "application/json" } });
    });
    const list = await screen.findByRole("list", { name: "Gate decisions" });
    await waitFor(() => expect(list).toHaveTextContent("Its record does not match its hash, so it is not read."));
    expect(list.querySelector("[data-matches=true]")).toBeNull();
    expect(list).not.toHaveTextContent("Allowed by the dry run");
  });
});
