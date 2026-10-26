import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { type Fetch } from "@/api/client";
import { MOCK_PASSKEY_ID, createMockServer } from "@/api/mock-server";
import type { Authenticator } from "@/api/passkey";
import type { AgentDetail, ApiView } from "@/api/read-models";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { type Role, RoleProvider } from "@/lib/roles";
import { sha256Hex } from "@/lib/sha256";
import { mockAuthenticator } from "@/test/set-up";
import { UNREAD_CONNECTION, buildApiRecord } from "./api-record";
import { recordHref } from "./commands";
import { recordLines } from "./record";
import { StopRecordScreen } from "./record-screen";
import { StopSheet } from "./stop-sheet";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";

/**
 * The kill switch over the workspace API (rule 13, DEC-1096): the Stop sheet and the D10 screens
 * build from what the API serves, so the switch is reachable there, and nothing the API does not
 * serve is shown as "none".
 */
function mount(ui: React.ReactNode, scenario: "normal" | "unreachable" | "silent" = "normal", options: { authenticator?: Authenticator | null; role?: Role; wrap?: (send: Fetch) => Fetch } = {}) {
  const server = createMockServer({ workspaceId: WS, scenario: scenario === "silent" ? "normal" : scenario });
  const posts: Array<{ path: string; body: Record<string, unknown> }> = [];
  const fetch: Fetch = (input, init) => {
    const method = init.method ?? "GET";
    if (method === "POST") posts.push({ path: new URL(input).pathname, body: JSON.parse(typeof init.body === "string" ? init.body : "{}") as Record<string, unknown> });
    if (scenario === "silent" && method === "GET") return new Promise<Response>(() => {});
    return (options.wrap ? options.wrap(server.fetch) : server.fetch)(input, init);
  };
  render(
    <RoleProvider initial={options.role ?? "owner"}>
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5} {...("authenticator" in options ? { authenticator: options.authenticator } : {})}>
        {ui}
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
  return { posts };
}

/** DEC-2102 item 1's object for a resume or Stop at the control head, digested its own way (DEC-2140 item 2); the mock's head is the count of commands it recorded. */
const commandDigest = (confirms: string, agentId: string, head: number) =>
  `sha256:${sha256Hex(`{"agent":"${agentId}","bid":null,"bid_size":null,"confirms":"${confirms}","control_head":"${head}","event":null,"floor":null,"instrument":null}`)}`;

const agent = (over: Partial<AgentDetail>): AgentDetail => ({
  agent_id: "agt_1",
  environment: "paper",
  mandate_version: `sha256:${"ab".repeat(32)}`,
  deployed_at: "2026-10-18T00:00:00.000000000Z",
  mode: "normal",
  pending_approvals: 0,
  label: "Agent 1",
  mandate: null,
  account_positions: [],
  ...over,
});

