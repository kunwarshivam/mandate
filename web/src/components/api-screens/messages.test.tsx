import type { ReactElement } from "react";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import MessagesPage from "@/app/(app)/messages/page";
import ThreadPage from "@/app/(app)/messages/[agentId]/page";
import DeskPage from "@/app/(app)/messages/[agentId]/desk/page";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import { ApiMessages } from "@/components/api-screens/messages";
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
    const hrefs = within(list).getAllByRole("link").map((a) => a.getAttribute("href"));
    expect([...hrefs].sort()).toEqual(fixture.agents.map((a) => `/messages/${a.agent_id}`).sort());
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
    expect(records.querySelector('[data-kind="kill_switch"]')?.getAttribute("data-type")).toBe("OwnerAlertSent");
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
    expect(await screen.findByText("The desk is not available from your deployment yet")).toBeInTheDocument();
    expect(document.querySelectorAll("h1")).toHaveLength(1);
  });
});

/** The thread for `agentId` with `events` on its stream, and `/alerts` answered by `alertsBody` (or the mock's empty list). */
async function thread(agentId: string, events: Array<{ event_id: string; event_type: string }>, extra: Wrap = (send) => send) {
  await mount(() => ThreadPage({ params: Promise.resolve({ agentId }) }), `/messages/${agentId}`, and(journal(agentId, events), extra));
}

const problemOf = (code: string, status: number) => json({ type: "about:blank", title: code, status, code, retryable: status >= 500 }, status);

