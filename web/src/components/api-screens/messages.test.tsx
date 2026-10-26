import type { ReactElement } from "react";
import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import MessagesPage from "@/app/(app)/messages/page";
import ThreadPage from "@/app/(app)/messages/[agentId]/page";
import DeskPage from "@/app/(app)/messages/[agentId]/desk/page";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import { buildWorkspace } from "@/fixtures/workspace";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { RoleProvider } from "@/lib/roles";
import { setPathname } from "@/test/navigation";

/**
 * Messages over the workspace API: each agent's thread is its journal as the deployment serves it,
 * with its alerts named and a waiting request linked, and nothing in a missing read's place.
 */
const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";
const T = (n: number) => `2026-10-26T10:00:${String(n).padStart(2, "0")}.000000000Z`;

type Wrap = (send: Fetch) => Fetch;

async function mount(page: () => Promise<ReactElement> | ReactElement, path: string, wrap: Wrap = (send) => send) {
  setPathname(path);
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const element = await page();
  render(
    <RoleProvider initial="owner">
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={wrap(server.fetch)} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        <main>{element}</main>
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
  return server;
}

const json = (body: unknown, status = 200) =>
  Promise.resolve(new Response(JSON.stringify(body), { status, headers: { "content-type": status === 200 ? "application/json" : "application/problem+json" } }));

const envelope = { api_version: "v1", build: `sha256:${"ab".repeat(32)}`, served_at: T(59), as_of: [{ stream_id: `ctl:${WS}`, seq: 1, hash: `sha256:${"cd".repeat(32)}`, recorded_at: T(1) }] };

/** Serves `events` as the agent stream's journal pages, honouring `after_seq` and `limit`. */
function journal(agentId: string, events: Array<{ event_id: string; event_type: string }>): Wrap {
  const stream = `agent:${WS}:${agentId}`;
  const lines = events.map((e, i) => ({ seq: i + 1, event_id: e.event_id, event_type: e.event_type, recorded_at: T(i + 1), prev_hash: "0".repeat(64), hash: "0".repeat(64), body: "e30=" }));
  return (send) => (input, init) => {
    const url = new URL(input);
    if (decodeURIComponent(url.pathname).endsWith(`/journal/streams/${stream}/events`)) {
      const after = Number(url.searchParams.get("after_seq") ?? "0");
      const limit = Number(url.searchParams.get("limit") ?? "100");
      const page = lines.filter((l) => l.seq > after).slice(0, limit);
      return json({ ...envelope, stream_id: stream, head: { seq: lines.length, hash: "0".repeat(64), recorded_at: T(59) }, events: page, next_after_seq: page.at(-1)?.seq ?? after, at_head: true });
    }
    return send(input, init);
  };
}

const and = (a: Wrap, b: Wrap): Wrap => (send) => a(b(send));

afterEach(() => cleanup());

describe("messages over the workspace API", () => {
  it("lists every agent's thread from the agents read, with what waits for you", async () => {
    await mount(MessagesPage, "/messages");
    const fixture = buildWorkspace("normal");
    const threads = await screen.findAllByRole("link", { name: new RegExp(fixture.agents[0].mandate.name.slice(0, 6)) });
    expect(threads.length).toBeGreaterThan(0);
    const list = screen.getByRole("list", { name: "Threads" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(fixture.agents.length);
    expect(document.querySelector("[data-slot=api-not-yet]")).toBeNull();
    expect(document.querySelector("[data-slot=fixture-tag]")).toBeNull();
  });

  it("shows a thread as the agent's records, an alert by its kind and a waiting request by its link", async () => {
    const fixture = buildWorkspace("normal");
    const server = createMockServer({ workspaceId: WS, scenario: "normal" });
    const pending = (await (await server.fetch(`${ORIGIN}/v1/workspaces/${WS}/approvals?state=pending`, { method: "GET", headers: {} })).json()) as { approvals: Array<{ approval_id: string; agent_id: string }> };
    const ask = pending.approvals[0];
    expect(ask, "the normal scenario has a request waiting").toBeDefined();
    const agent = fixture.agents.find((a) => a.agent_id === ask.agent_id)!;
    const alertId = "01JBH3BTZ8K2M4N6P8R0S2T4V8";
    const events = [
      { event_id: "01JBH3BTZ8K2M4N6P8R0S2T401", event_type: "AgentDeployed" },
      { event_id: ask.approval_id, event_type: "ApprovalRequested" },
      { event_id: "01JBH3BTZ8K2M4N6P8R0S2T402", event_type: "KillSwitchActivated" },
      { event_id: alertId, event_type: "OwnerAlertSent" },
    ];
    const alerts: Wrap = (send) => (input, init) =>
      new URL(input).pathname.endsWith("/alerts")
        ? json({
            ...envelope,
            truncated: false,
            alerts: [
              {
                alert_id: alertId,
                kind: "kill_switch",
                class: "safety",
                raised_at: T(4),
                stream: { type: "agent", agent_id: agent.agent_id, account_ref: null },
                subject: { event_id: "01JBH3BTZ8K2M4N6P8R0S2T402", event_type: "KillSwitchActivated" },
                owner_command: null,
                notice: null,
              },
            ],
          })
        : send(input, init);
    await mount(() => ThreadPage({ params: Promise.resolve({ agentId: agent.agent_id }) }), `/messages/${agent.agent_id}`, and(journal(agent.agent_id, events), alerts));
    const records = await screen.findByRole("list", { name: "Thread records" });
    await within(records).findByText("Kill switch activated");
    const shown = within(records).getAllByRole("listitem").map((li) => li.getAttribute("data-type"));
    expect(shown).toEqual(events.map((e) => e.event_type));
    expect(within(records).getByText("AgentDeployed")).toBeInTheDocument();
    expect(within(records).getByRole("link", { name: "A request waits for you" })).toHaveAttribute("href", `/approvals/${ask.approval_id}`);
  });

  it("names a thread that could not be read, never shows it empty", async () => {
    const agent = buildWorkspace("normal").agents[0];
    const failing: Wrap = (send) => (input, init) =>
      new URL(input).pathname.includes("/journal/streams/") ? json({ type: "about:blank", title: "Journal unavailable", status: 503, code: "journal_unavailable", retryable: true }, 503) : send(input, init);
    await mount(() => ThreadPage({ params: Promise.resolve({ agentId: agent.agent_id }) }), `/messages/${agent.agent_id}`, failing);
    expect(await screen.findByText(/The thread is not reported/)).toBeInTheDocument();
    expect(screen.queryByText("Nothing recorded yet.")).toBeNull();
  });

  it("says the desk is not served rather than showing an example", async () => {
    const agent = buildWorkspace("normal").agents[0];
    await mount(() => DeskPage({ params: Promise.resolve({ agentId: agent.agent_id }) }), `/messages/${agent.agent_id}/desk`);
    expect(await screen.findByText("Not available from your deployment yet")).toBeInTheDocument();
  });
});
