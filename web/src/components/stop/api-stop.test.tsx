import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import type { AgentDetail, ApiView } from "@/api/read-models";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { buildApiRecord } from "./api-record";
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
function mount(ui: React.ReactNode) {
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const posts: Array<{ path: string; body: Record<string, unknown> }> = [];
  const fetch: Fetch = (input, init) => {
    if ((init.method ?? "GET") === "POST") posts.push({ path: new URL(input).pathname, body: JSON.parse(typeof init.body === "string" ? init.body : "{}") as Record<string, unknown> });
    return server.fetch(input, init);
  };
  render(
    <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
      {ui}
    </ApiRuntimeProvider>,
  );
  return { posts };
}

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

  it("sends stop-all scoped to the served connection", async () => {
    const connection = createMockServer({ workspaceId: WS, scenario: "normal" });
    const read = await connection.fetch(`/v1/workspaces/${WS}/connections`, { method: "GET" });
    const id = ((await read.json()) as { connections: Array<{ connection_id: string }> }).connections[0].connection_id;
    expect(recordHref("stop_all", id)).toBe(`/connections/${id}/stop-all`);
    const { posts } = mount(<StopRecordScreen kind="stop_all" targetId={id} />);
    fireEvent.click(await screen.findByRole("button", { name: /^Stop all agents|^Activate/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Use passkey" }));
    await waitFor(() => expect(posts.filter((p) => p.path.endsWith("/kill-switch"))).toHaveLength(1), { timeout: 5000 });
    expect(posts.find((p) => p.path.endsWith("/kill-switch"))?.body).toMatchObject({ scope: { kind: "connection", id } });
  });
});
