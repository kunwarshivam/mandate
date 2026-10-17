import { useEffect } from "react";
import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiRuntimeProvider } from "./api-runtime";
import { type Runtime, useRuntime } from "./mock-runtime";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import { useMarket } from "@/components/charts/chart-parts";
import { FixtureTag } from "@/components/domain/placeholders";
import type { Scenario } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";

const seen: { runtime: Runtime | null; market: unknown } = { runtime: null, market: undefined };
function Probe() {
  const runtime = useRuntime();
  const market = useMarket();
  useEffect(() => {
    seen.runtime = runtime;
    seen.market = market;
  }, [runtime, market]);
  return (
    <>
      <p data-testid="status">{runtime.ws.status}</p>
      <FixtureTag scope="workspace" />
    </>
  );
}

function current(): Runtime {
  if (!seen.runtime) throw new Error("no runtime");
  return seen.runtime;
}

interface Call {
  method: string;
  url: string;
  headers: Headers;
  body: string | null;
  credentials: RequestCredentials | undefined;
}

function mount(scenario: Scenario = "approvals", wrap?: (fetch: Fetch) => Fetch) {
  const server = createMockServer({ workspaceId: WS, scenario });
  const calls: Call[] = [];
  const recordingFetch: Fetch = (input, init) => {
    calls.push({ method: init.method ?? "GET", url: input, headers: new Headers(init.headers), body: typeof init.body === "string" ? init.body : null, credentials: init.credentials });
    return server.fetch(input, init);
  };
  render(
    <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={wrap ? wrap(recordingFetch) : recordingFetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
      <Probe />
    </ApiRuntimeProvider>,
  );
  return { server, calls, posts: () => calls.filter((c) => c.method === "POST") };
}

async function ready() {
  await waitFor(() => expect(screen.getByTestId("status").textContent).toBe("ready"));
}

afterEach(() => {
  seen.runtime = null;
  seen.market = undefined;
  vi.restoreAllMocks();
});

describe("the runtime over the workspace API (E11-9, DEC-975)", () => {
  it("shows nothing until the reads answer, then the workspace they hold, marked as the API's", async () => {
    mount();
    expect(screen.getByTestId("status").textContent).toBe("loading");
    expect(current().ws.agents).toEqual([]);
    await ready();
    const fixture = buildWorkspace("approvals");
    expect(current().source).toBe("api");
    expect(current().ws.agents.map((a) => a.agent_id)).toEqual(fixture.agents.map((a) => a.agent_id));
    expect(current().ws.approvals.map((a) => a.approval_id)).toEqual(fixture.approvals.map((a) => a.approval_id));
    expect(current().reachable).toBe(true);
  });

  it("reads with the browser's own session at the API's origin, and never with a header carrying a token", async () => {
    const { calls } = mount();
    await ready();
    for (const call of calls) {
      expect(call.url.startsWith(`${ORIGIN}/v1/workspaces/${WS}/`), call.url).toBe(true);
      expect(call.credentials).toBe("include");
      expect(call.headers.has("Authorization")).toBe(false);
    }
  });

  it("does not label the API's workspace as fixture data, and draws no fixture market behind it", async () => {
    mount();
    await ready();
    expect(document.querySelector("[data-slot=fixture-tag]")).toBeNull();
    expect(seen.market).toBeNull();
  });

  it("pauses an agent with one idempotent call, shows it recorded only when the API says so, and follows its status", async () => {
    const { posts, calls } = mount();
    await ready();
    const agent = current().ws.agents[0].agent_id;
    let id = "";
    act(() => {
      id = current().send("pause", agent).id;
    });
    expect(current().commands.find((c) => c.id === id)?.phase).toBe("sent");
    await waitFor(() => expect(current().commands.find((c) => c.id === id)?.phase).toBe("recorded"));
    expect(posts()).toHaveLength(1);
    const [post] = posts();
    expect(new URL(post.url).pathname).toBe(`/v1/workspaces/${WS}/agents/${agent}/pause`);
    expect(post.headers.get("Idempotency-Key")).toMatch(/^[A-Za-z0-9_-]{16,64}$/);
    expect(post.headers.get("X-Mandate-Request")).toBe("1");
    await waitFor(() => expect(calls.some((c) => c.method === "GET" && /\/commands\/[0-9A-Z]{26}$/.test(c.url))).toBe(true));
  });

  it("sends the kill switch at the agent's scope, and stopping the account at the connection's", async () => {
    const { posts } = mount();
    await ready();
    const ws = current().ws;
    act(() => {
      current().send("kill", ws.agents[0].agent_id);
      current().send("stop_all", null);
    });
    await waitFor(() => expect(posts()).toHaveLength(2));
    const bodies = posts().map((p) => ({ path: new URL(p.url).pathname, body: JSON.parse(p.body ?? "{}") as { scope: unknown; environment_shown: unknown } }));
    expect(bodies).toEqual([
      { path: `/v1/workspaces/${WS}/kill-switch`, body: expect.objectContaining({ scope: { kind: "agent", id: ws.agents[0].agent_id }, environment_shown: ws.environment }) },
      { path: `/v1/workspaces/${WS}/kill-switch`, body: expect.objectContaining({ scope: { kind: "connection", id: ws.connection.connection_id }, environment_shown: ws.environment }) },
    ]);
  });

  it("pauses every agent not stopped when the owner pauses all, one call each", async () => {
    const { posts } = mount("normal");
    await ready();
    const live = current().ws.agents.filter((a) => a.mode !== "stopped").map((a) => a.agent_id);
    expect(live.length).toBeGreaterThan(1);
    act(() => {
      current().send("pause_all", null);
    });
    await waitFor(() => expect(posts()).toHaveLength(live.length));
    expect(posts().map((p) => new URL(p.url).pathname)).toEqual(live.map((a) => `/v1/workspaces/${WS}/agents/${a}/pause`));
    await waitFor(() => expect(current().commands.at(-1)?.phase).toBe("recorded"));
  });

  it("answers a request with the content hash the read gave, and Skip with no step-up and no delegation", async () => {
    const { posts } = mount();
    await ready();
    const approval = current().ws.approvals.find((a) => a.status === "delivered");
    if (!approval) throw new Error("an open request");
    act(() => current().respond(approval.approval_id, "skip", { screen: "D6", environment: "paper", shown: ["Buy 1 XYZ"], modelOutputExpanded: false }));
    await waitFor(() => expect(current().responses[approval.approval_id]?.phase).toBe("recorded"));
    const [post] = posts();
    expect(new URL(post.url).pathname).toBe(`/v1/workspaces/${WS}/approvals/${approval.approval_id}/responses`);
    expect(JSON.parse(post.body ?? "{}")).toMatchObject({ verdict: "skipped", content_hash: approval.content_hash, step_up: null, delegation: null, record: { artifact: expect.stringMatching(/^sha256:[0-9a-f]{64}$/), ui_build: expect.stringMatching(/^sha256:[0-9a-f]{64}$/) } });
  });

  it("says nothing was sent when an approval needs step-up the deployment did not get, never that it was approved", async () => {
    mount();
    await ready();
    const approval = current().ws.approvals.find((a) => a.status === "delivered");
    if (!approval) throw new Error("an open request");
    act(() => current().respond(approval.approval_id, "approve", { screen: "D6", environment: "paper", shown: ["Buy 1 XYZ"], modelOutputExpanded: false }));
    await waitFor(() => expect(current().responses[approval.approval_id]?.phase).toBe("undelivered"));
    expect(current().responses[approval.approval_id]?.reason).toBe("step_up_required");
    expect(current().ws.approvals.find((a) => a.approval_id === approval.approval_id)?.status).toBe("delivered");
  });

  it("retries an unanswered command with the same key and body, records it once, and calls the result unknown", async () => {
    const { posts, server } = mount("result-unknown");
    await ready();
    let id = "";
    act(() => {
      id = current().send("pause", current().ws.agents[0].agent_id).id;
    });
    await waitFor(() => expect(current().commands.find((c) => c.id === id)?.phase).toBe("unknown"));
    expect(posts().length).toBeGreaterThan(1);
    expect(new Set(posts().map((p) => p.headers.get("Idempotency-Key"))).size).toBe(1);
    expect(new Set(posts().map((p) => p.body)).size).toBe(1);
    expect(server.recorded()).toHaveLength(1);
  });

  it("still pauses every agent last seen, and stops its connection, when the reads stop answering (API-8, rule 13)", async () => {
    let readsDown = false;
    const { posts } = mount("normal", (inner) => (input, init) => (readsDown && (init.method ?? "GET") === "GET" ? Promise.reject(new TypeError("reads are down")) : inner(input, init)));
    await ready();
    const fixture = buildWorkspace("normal");
    const live = fixture.agents.filter((a) => a.mode !== "stopped").map((a) => a.agent_id);
    readsDown = true;
    act(() => {
      current().send("stop_all", null);
    });
    await waitFor(() => expect(screen.getByTestId("status").textContent).toBe("unreachable"));
    expect(current().ws.agents).toEqual([]);
    act(() => {
      current().send("pause_all", null);
    });
    await waitFor(() => expect(posts()).toHaveLength(live.length + 1));
    expect(JSON.parse(posts()[0].body ?? "{}")).toMatchObject({ scope: { kind: "connection", id: fixture.connection.connection_id } });
    expect(posts().slice(1).map((p) => new URL(p.url).pathname)).toEqual(live.map((a) => `/v1/workspaces/${WS}/agents/${a}/pause`));
  });

  it("shows the deployment unreachable, with no figure, when the reads do not answer", async () => {
    mount("unreachable");
    await waitFor(() => expect(screen.getByTestId("status").textContent).toBe("unreachable"));
    expect(current().reachable).toBe(false);
    expect(current().ws.agents).toEqual([]);
  });

  it("refuses set-up and mandate changes it cannot send yet, and sends nothing for them", async () => {
    const { posts } = mount();
    await ready();
    const agent = current().ws.agents[0];
    let deployment: ReturnType<Runtime["deploy"]> | null = null;
    act(() => {
      deployment = current().deploy({ mandate: agent.mandate, provenance: agent.provenance }, { screen: "A5", environment: "paper", shown: [] });
    });
    expect(deployment).not.toBeNull();
    await waitFor(() => expect(current().deployments.at(-1)?.phase).toBe("undelivered"));
    expect(posts()).toHaveLength(0);
  });

  it("writes nothing to browser storage", async () => {
    const local = vi.spyOn(Storage.prototype, "setItem");
    mount();
    await ready();
    act(() => {
      current().send("pause", current().ws.agents[0].agent_id);
    });
    await waitFor(() => expect(current().commands.at(-1)?.phase).toBe("recorded"));
    expect(local).not.toHaveBeenCalled();
  });
});