describe("the kill switch's record over the API", () => {
  const view: ApiView = { agents: [agent({}), agent({ agent_id: "agt_2", label: null, mode: "stopped" })], approvals: [] };

  it("says what the API does not serve is not reported, never that nothing is canceled or sold", () => {
    const record = buildApiRecord("kill", view, "paper", "con_1", "agt_1");
    expect(record).not.toBeNull();
    const lines = recordLines(record!);
    expect(lines.some((l) => l.startsWith("Not reported: the deployment does not serve open orders"))).toBe(true);
    expect(lines.some((l) => l.startsWith("Not reported for one agent"))).toBe(true);
    expect(lines).not.toContain("No open orders, so nothing is canceled.");
    expect(lines).not.toContain("No positions, so nothing is sold.");
    expect(record).toMatchObject({ agentId: "agt_1", nothingToDo: null, modes: [{ agentId: "agt_1", label: "Agent 1", mode: "normal" }] });
    expect(lines).toContain("agt_2, with its orders and positions");
  });

  it("is for an agent or connection the API serves only, and never a release, whose positions it cannot show", () => {
    expect(buildApiRecord("kill", view, "paper", "con_1", "agt_9")).toBeNull();
    expect(buildApiRecord("stop_all", view, "paper", "con_1", "con_2")).toBeNull();
    expect(buildApiRecord("stop_all", view, "paper", "", "")).toBeNull();
    expect(buildApiRecord("release", view, "paper", "con_1", "agt_1")).toBeNull();
    expect(buildApiRecord("kill", view, "paper", "con_1", "agt_2")?.nothingToDo).toMatch(/^Stopped/);
    expect(buildApiRecord("stop_all", view, "paper", "con_1", "con_1")?.lists.find((l) => l.key === "agents")?.items).toEqual(["Agent 1"]);
  });

  it("builds the account's switch with no read at all, says the agents are not available, and names the workspace scope when no connection was read", () => {
    const unread = buildApiRecord("stop_all", null, "unknown", "", UNREAD_CONNECTION);
    expect(unread).not.toBeNull();
    expect(unread).toMatchObject({ nothingToDo: null, agentId: null, modes: [], subject: "Every agent in this workspace" });
    expect(unread!.scope).toMatch(/^The account has not been read, so this is the workspace's kill switch/);
    expect(recordLines(unread!).some((l) => l.startsWith("Not available: the agents could not be read"))).toBe(true);
    expect(buildApiRecord("stop_all", null, "paper", "con_1", "con_1")?.subject).toBe("The workspace's account");
    expect(buildApiRecord("close_all", null, "unknown", "", UNREAD_CONNECTION)).toBeNull();
    expect(buildApiRecord("stop_all", null, "unknown", "con_1", UNREAD_CONNECTION)).toBeNull();
    expect(buildApiRecord("kill", null, "unknown", "", "agt_1")).toBeNull();
  });
});

describe("the Stop sheet and D10 over the API (rule 13)", () => {
  it("offers each served agent's kill switch, and no release it cannot show", async () => {
    mount(<StopSheet open onOpenChange={() => {}} agentId={null} />);
    const sheet = await screen.findByRole("dialog", { name: /^Stop/ });
    const kills = await within(sheet).findAllByRole("link", { name: /^Kill switch:/ });
    expect(kills.length).toBeGreaterThan(0);
    for (const link of kills) expect(link.getAttribute("href")).toMatch(/^\/agents\/agt_[0-9A-Z]{26}\/kill-switch$/);
    expect(within(sheet).queryByRole("link", { name: /release positions/i })).toBeNull();
  });

  it("sends the agent's kill switch, scoped to that agent, from its record screen", async () => {
    const { posts } = mount(<StopRecordScreen kind="kill" targetId="agt_01JB3K8Y4N7QW2M6R9T5V0XZAC" />);
    fireEvent.click(await screen.findByRole("button", { name: /^Activate the kill switch/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Use passkey" }));
    await waitFor(() => expect(posts.filter((p) => p.path.endsWith("/kill-switch"))).toHaveLength(1), { timeout: 5000 });
    expect(posts.find((p) => p.path.endsWith("/kill-switch"))?.body).toMatchObject({ scope: { kind: "agent", id: "agt_01JB3K8Y4N7QW2M6R9T5V0XZAC" }, environment_shown: "paper" });
  });

  it("says on the sheet what close-all sends over the API, never that it closes every position", async () => {
    mount(<StopSheet open onOpenChange={() => {}} agentId={null} />);
    const sheet = await screen.findByRole("dialog", { name: /^Stop/ });
    const closeAll = await within(sheet).findByRole("link", { name: /^Close everything on this account/ });
    expect(closeAll).toHaveTextContent("From here this sends the account's kill switch: each agent on the account is stopped by its own kill switch.");
    expect(closeAll.textContent).not.toMatch(/cancel-all and close-all|closes every position/);
  });

  it("says the agents are unavailable when they cannot be read, and keeps the account's kill switch", async () => {
    mount(<StopSheet open onOpenChange={() => {}} agentId={null} />, "unreachable");
    const sheet = await screen.findByRole("dialog", { name: /^Stop/ });
    await waitFor(() => expect(sheet.querySelector("[data-slot=agents-unavailable]")).not.toBeNull());
    expect(within(sheet).queryAllByRole("link", { name: /^Kill switch:/ })).toHaveLength(0);
    expect(within(sheet).getByRole("link", { name: /^Stop all agents on this account/ }).getAttribute("href")).toBe(`/connections/${UNREAD_CONNECTION}/stop-all`);
    expect(within(sheet).queryByRole("link", { name: /^Close everything/ })).toBeNull();
  });

  it.each(["unreachable", "silent"] as const)("sends the account's switch with no read (%s), as the workspace's, which its screen named", async (reads) => {
    const { posts } = mount(<StopRecordScreen kind="stop_all" targetId={UNREAD_CONNECTION} />, reads);
    expect(await screen.findByText(/^The account has not been read, so this is the workspace's kill switch/)).toBeTruthy();
    fireEvent.click(await screen.findByRole("button", { name: /^Stop all agents on this account/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Use passkey" }));
    await waitFor(() => expect(posts.filter((p) => p.path.endsWith("/kill-switch")).length).toBeGreaterThan(0), { timeout: 5000 });
    for (const p of posts.filter((p) => p.path.endsWith("/kill-switch"))) expect(p.body).toMatchObject({ scope: { kind: "workspace", id: null } });
  });

  it("says close-all sends the account's kill switch, the same as stop-all, never the broker's close-all", () => {
    const record = buildApiRecord("close_all", { agents: [agent({})], approvals: [] }, "paper", "con_1", "con_1");
    expect(record?.scope).toMatch(/^From here this sends the account's kill switch/);
    expect(recordLines(record!).join("\n")).not.toMatch(/every position closed|your own holdings included|cancel-all and close-position for the whole account/);
  });

  it("sends stop-all scoped to the served connection", async () => {
    const connection = createMockServer({ workspaceId: WS, scenario: "normal" });
    const read = await connection.fetch(`/v1/workspaces/${WS}/connections`, { method: "GET" });
    const id = ((await read.json()) as { connections: Array<{ connection_id: string }> }).connections[0].connection_id;
    expect(recordHref("stop_all", id)).toBe(`/connections/${id}/stop-all`);
    const { posts } = mount(<StopRecordScreen kind="stop_all" targetId={id} />);
    fireEvent.click(await screen.findByRole("button", { name: /^Stop all agents on this account/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Use passkey" }));
    await waitFor(() => expect(posts.filter((p) => p.path.endsWith("/kill-switch"))).toHaveLength(1), { timeout: 5000 });
    expect(posts.find((p) => p.path.endsWith("/kill-switch"))?.body).toMatchObject({ scope: { kind: "connection", id } });
  });
});

describe("resume and Stop over the API, with the owner's passkey (DEC-2140)", () => {
  const AGENT = "agt_01JB3KAQ5R8TV2N4M6P9S1W3XD";
  const sheet = () => screen.findByRole("dialog", { name: /^Stop/ });
  const passkey = () => mockAuthenticator(MOCK_PASSKEY_ID);

  it("resumes a paused agent with a passkey bound to resume, the agent and the control head the deployment served", async () => {
    const authenticator = passkey();
    const { posts } = mount(<StopSheet open onOpenChange={() => {}} agentId={AGENT} />, "normal", { authenticator: authenticator.get });
    fireEvent.click(await within(await sheet()).findByRole("button", { name: /^Pause (?!all)/ }));
    const resume = await within(await sheet()).findByRole("button", { name: /^Resume / }, { timeout: 5000 });
    fireEvent.click(resume);
    await waitFor(() => expect(posts.filter((p) => p.path.endsWith(`/agents/${AGENT}/resume`))).toHaveLength(1), { timeout: 5000 });
    const challenge = posts.find((p) => p.path.endsWith("/step-up/challenges"))!.body as { action: { kind: string; digest: string } };
    expect(challenge.action).toEqual({ kind: "resume", digest: commandDigest("resume", AGENT, 1) });
    const body = posts.find((p) => p.path.endsWith("/resume"))!.body;
    expect(Object.keys(body).sort()).toEqual(["record", "step_up", "step_up_assertion"]);
    expect(body.step_up).toMatchObject({ method: "passkey" });
    expect(body.step_up_assertion).toMatchObject({ passkey_id: MOCK_PASSKEY_ID });
    expect(authenticator.calls).toHaveLength(1);
  });

  it("stops an agent with a passkey bound to stop, saying a Stop sells nothing and the kill switch does", async () => {
    const { posts } = mount(<StopSheet open onOpenChange={() => {}} agentId={AGENT} />, "normal", { authenticator: passkey().get });
    const stop = await within(await sheet()).findByRole("button", { name: /^Stop / });
    expect(stop).toHaveTextContent("A Stop sells nothing; the kill switch below sells its positions.");
    fireEvent.click(stop);
    await waitFor(() => expect(posts.filter((p) => p.path.endsWith(`/agents/${AGENT}/stop`))).toHaveLength(1), { timeout: 5000 });
    const challenge = posts.find((p) => p.path.endsWith("/step-up/challenges"))!.body as { action: { kind: string; digest: string } };
    expect(challenge.action).toEqual({ kind: "stop", digest: commandDigest("stop", AGENT, 0) });
    expect(posts.find((p) => p.path.endsWith("/stop"))!.body).toMatchObject({ release: false, warning_shown: null, step_up: { method: "passkey" } });
  });

  it("sends nothing when the browser cannot use a passkey, says so in words, and points to the kill switch", async () => {
    const { posts } = mount(<StopSheet open onOpenChange={() => {}} agentId={AGENT} />, "normal", { authenticator: null });
    fireEvent.click(await within(await sheet()).findByRole("button", { name: /^Stop / }));
    await waitFor(() => expect(document.querySelector("[data-phase=undelivered]")).toHaveTextContent("Not sent: this browser can't use a passkey. Nothing changed. The kill switch still works."), { timeout: 5000 });
    expect(posts.filter((p) => /\/(stop|resume)$/.test(p.path))).toEqual([]);
  });

  const entry = () => document.querySelector("[data-slot=stop-sheet] [data-phase], [data-phase]");

  it("says the Stop is stopped only once the runtime applied it, and that the runtime refused one it refused", async () => {
    mount(<StopSheet open onOpenChange={() => {}} agentId={AGENT} />, "normal", { authenticator: passkey().get });
    fireEvent.click(await within(await sheet()).findByRole("button", { name: /^Stop / }));
    await waitFor(() => expect(entry()).toHaveTextContent(/is stopped\.$/), { timeout: 5000 });
    cleanup();
    mount(<StopSheet open onOpenChange={() => {}} agentId={AGENT} />, "normal", {
      authenticator: passkey().get,
      wrap: (send) => async (input, init) => {
        const answer = await send(input, init);
        if (!/\/commands\/[^/]+$/.test(new URL(input).pathname)) return answer;
        const body = (await answer.json()) as Record<string, unknown>;
        return new Response(JSON.stringify({ ...body, phase: body.phase === "applied" ? "refused" : body.phase }), { status: 200, headers: { "Content-Type": "application/json" } });
      },
    });
    fireEvent.click(await within(await sheet()).findByRole("button", { name: /^Stop / }));
    await waitFor(() => expect(entry()).toHaveTextContent("then refused by the runtime: nothing changed from this request."), { timeout: 5000 });
    expect(entry()).not.toHaveTextContent("is stopped");
  });

  it("asks one passkey and sends one Stop however often it is pressed while the first is in flight", async () => {
    const authenticator = passkey();
    const { posts } = mount(<StopSheet open onOpenChange={() => {}} agentId={AGENT} />, "normal", { authenticator: authenticator.get });
    const stop = await within(await sheet()).findByRole("button", { name: /^Stop / });
    fireEvent.click(stop);
    fireEvent.click(stop);
    await waitFor(() => expect(posts.filter((p) => p.path.endsWith(`/agents/${AGENT}/stop`))).toHaveLength(1), { timeout: 5000 });
    await waitFor(() => expect(entry()).toHaveTextContent(/is stopped\.$/), { timeout: 5000 });
    expect(posts.filter((p) => p.path.endsWith("/step-up/challenges"))).toHaveLength(1);
    expect(authenticator.calls).toHaveLength(1);
  });

  it("sends nothing when the control stream moved while the passkey was asked, and says to try again", async () => {
    let agentReads = 0;
    const { posts } = mount(<StopSheet open onOpenChange={() => {}} agentId={AGENT} />, "normal", {
      authenticator: passkey().get,
      wrap: (send) => async (input, init) => {
        const answer = await send(input, init);
        if ((init.method ?? "GET") !== "GET" || !new URL(input).pathname.endsWith(`/agents/${AGENT}`)) return answer;
        agentReads += 1;
        const body = (await answer.json()) as { as_of: Array<{ stream_id: string; seq: number }> };
        return new Response(JSON.stringify({ ...body, as_of: body.as_of.map((w) => ({ ...w, seq: w.seq + agentReads })) }), { status: 200, headers: { "Content-Type": "application/json" } });
      },
    });
    fireEvent.click(await within(await sheet()).findByRole("button", { name: /^Stop / }));
    await waitFor(() => expect(document.querySelector("[data-phase=undelivered]")).toHaveTextContent("Not sent: something else was recorded while your passkey was asked, so its check no longer matches. Try again. Nothing changed."), { timeout: 5000 });
    expect(posts.filter((p) => p.path.endsWith(`/agents/${AGENT}/stop`))).toEqual([]);
  });

  it("offers neither to a role that cannot stop", async () => {
    mount(<StopSheet open onOpenChange={() => {}} agentId={AGENT} />, "normal", { role: "approver" });
    const shown = await sheet();
    await within(shown).findByRole("button", { name: /^Pause (?!all)/ });
    expect(within(shown).queryByRole("button", { name: /^(Resume|Stop) / })).toBeNull();
  });
});