describe("messages over the workspace API: what a thread says when a read holds nothing or fails", () => {
  const agent = () => buildWorkspace("normal").agents[0];

  it("says nothing is recorded for an empty stream and for a stream not found", async () => {
    await thread(agent().agent_id, []);
    expect(await screen.findByText("Nothing recorded yet.")).toBeInTheDocument();
    cleanup();
    const missing: Wrap = (send) => (input, init) => (new URL(input).pathname.includes("/journal/streams/") ? problemOf("not_found", 404) : send(input, init));
    await mount(() => ThreadPage({ params: Promise.resolve({ agentId: agent().agent_id }) }), `/messages/${agent().agent_id}`, missing);
    expect(await screen.findByText("Nothing recorded yet.")).toBeInTheDocument();
  });

  it("says a stream the role may not read is not available to it", async () => {
    const forbidden: Wrap = (send) => (input, init) => (new URL(input).pathname.includes("/journal/streams/") ? problemOf("forbidden", 403) : send(input, init));
    await mount(() => ThreadPage({ params: Promise.resolve({ agentId: agent().agent_id }) }), `/messages/${agent().agent_id}`, forbidden);
    expect(await screen.findByText("Not available to your role.")).toBeInTheDocument();
  });

  it("shows records by their type, and says so, when the alerts read fails", async () => {
    const down: Wrap = (send) => (input, init) => (new URL(input).pathname.endsWith("/alerts") ? problemOf("journal_unavailable", 503) : send(input, init));
    await thread(agent().agent_id, [{ event_id: "01JBH3BTZ8K2M4N6P8R0S2T4V8", event_type: "OwnerAlertSent" }], down);
    expect(await screen.findByText(/Alerts not reported/)).toBeInTheDocument();
    const records = screen.getByRole("list", { name: "Thread records" });
    expect(within(records).getByText("OwnerAlertSent")).toBeInTheDocument();
    expect(records.querySelector("[data-kind]")).toBeNull();
  });

  it("links no request that is resolved or another agent's, and names no alert it was not served", async () => {
    const fixture = buildWorkspace("normal");
    const server = createMockServer({ workspaceId: WS, scenario: "normal" });
    const read = async (state: string) =>
      ((await (await server.fetch(`${ORIGIN}/v1/workspaces/${WS}/approvals?state=${state}`, { method: "GET", headers: {} })).json()) as { approvals: Array<{ approval_id: string; agent_id: string }> }).approvals;
    const [pending, resolved] = [await read("pending"), await read("resolved")];
    const mine = fixture.agents.find((a) => pending.some((p) => p.agent_id !== a.agent_id))!;
    const others = pending.filter((p) => p.agent_id !== mine.agent_id).map((p) => ({ event_id: p.approval_id, event_type: "ApprovalRequested" }));
    const done = resolved.slice(0, 1).map((p) => ({ event_id: p.approval_id, event_type: "ApprovalRequested" }));
    expect(others.length, "another agent has a request waiting").toBeGreaterThan(0);
    expect(done.length, "a request was resolved").toBeGreaterThan(0);
    const elsewhere: Wrap = (send) => (input, init) =>
      new URL(input).pathname.endsWith("/alerts")
        ? json({
            ...envelope,
            truncated: false,
            alerts: [
              {
                alert_id: "01JBH3BTZ8K2M4N6P8R0S2T4VA",
                kind: "risk_limit",
                class: "safety",
                raised_at: T(9),
                stream: { type: "agent", agent_id: mine.agent_id, account_ref: null },
                subject: { event_id: "01JBH3BTZ8K2M4N6P8R0S2T4VB", event_type: "RiskLimitTripped" },
                owner_command: null,
                notice: null,
              },
            ],
          })
        : send(input, init);
    await thread(mine.agent_id, [...others, ...done, { event_id: "01JBH3BTZ8K2M4N6P8R0S2T4V9", event_type: "OwnerAlertSent" }], elsewhere);
    const records = await screen.findByRole("list", { name: "Thread records" });
    expect(within(records).queryByRole("link")).toBeNull();
    expect(records.querySelector("[data-kind]")).toBeNull();
  });

  it("says when older alerts were left out of the alerts read", async () => {
    const cut: Wrap = (send) => (input, init) => (new URL(input).pathname.endsWith("/alerts") ? json({ ...envelope, alerts: [], truncated: true }) : send(input, init));
    await thread(agent().agent_id, [{ event_id: "01JBH3BTZ8K2M4N6P8R0S2T401", event_type: "AgentDeployed" }], cut);
    expect(await screen.findByText(/Older alerts are not listed/)).toBeInTheDocument();
  });

  it("says an agent the deployment does not report is not there, and an empty deployment has no threads", async () => {
    await mount(() => ThreadPage({ params: Promise.resolve({ agentId: "agt_01JBH3BTZ8K2M4N6P8R0S2T4V9" }) }), "/messages/agt_01JBH3BTZ8K2M4N6P8R0S2T4V9");
    expect(await screen.findByText("Your deployment reports no such agent.")).toBeInTheDocument();
    cleanup();
    const none: Wrap = (send) => (input, init) => (new URL(input).pathname.endsWith("/agents") ? json({ ...envelope, agents: [] }) : send(input, init));
    await mount(MessagesPage, "/messages", none);
    expect(await screen.findByText("Your deployment reports no agents.")).toBeInTheDocument();
  });

  it("never shows one agent's records under another's name while the other's are read", async () => {
    const [a, b] = buildWorkspace("normal").agents;
    const server = createMockServer({ workspaceId: WS, scenario: "normal" });
    const hold: Wrap = (send) => (input, init) => (decodeURIComponent(new URL(input).pathname).includes(`/journal/streams/agent:${WS}:${b.agent_id}/`) ? new Promise<Response>(() => {}) : send(input, init));
    const fetch = and(journal(a.agent_id, [{ event_id: "01JBH3BTZ8K2M4N6P8R0S2T401", event_type: "AgentDeployed" }]), hold)(server.fetch);
    const tree = (agentId: string) => (
      <RoleProvider initial="owner">
        <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
          <main>
            <ApiMessages agentId={agentId} view="chat" />
          </main>
        </ApiRuntimeProvider>
      </RoleProvider>
    );
    setPathname(`/messages/${a.agent_id}`);
    const { rerender } = render(tree(a.agent_id));
    await screen.findByText("AgentDeployed");
    await act(async () => rerender(tree(b.agent_id)));
    expect(screen.queryByText("AgentDeployed")).toBeNull();
    expect(screen.getByText("Reading the thread.")).toBeInTheDocument();
  });
});
