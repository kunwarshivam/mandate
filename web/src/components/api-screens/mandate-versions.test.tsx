import { readFileSync } from "node:fs";
import { join } from "node:path";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ApiAgent } from "./api-screens";
import { ApiMandateVersions, REJECTED } from "./mandate-versions";
import { type Fetch } from "@/api/client";
import { REJECTION_REASONS } from "@/api/mandate-versions";
import { createMockServer } from "@/api/mock-server";
import { buildWorkspace } from "@/fixtures/workspace";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { RoleProvider } from "@/lib/roles";

/**
 * The versions screen over the API (§4.7, DEC-2147): what the deployment serves, newest first, and
 * "not reported" when the read fails, never the fixture agent's own versions.
 */
const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";
const FIXTURE = buildWorkspace("normal").agents[0];
const AGENT = FIXTURE.agent_id;
const BASE = JSON.parse(readFileSync(join(__dirname, "..", "..", "..", "..", "schemas", "workspace-api", "examples", "read-models.agent-mandate-versions.json"), "utf8")) as Record<string, unknown> & { versions: Array<Record<string, unknown>> };
const ROUTE = `/v1/workspaces/${WS}/agents/${AGENT}/mandate-versions`;

const problem = (status: number, code: string) => new Response(JSON.stringify({ type: "about:blank", status, title: code, code, effect: "none", event_id: null, retryable: false }), { status, headers: { "Content-Type": "application/problem+json" } });

function mount(answer: Record<string, unknown> | number | null, screenFor: "versions" | "agent" = "versions") {
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const fetch: Fetch = (input, init) => {
    if (answer === null || new URL(input).pathname !== ROUTE) return server.fetch(input, init);
    if (typeof answer === "number") return Promise.resolve(problem(answer, answer === 404 ? "not_found" : "journal_unavailable"));
    return Promise.resolve(new Response(JSON.stringify({ ...answer, agent_id: AGENT }), { status: 200, headers: { "Content-Type": "application/json" } }));
  };
  render(
    <RoleProvider>
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        {screenFor === "versions" ? <ApiMandateVersions agentId={AGENT} /> : <ApiAgent agentId={AGENT} />}
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
}

afterEach(() => cleanup());

describe("the versions screen over the API (§4.7)", () => {
  it("lists the served history newest first, tags only the version in effect, and says how each applied", async () => {
    mount(BASE);
    await waitFor(() => expect(document.querySelectorAll('[data-slot="version"]')).toHaveLength(3));
    const items = [...document.querySelectorAll<HTMLElement>('[data-slot="version"]')];
    expect(items.map((li) => within(li).getByRole("heading").textContent)).toEqual(["Change 2", "Change 1", "Deployed version"]);
    expect(items.map((li) => li.dataset.current === "true")).toEqual([false, true, false]);
    expect(within(items[0]).queryByText("In effect")).toBeNull();
    expect(within(items[1]).getByText("In effect")).toBeTruthy();
    expect(items[0].querySelector('[data-classification="risk_increasing"]')).not.toBeNull();
    expect(items[0].querySelector('[data-slot="version-application"]')?.textContent).toMatch(/^Not applied yet: a risk-increasing version applies at the agent's next check/);
    expect(items[1].querySelector('[data-slot="version-application"]')?.textContent).toBe("Applied 2026-10-22 10:05:03 UTC.");
    expect(items[2].querySelector('[data-slot="version-application"]')?.textContent).toBe("Deployed 2026-10-20 09:10:00 UTC.");
    expect(document.body.textContent).not.toContain(FIXTURE.label);
  });

  it("says why a rejected version did not apply, in words for every reason the schema lists", async () => {
    expect(Object.keys(REJECTED).sort()).toEqual([...REJECTION_REASONS].sort());
    const later = structuredClone(BASE.versions);
    later[2].application = { result: "rejected", reason: "would_trigger_limit", recorded_at: "2026-10-27T11:05:02.000000000Z" };
    mount({ ...BASE, versions: later });
    await waitFor(() => expect(document.querySelectorAll('[data-slot="version"]')).toHaveLength(3));
    const newest = document.querySelector<HTMLElement>('[data-slot="version"]');
    expect(newest?.textContent).toContain("Not applied");
    expect(newest?.querySelector('[data-slot="version-application"]')?.textContent).toBe(`Not applied: the agent refused it 2026-10-27 11:05:02 UTC, because ${REJECTED.would_trigger_limit}. The version before it stayed in effect.`);
    expect(newest?.textContent).not.toContain("would_trigger_limit");
  });

  it("puts nothing in effect once the agent stopped, or when the account's record was not found", async () => {
    mount({ ...BASE, stopped_at: "2026-10-28T09:00:00.000000000Z", in_effect: null });
    expect(await screen.findByText("The agent stopped 2026-10-28 09:00:00 UTC: no version is in effect.", {}, { timeout: 5_000 })).toBeTruthy();
    expect(document.querySelector('[data-current="true"]')).toBeNull();
    expect(document.querySelector<HTMLElement>('[data-slot="version"] [data-slot="version-application"]')?.textContent).toBe("Not applied before the agent stopped.");
    cleanup();
    mount({ ...BASE, account_ref: null, in_effect: null });
    expect(await screen.findByText(/Which version is in effect is not reported/, {}, { timeout: 5_000 })).toBeTruthy();
    expect(document.querySelector('[data-current="true"]')).toBeNull();
    expect(document.querySelector<HTMLElement>('[data-slot="version"] [data-slot="version-application"]')?.textContent).toMatch(/^Whether it applied is not reported/);
  });

  it("says the versions are not reported when the read fails or is malformed, and shows no fixture version", async () => {
    for (const answer of [503, { ...BASE, versions: [] }]) {
      mount(answer);
      expect(await screen.findByText("The versions are not reported.", {}, { timeout: 5_000 })).toBeTruthy();
      expect(document.querySelector('[data-slot="version"]')).toBeNull();
      cleanup();
    }
    mount(404);
    expect(await screen.findByText("Your deployment reports no such agent.", {}, { timeout: 5_000 })).toBeTruthy();
  });

  it("reads the mock server's history for the fixture agent end to end", async () => {
    mount(null);
    await waitFor(() => expect(document.querySelectorAll('[data-slot="version"]')).toHaveLength(FIXTURE.versions.length), { timeout: 5_000 });
    expect(document.querySelectorAll('[data-current="true"]')).toHaveLength(1);
  });

  it("is linked from the agent's page as Versions", async () => {
    mount(BASE, "agent");
    const link = await screen.findByRole("link", { name: "Versions" }, { timeout: 5_000 });
    expect(link.getAttribute("href")).toBe(`/agents/${AGENT}/mandate/versions`);
  });
});
