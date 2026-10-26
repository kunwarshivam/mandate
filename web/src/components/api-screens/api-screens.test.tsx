import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ApiAgent, ApiAgents, ApiApproval, ApiApprovals, ApiHome, BySource } from "./api-screens";
import { type Fetch } from "@/api/client";
import { MOCK_PRINCIPAL, type MockServer, createMockServer } from "@/api/mock-server";
import { Providers } from "@/components/providers";
import { WorkspaceGate } from "@/components/screens/common";
import type { Scenario } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { quantity } from "@/lib/format";
import { RoleProvider } from "@/lib/roles";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";

type Tamper = (route: string, body: Record<string, unknown>) => Record<string, unknown>;

function mount(node: React.ReactNode, scenario: Scenario = "approvals", tamper?: Tamper, hostConfirms = true) {
  const server = createMockServer({ workspaceId: WS, scenario, hostConfirms });
  const posts: Array<{ path: string; body: Record<string, unknown> }> = [];
  const fetch: Fetch = async (input, init) => {
    const url = new URL(input);
    const route = url.pathname.slice(`/v1/workspaces/${WS}`.length);
    if (init.method === "POST") posts.push({ path: route, body: JSON.parse(String(init.body)) as Record<string, unknown> });
    const answer = await server.fetch(input, init);
    if (!tamper || init.method !== "GET" || !answer.ok) return answer;
    return new Response(JSON.stringify(tamper(route, (await answer.json()) as Record<string, unknown>)), { status: answer.status });
  };
  render(
    <RoleProvider>
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        {node}
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
  return { posts };
}

/** Mounts over a fetch the test writes itself, in front of the mock. */
function mountWith(node: React.ReactNode, fetchVia: (input: string, init: RequestInit, server: MockServer) => Promise<Response>) {
  const server = createMockServer({ workspaceId: WS, scenario: "approvals" });
  const fetch: Fetch = (input, init) => fetchVia(input, init, server);
  render(
    <RoleProvider>
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        {node}
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
}

const fixture = buildWorkspace("approvals");
const agentOf = (id: string) => fixture.agents.find((a) => a.agent_id === id)!;
/** The API names an approval by its event id, a bare ULID; the fixtures prefix theirs with `apr_`. */
const pending = fixture.approvals.filter((a) => a.status === "delivered").map((a) => ({ ...a, approval_id: a.approval_id.replace(/^apr_/, "") }));

afterEach(() => {
  cleanup();
});

describe("the screens over the workspace API's read models (DEC-1096)", () => {
  it("Home lists each waiting request by the agent's name and the order it binds, and every agent with its mode", async () => {
    mount(<ApiHome />);
    const needs = await screen.findByRole("region", { name: /^Needs you/ });
    for (const approval of pending) {
      const name = agentOf(approval.agent_id).mandate.name;
      expect(within(needs).getByRole("link", { name: new RegExp(`^${name} asks to buy ${approval.bound.qty.replace(".", "\\.")} ${approval.bound.symbol.replace("/", "\\/")}`) }).getAttribute("href")).toBe(`/approvals/${approval.approval_id}`);
    }
    const agents = screen.getByRole("region", { name: "Agents" });
    for (const agent of fixture.agents) expect(within(agents).getByRole("link", { name: agent.mandate.name }).getAttribute("href")).toBe(`/agents/${agent.agent_id}`);
    expect(document.querySelector("[data-slot=fixture-tag]")).toBeNull();
  });

  it("names what the API does not report as not reported, and never fills it in", async () => {
    const blank: Tamper = (route, body) => {
      if (route === "/agents") return { ...body, agents: (body.agents as object[]).map((a) => ({ ...a, label: null, mode: null })) };
      if (/^\/agents\/[^/]+$/.test(route)) return { ...body, agent: { ...(body.agent as object), label: null, mode: null, mandate: null, account_positions: null } };
      return body;
    };
    mount(<ApiHome />, "approvals", blank);
    const agents = await screen.findByRole("region", { name: "Agents" });
    expect(within(agents).getAllByText("Name not reported")).toHaveLength(fixture.agents.length);
    expect(within(agents).getAllByText("Mode not reported")).toHaveLength(fixture.agents.length);
    cleanup();
    mount(<ApiAgent agentId={fixture.agents[0].agent_id} />, "approvals", blank);
    expect(await screen.findByText("Mandate not reported")).toBeTruthy();
    expect(screen.getByText("Account positions not reported")).toBeTruthy();
    expect(screen.queryByText(/\$10,000/)).toBeNull();
  });

  it("an agent shows its orders, newest first, and its own positions, from the served reads (DEC-2111)", async () => {
    const agent = fixture.agents.find((a) => a.orders.length + a.past_orders.length > 0 && a.positions.length > 0)!;
    mount(<ApiAgent agentId={agent.agent_id} />);
    const orders = await screen.findByRole("list", { name: "Orders" });
    const rows = within(orders).getAllByRole("listitem");
    expect(rows).toHaveLength(agent.orders.length + agent.past_orders.length);
    const newest = [...agent.past_orders, ...agent.orders].at(-1)!;
    expect(rows[0].textContent).toContain(`${newest.side === "buy" ? "Buy" : "Sell"} ${quantity(newest.qty)} ${newest.instrument.symbol}`);
    const held = await screen.findByRole("list", { name: "This agent's positions" });
    expect(within(held).getAllByRole("listitem")).toHaveLength(agent.positions.length);
  });

  it("an agent lists the latest records on its stream, by type and time, from the journal read", async () => {
    const agent = fixture.agents.find((a) => a.orders.length + a.past_orders.length > 0)!;
    const { posts } = mount(<ApiAgent agentId={agent.agent_id} />);
    const journal = await screen.findByRole("list", { name: "Journal" });
    const rows = within(journal).getAllByRole("listitem");
    expect(rows[0].textContent).toContain("#1 AgentDeployed");
    expect(rows.some((r) => r.textContent?.includes("OrderSubmitted"))).toBe(true);
    expect(posts).toEqual([]);
  });

  it("a journal the role may not read says so, and shows no records", async () => {
    const agent = fixture.agents[0];
    mountWith(<ApiAgent agentId={agent.agent_id} />, (input, init, server) =>
      input.includes("/journal/") ? Promise.resolve(new Response(JSON.stringify({ code: "forbidden", effect: "none", retryable: false }), { status: 403 })) : server.fetch(input, init),
    );
    expect(await screen.findByText("Not available to your role.")).toBeTruthy();
    expect(screen.queryByRole("list", { name: "Journal" })).toBeNull();
  });

  it("an order list the API cannot answer says not reported, never none", async () => {
    const agent = fixture.agents[0];
    mount(<ApiAgent agentId={agent.agent_id} />, "approvals", (route, body) => (/\/(orders|positions)$/.test(route) ? { ...body, agent_id: "agt_someone_else" } : body));
    expect(await screen.findByText("Orders not reported.")).toBeTruthy();
    expect(screen.queryByText("No order yet.")).toBeNull();
    expect(screen.queryByText("This agent holds nothing.")).toBeNull();
  });

  it("an agent the deployment found no account for says its orders are not reported", async () => {
    const agent = fixture.agents[0];
    mount(<ApiAgent agentId={agent.agent_id} />, "approvals", (route, body) => (/\/orders$/.test(route) ? { ...body, account_ref: null, orders: [] } : body));
    expect(await screen.findByText(/^Orders not reported: the deployment found no account/)).toBeTruthy();
  });

  it("an agent shows its mandate's envelope, its account's positions, and limit usage as not reported yet", async () => {
    const agent = fixture.agents[1];
    mount(<ApiAgent agentId={agent.agent_id} />);
    expect(await screen.findByRole("heading", { level: 1, name: agent.mandate.name })).toBeTruthy();
    const mandate = screen.getByRole("region", { name: "Mandate" });
    expect(within(mandate).getByText("$10,000.00")).toBeTruthy();
    const positions = screen.getByRole("region", { name: "Account positions" });
    for (const p of agent.positions) expect(within(positions).getByText(p.instrument.symbol)).toBeTruthy();
    expect(within(screen.getByRole("region", { name: "Limit usage" })).getByText(/not reported/i)).toBeTruthy();
    expect(screen.queryByText(/[+−]\$/)).toBeNull();
  });

  it("an agent the API does not know says so", async () => {
    mount(<ApiAgent agentId="agt_01JB3K8Y4N7QW2M6R9T5V0ZZZZ" />);
    expect(await screen.findByRole("heading", { name: "Agent not found" })).toBeTruthy();
  });

  it("the inbox lists waiting requests by deadline, then resolved ones with how they ended", async () => {
    mount(<ApiApprovals />);
    const waiting = await screen.findByRole("region", { name: /^Waiting/ });
    const links = within(waiting).getAllByRole("link").map((l) => l.getAttribute("href"));
    const byDeadline = [...pending].sort((a, b) => Date.parse(a.deadline) - Date.parse(b.deadline)).map((a) => `/approvals/${a.approval_id}`);
    expect(links).toEqual(byDeadline);
  });

  it("a request shows the order it binds, and Skip sends its content hash through the responses route", async () => {
    const approval = pending[0];
    const { posts } = mount(<ApiApproval approvalId={approval.approval_id} />);
    expect(await screen.findByText(new RegExp(`^Buy ${approval.bound.qty} ${approval.bound.symbol.replace("/", "\\/")} at a limit of`))).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Skip" }));
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0].path).toBe(`/approvals/${approval.approval_id}/responses`);
    expect(posts[0].body).toMatchObject({ verdict: "skipped", content_hash: expect.stringMatching(/^sha256:[0-9a-f]{64}$/), step_up: null });
    expect(await screen.findByText(/Recorded in the journal/)).toBeTruthy();
  });

  it("a request names its agent's environment, and says not reported, never paper, when the agent is not served (DEC-1096)", async () => {
    const approval = pending[0];
    mount(<ApiApproval approvalId={approval.approval_id} />);
    await screen.findByRole("button", { name: "Skip" });
    expect(screen.getByText("PAPER")).toBeTruthy();
    cleanup();
    mount(<ApiApproval approvalId={approval.approval_id} />, "approvals", (route, body) => (route === "/agents" ? { ...body, agents: [] } : body));
    await screen.findByRole("button", { name: "Skip" });
    expect(screen.getByText("ENVIRONMENT NOT REPORTED")).toBeTruthy();
    expect(screen.queryByText("PAPER")).toBeNull();
  });

  it("Approve asks for a code first and sends no answer: the owner confirms it on the host (DEC-2104)", async () => {
    const approval = pending[0];
    const { posts } = mount(<ApiApproval approvalId={approval.approval_id} />);
    fireEvent.click(await screen.findByRole("button", { name: "Approve" }));
    const panel = await screen.findByRole("region", { name: "Confirm on the host" });
    expect(posts.map((p) => p.path)).toEqual(["/step-up/challenges"]);
    expect(posts[0].body).toEqual({ action: { kind: "approve", digest: expect.stringMatching(/^sha256:[0-9a-f]{64}$/) } });
    const code = within(panel).getByText(/^[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}$/).textContent ?? "";
    const command = panel.querySelector("[data-slot=host-confirm-command]")?.textContent ?? "";
    expect(command).toContain(`mandate step-up confirm --workspace ${WS} --principal ${MOCK_PRINCIPAL} --agent ${approval.agent_id} --approval ${approval.approval_id} --challenge `);
    expect(command).toContain(`--code ${code}`);
    expect(screen.queryByText(/^Recorded in the journal/)).toBeNull();
  });

  it("an approve the host confirmed is recorded with that challenge's evidence", async () => {
    const approval = pending[0];
    const { posts } = mount(<ApiApproval approvalId={approval.approval_id} />);
    fireEvent.click(await screen.findByRole("button", { name: "Approve" }));
    const panel = await screen.findByRole("region", { name: "Confirm on the host" });
    fireEvent.click(within(panel).getByRole("button", { name: "Approve" }));
    expect(await screen.findByText(/^Recorded in the journal: you approved/)).toBeTruthy();
    const answer = posts.find((p) => p.path.endsWith("/responses"));
    expect(answer?.body).toMatchObject({ verdict: "approved", content_hash: posts[0].body.action && (posts[0].body.action as { digest: string }).digest, step_up: { method: "cli_confirm", assertion_id: expect.stringMatching(/^[0-7][0-9A-HJKMNP-TV-Z]{25}$/) } });
    expect(screen.queryByRole("region", { name: "Confirm on the host" })).toBeNull();
  });

  it("an approve the host has not confirmed says nothing was approved, and the owner can approve again", async () => {
    const approval = pending[0];
    mount(<ApiApproval approvalId={approval.approval_id} />, "approvals", undefined, false);
    fireEvent.click(await screen.findByRole("button", { name: "Approve" }));
    const panel = await screen.findByRole("region", { name: "Confirm on the host" });
    fireEvent.click(within(panel).getByRole("button", { name: "Approve" }));
    expect(await screen.findByText(/found no confirmation of this code on the host, so nothing was approved/)).toBeTruthy();
    expect(screen.queryByText(/^Recorded in the journal/)).toBeNull();
    expect(within(screen.getByRole("region", { name: "Confirm on the host" })).getByRole("button", { name: "Approve" })).toBeTruthy();
  });

  it("Cancel drops the code and sends nothing more", async () => {
    const approval = pending[0];
    const { posts } = mount(<ApiApproval approvalId={approval.approval_id} />);
    fireEvent.click(await screen.findByRole("button", { name: "Approve" }));
    fireEvent.click(within(await screen.findByRole("region", { name: "Confirm on the host" })).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("region", { name: "Confirm on the host" })).toBeNull();
    expect(screen.getByRole("button", { name: "Skip" })).toBeTruthy();
    expect(posts.map((p) => p.path)).toEqual(["/step-up/challenges"]);
  });

  it("no code from the deployment says nothing was approved, and asks the host for nothing", async () => {
    const approval = pending[0];
    const refused = () => Promise.resolve(new Response(JSON.stringify({ code: "live_unavailable", effect: "none", retryable: false }), { status: 403 }));
    mountWith(<ApiApproval approvalId={approval.approval_id} />, (input, init, server) => (init.method === "POST" && input.endsWith("/step-up/challenges") ? refused() : server.fetch(input, init)));
    fireEvent.click(await screen.findByRole("button", { name: "Approve" }));
    expect(await screen.findByText("No code: a host confirmation is for paper only. Nothing was approved.")).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Confirm on the host" })).toBeNull();
  });

  it("an expired code cannot be sent: its life is counted from when it was shown", async () => {
    const approval = pending[0];
    const shortLived = async (input: string, init: RequestInit, server: MockServer) => {
      const answer = await server.fetch(input, init);
      if (!(init.method === "POST" && input.endsWith("/step-up/challenges"))) return answer;
      const body = (await answer.json()) as { issued_at: string };
      const second = `${new Date(Date.parse(`${body.issued_at.slice(0, 23)}Z`) + 200).toISOString().slice(0, 23)}000000Z`;
      return new Response(JSON.stringify({ ...body, expires_at: second }), { status: 201 });
    };
    mountWith(<ApiApproval approvalId={approval.approval_id} />, shortLived);
    fireEvent.click(await screen.findByRole("button", { name: "Approve" }));
    const panel = await screen.findByRole("region", { name: "Confirm on the host" });
    expect(await within(panel).findByText(/This code has expired/)).toBeTruthy();
    expect((within(panel).getByRole("button", { name: "Approve" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("the agents screen lists every agent the API serves", async () => {
    mount(<ApiAgents />);
    const list = await screen.findByRole("region", { name: "Agents" });
    expect(within(list).getAllByRole("link")).toHaveLength(fixture.agents.length);
  });

  it("a fixture screen over the API says it is not available yet, and shows nothing", async () => {
    mount(
      <WorkspaceGate>
        <p data-testid="fixture-screen">figures</p>
      </WorkspaceGate>,
    );
    expect(await screen.findByRole("heading", { name: "Not available from your deployment yet" })).toBeTruthy();
    expect(screen.queryByTestId("fixture-screen")).toBeNull();
  });

  it("picks the screen by the source: the API's over the API, the fixture screen over the fixtures", async () => {
    mount(<BySource fixtures={<p>fixture screen</p>} api={<p>api screen</p>} />);
    expect(await screen.findByText("api screen")).toBeTruthy();
    cleanup();
    await act(async () => {
      render(
        <Providers workspace={buildWorkspace("normal")} tick={false}>
          <BySource fixtures={<p>fixture screen</p>} api={<p>api screen</p>} />
        </Providers>,
      );
    });
    expect(screen.getByText("fixture screen")).toBeTruthy();
    expect(screen.queryByText("api screen")).toBeNull();
  });
});
