import { readFileSync } from "node:fs";
import { join } from "node:path";
import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import AlertsPage from "@/app/(app)/alerts/page";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { RoleProvider } from "@/lib/roles";
import { setPathname } from "@/test/navigation";

/**
 * The alerts screen over the workspace API (`GET /alerts`, DEC-2141): it shows what the read serves
 * and nothing else. The served body is the schema's own example, so the screen is tested against
 * the published shape, not one written for it.
 */
const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";
const EXAMPLE = readFileSync(join(__dirname, "..", "..", "..", "..", "schemas", "workspace-api", "examples", "read-models.alerts.json"), "utf8");

function mount(alerts: ((send: Fetch) => Fetch) | null) {
  setPathname("/alerts");
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const reads: string[] = [];
  const fetch: Fetch = (input, init) => {
    reads.push(new URL(input).pathname);
    return server.fetch(input, init);
  };
  render(
    <RoleProvider initial="owner">
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={alerts ? alerts(fetch) : fetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        <main>
          <AlertsPage />
        </main>
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
  return { reads };
}

/** The served body for `/alerts`, everything else from the mock server. */
const serving = (body: string, status = 200) => (send: Fetch): Fetch => (input, init) =>
  new URL(input).pathname.endsWith("/alerts") ? Promise.resolve(new Response(body, { status, headers: { "content-type": status === 200 ? "application/json" : "application/problem+json" } })) : send(input, init);

afterEach(() => cleanup());

describe("alerts over the workspace API (§4.7, DEC-2141)", () => {
  it("says none yet when the deployment records no alert, and reads GET /alerts", async () => {
    const { reads } = mount(null);
    expect(await screen.findByText("None yet.")).toBeInTheDocument();
    expect(reads).toContain(`/v1/workspaces/${WS}/alerts`);
    expect(document.querySelector("[data-slot=api-not-yet]")).toBeNull();
  });

  it("lists each served alert by its kind's name, newest first, with my delivery and a link to its subject", async () => {
    mount(serving(EXAMPLE));
    const rows = await screen.findAllByRole("listitem");
    const alerts = rows.filter((r) => r.getAttribute("data-slot") === "api-alert");
    expect(alerts.map((r) => r.getAttribute("data-kind"))).toEqual(["kill_switch", "protection"]);
    const [kill, protection] = alerts;
    expect(within(kill).getByText("Kill switch activated")).toBeInTheDocument();
    expect(within(kill).getByText(/Email delivered\./)).toBeInTheDocument();
    expect(within(kill).getByRole("link")).toHaveAttribute("href", "/audit");
    expect(within(protection).getByText("A position's protection changed")).toBeInTheDocument();
    expect(within(protection).getByText(/No notice recorded for this alert\./)).toBeInTheDocument();
    expect(within(protection).getByRole("link")).toHaveAttribute("href", "/positions");
  });

  it("never shows a read that failed as no alerts", async () => {
    mount(serving(JSON.stringify({ type: "about:blank", title: "Journal unavailable", status: 503, code: "journal_unavailable", retryable: true }), 503));
    expect(await screen.findByText(/Alerts not reported/)).toBeInTheDocument();
    expect(screen.queryByText("None yet.")).toBeNull();
  });

  it("refuses a body with a kind it does not name, rather than guessing a name", async () => {
    const body = JSON.parse(EXAMPLE) as { alerts: Array<{ kind: string }> };
    body.alerts[0].kind = "recovery_used";
    mount(serving(JSON.stringify(body)));
    expect(await screen.findByText(/Alerts not reported/)).toBeInTheDocument();
    expect(document.querySelector("[data-slot=api-alert]")).toBeNull();
  });
});
