import { useEffect } from "react";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiRuntimeProvider } from "./api-runtime";
import { type Runtime, useRuntime } from "./mock-runtime";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import type { SessionKeeper, SessionState } from "@/api/session";
import { useMarket } from "@/components/charts/chart-parts";
import { WorkspaceGate } from "@/components/screens/common";
import { FixtureTag } from "@/components/domain/placeholders";
import type { Scenario } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";
import { confirmStepUpDigest } from "@/api/confirm";
import { CONNECT_START_ROUTE } from "@/api/connect";
import { CONFIRM_RECORD, DEPLOY_RECORD, mockAuthenticator } from "@/test/set-up";
import { type Authenticator, deployStepUpDigest } from "@/api/passkey";
import { MOCK_PASSKEY_ID } from "@/api/mock-server";
import { AGENT_ID } from "./ids";

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

/** The API's own view of the workspace, once the reads have answered. */
function view() {
  const v = current().view;
  if (!v) throw new Error("no view");
  return v;
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

/** A session keeper that records what the runtime asked of it. */
function fakeSession(opens: SessionState[] = ["open"], refreshes: SessionState[] = []) {
  const asked: string[] = [];
  const session: SessionKeeper = {
    open: async () => (asked.push("open"), opens.shift() ?? "open"),
    refresh: async () => (asked.push("refresh"), refreshes.shift() ?? "open"),
    signOut: async () => (asked.push("signOut"), "signed_out"),
    guard: (call) => (asked.push("guard"), call()),
  };
  return { session, asked };
}

function mount(scenario: Scenario = "approvals", wrap?: (fetch: Fetch) => Fetch, session?: SessionKeeper, timing: { refreshMs?: number; keepAliveMs?: number; authenticator?: Authenticator | null } = {}) {
  const server = createMockServer({ workspaceId: WS, scenario });
  const calls: Call[] = [];
  const recordingFetch: Fetch = (input, init) => {
    calls.push({ method: init.method ?? "GET", url: input, headers: new Headers(init.headers), body: typeof init.body === "string" ? init.body : null, credentials: init.credentials });
    return server.fetch(input, init);
  };
  render(
    <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={wrap ? wrap(recordingFetch) : recordingFetch} session={session} refreshMs={timing.refreshMs ?? 60_000} keepAliveMs={timing.keepAliveMs} statusPollMs={5} retryMs={5} authenticator={timing.authenticator === undefined ? mockAuthenticator(MOCK_PASSKEY_ID).get : timing.authenticator}>
      <Probe />
      <WorkspaceGate>
        <p data-testid="gated">workspace</p>
      </WorkspaceGate>
    </ApiRuntimeProvider>,
  );
  return { server, calls, posts: () => calls.filter((c) => c.method === "POST") };
}

/** Ready on screen and in the probe, whose effect runs after the render that shows it. */
async function ready() {
  await waitFor(() => expect(screen.getByTestId("status").textContent).toBe("ready"));
  await waitFor(() => expect(seen.runtime?.ws.status).toBe("ready"));
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
    expect(current().view).toBeNull();
    await ready();
    const fixture = buildWorkspace("approvals");
    expect(current().source).toBe("api");
    expect(view().agents.map((a) => a.agent_id)).toEqual(fixture.agents.map((a) => a.agent_id));
    expect(view().approvals.map((a) => a.approval_id).sort()).toEqual(fixture.approvals.map((a) => a.approval_id.replace(/^apr_/, "")).sort());
    expect(current().ws.agents).toEqual([]);
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
    const agent = view().agents[0].agent_id;
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
      current().send("kill", view().agents[0].agent_id);
      current().send("stop_all", null);
    });
    await waitFor(() => expect(posts()).toHaveLength(2));
    const bodies = posts().map((p) => ({ path: new URL(p.url).pathname, body: JSON.parse(p.body ?? "{}") as { scope: unknown; environment_shown: unknown } }));
    expect(bodies).toEqual([
      { path: `/v1/workspaces/${WS}/kill-switch`, body: expect.objectContaining({ scope: { kind: "agent", id: view().agents[0].agent_id }, environment_shown: ws.environment }) },
      { path: `/v1/workspaces/${WS}/kill-switch`, body: expect.objectContaining({ scope: { kind: "connection", id: ws.connection.connection_id }, environment_shown: ws.environment }) },
    ]);
  });

  it("pauses every agent not stopped when the owner pauses all, one call each", async () => {
    const { posts } = mount("normal");
    await ready();
    const live = view().agents.filter((a) => a.mode !== "stopped").map((a) => a.agent_id);
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
    const approval = view().approvals.find((a) => a.state === "pending");
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
    const approval = view().approvals.find((a) => a.state === "pending");
    if (!approval) throw new Error("an open request");
    act(() => current().respond(approval.approval_id, "approve", { screen: "D6", environment: "paper", shown: ["Buy 1 XYZ"], modelOutputExpanded: false }));
    await waitFor(() => expect(current().responses[approval.approval_id]?.phase).toBe("undelivered"));
    expect(current().responses[approval.approval_id]?.reason).toBe("step_up_required");
    expect(view().approvals.find((a) => a.approval_id === approval.approval_id)?.state).toBe("pending");
  });

  it("retries an unanswered command with the same key and body, records it once, and calls the result unknown", async () => {
    const { posts, server } = mount("result-unknown");
    await ready();
    let id = "";
    act(() => {
      id = current().send("pause", view().agents[0].agent_id).id;
    });
    await waitFor(() => expect(current().commands.find((c) => c.id === id)?.phase).toBe("unknown"));
    expect(posts().length).toBeGreaterThan(1);
    expect(new Set(posts().map((p) => p.headers.get("Idempotency-Key"))).size).toBe(1);
    expect(new Set(posts().map((p) => p.body)).size).toBe(1);
    expect(server.recorded()).toHaveLength(1);
  });

  it("sends a connect's start again under one key and one body while its answer is lost, and the server starts it once (DEC-2623 item 8)", async () => {
    let lost = 2;
    const { calls, server } = mount("approvals", (fetch) => async (input, init) => {
      const answer = await fetch(input, init);
      if (init.method === "POST" && input.endsWith(CONNECT_START_ROUTE) && lost > 0) {
        lost -= 1;
        throw new TypeError("the answer was lost");
      }
      return answer;
    });
    await ready();
    const connect = current().connect;
    if (!connect) throw new Error("no connect port");
    const asked = await connect.challenge();
    if (!asked.ok) throw new Error(asked.reason);
    const started = await connect.start(asked.shown.challenge);
    expect(started.ok).toBe(true);
    const starts = calls.filter((c) => c.method === "POST" && c.url.endsWith(CONNECT_START_ROUTE));
    expect(starts).toHaveLength(3);
    expect(new Set(starts.map((c) => c.headers.get("Idempotency-Key"))).size).toBe(1);
    expect(new Set(starts.map((c) => c.body)).size).toBe(1);
    expect(server.started()).toHaveLength(1);
  });

  it("stops sending a connect's start once an answer says what happened, even one marked retryable", async () => {
    let starts = 0;
    mount("approvals", (fetch) => async (input, init) => {
      if (init.method === "POST" && input.endsWith(CONNECT_START_ROUTE)) {
        starts += 1;
        return new Response(JSON.stringify({ code: "journal_unavailable", effect: "none", retryable: true }), { status: 503, headers: { "Content-Type": "application/problem+json" } });
      }
      return fetch(input, init);
    });
    await ready();
    const connect = current().connect;
    if (!connect) throw new Error("no connect port");
    const asked = await connect.challenge();
    if (!asked.ok) throw new Error(asked.reason);
    expect(await connect.start(asked.shown.challenge)).toEqual({ ok: false, reason: "journal_unavailable", effect: "none" });
    expect(starts).toBe(1);
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
    expect(current().view).toBeNull();
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
    expect(current().view).toBeNull();
  });

  it("creates the version from its document, and takes the server's version and the served classification; a deploy before the confirm sends nothing", async () => {
    const { posts } = mount();
    await ready();
    const agent = buildWorkspace("approvals").agents[0];
    let id = "";
    act(() => {
      id = current().createVersion({ mandate: agent.mandate, provenance: agent.provenance }).id;
    });
    expect(current().setUps.at(-1)).toMatchObject({ id, agentId: "", created: { phase: "sent", validated: false, classification: null } });
    await waitFor(() => expect(current().setUps.at(-1)?.created).toMatchObject({ phase: "recorded", validated: true, classification: "risk_increasing" }));
    expect(posts().map((p) => new URL(p.url).pathname)).toEqual([`/v1/workspaces/${WS}/mandate-versions`]);
    expect(JSON.parse(posts()[0].body ?? "{}")).toEqual(agent.mandate);
    act(() => current().deployVersion(id, DEPLOY_RECORD));
    await waitFor(() => expect(current().setUps.at(-1)?.deployed).toMatchObject({ phase: "undelivered", reason: "not_confirmed" }));
    expect(current().setUps.at(-1)?.confirmed).toBeNull();
    expect(posts()).toHaveLength(1);
    expect(current().backtestFor(agent.mandate_version)).toBeNull();
  });

  /** Creates a version over the mock and waits for it; returns the set-up's id and a reader of it. */
  async function created() {
    const agent = buildWorkspace("approvals").agents[0];
    let id = "";
    act(() => {
      id = current().createVersion({ mandate: agent.mandate, provenance: agent.provenance }).id;
    });
    const setUp = () => current().setUps.find((s) => s.id === id)!;
    await waitFor(() => expect(setUp().created.phase).toBe("recorded"));
    return { id, setUp };
  }

  it("confirms bound to the served screen, under a passkey that signed the confirm_version challenge for §5.1's digest", async () => {
    const authenticator = mockAuthenticator(MOCK_PASSKEY_ID);
    const { posts, server } = mount("approvals", undefined, undefined, { authenticator: authenticator.get });
    await ready();
    const { id, setUp } = await created();
    expect(server.servedDigests()).toHaveLength(1);
    act(() => current().confirmVersion(id, CONFIRM_RECORD, null));
    expect(setUp().confirmed).toMatchObject({ phase: "sent", record: CONFIRM_RECORD, stepUp: null });
    const digest = confirmStepUpDigest(setUp().version, null, null);
    await waitFor(() => expect(setUp().confirmed).toMatchObject({ phase: "recorded", classification: "risk_increasing", applies: "now", stepUp: digest }));
    const [challenge] = server.challenges();
    expect(challenge).toMatchObject({ kind: "confirm_version", digest, used: true });
    expect(authenticator.calls).toHaveLength(1);
    const confirm = posts().find((p) => p.url.endsWith("/confirm"));
    expect(new URL(confirm?.url ?? "").pathname).toBe(`/v1/workspaces/${WS}/mandate-versions/${encodeURIComponent(setUp().version)}/confirm`);
    const body = JSON.parse(confirm?.body ?? "{}");
    expect(body).toMatchObject({ classification_shown: "risk_increasing", agent_id: null, base_version: null, step_up: { assertion_id: challenge.challenge_id, authenticated_at: challenge.issued_at, method: "passkey" } });
    expect(Object.keys(body.step_up_assertion).sort()).toEqual(["authenticator_data", "client_data_json", "passkey_id", "signature"]);
    expect(body.step_up_assertion.passkey_id).toBe(MOCK_PASSKEY_ID);
    const [atCreate, atConfirm] = server.servedDigests();
    expect(atConfirm).not.toBe(atCreate);
    expect(body.screen_digest).toBe(atConfirm);
  });

  it("sends no confirm when the passkey is not given, and says why: no WebAuthn, no passkey, or canceled", async () => {
    const dismissed: Authenticator = async () => {
      throw new DOMException("not allowed", "NotAllowedError");
    };
    for (const [authenticator, reason] of [
      [null, "passkey_unavailable"],
      [dismissed, "passkey_canceled"],
    ] as const) {
      const { posts, server } = mount("approvals", undefined, undefined, { authenticator });
      await ready();
      const { id, setUp } = await created();
      act(() => current().confirmVersion(id, CONFIRM_RECORD, null));
      await waitFor(() => expect(setUp().confirmed).toMatchObject({ phase: "undelivered", reason, stepUp: null }));
      expect(posts().filter((p) => p.url.endsWith("/confirm"))).toHaveLength(0);
      expect(server.recorded().filter((c) => c.path.endsWith("/confirm"))).toHaveLength(0);
      cleanup();
    }
  });

  it("deploys the confirmed version to paper under a passkey bound to {agent_id, mandate_version}, and keeps the agent's id across a retry", async () => {
    let refused: Headers | null = null;
    const { posts, server } = mount("approvals", (send) => async (input, init) => {
      if (refused === null && init.method === "POST" && new URL(input).pathname === `/v1/workspaces/${WS}/agents`) {
        refused = new Headers(init.headers);
        return new Response(JSON.stringify({ type: "about:blank", status: 503, title: "x", code: "journal_unavailable", effect: "none", event_id: null, retryable: false }), { status: 503, headers: { "Content-Type": "application/problem+json" } });
      }
      return send(input, init);
    });
    await ready();
    const { id, setUp } = await created();
    act(() => current().confirmVersion(id, CONFIRM_RECORD, null));
    await waitFor(() => expect(setUp().confirmed?.phase).toBe("recorded"));
    act(() => current().deployVersion(id, DEPLOY_RECORD));
    await waitFor(() => expect(setUp().deployed).toMatchObject({ phase: "undelivered", reason: "journal_unavailable" }));
    const agentId = setUp().agentId;
    expect(agentId).toMatch(AGENT_ID);
    act(() => current().deployVersion(id, DEPLOY_RECORD));
    await waitFor(() => expect(setUp().deployed?.phase).toBe("recorded"));
    expect(setUp().agentId).toBe(agentId);
    expect(server.deployed()).toEqual([agentId]);
    const deploys = posts().filter((p) => new URL(p.url).pathname === `/v1/workspaces/${WS}/agents`);
    expect(deploys).toHaveLength(1);
    const body = JSON.parse(deploys[0].body ?? "{}");
    expect(Object.keys(body).sort()).toEqual(["agent_id", "backtest_id", "mandate_version", "record", "step_up", "step_up_assertion"]);
    expect(body).toMatchObject({ agent_id: agentId, mandate_version: setUp().version, backtest_id: null, step_up: { method: "passkey" } });
    const deployChallenges = server.challenges().filter((c) => c.kind === "deploy");
    expect(deployChallenges).toHaveLength(2);
    expect(deployChallenges.every((c) => c.digest === deployStepUpDigest(agentId, setUp().version))).toBe(true);
    expect(body.step_up.assertion_id).toBe(deployChallenges[1].challenge_id);
    expect((refused as Headers | null)?.get("Idempotency-Key")).not.toBe(deploys[0].headers.get("Idempotency-Key"));
  });

  it("refuses a confirm before its version is recorded, and a set-up it never made, sending nothing", async () => {
    const { posts } = mount("unreachable");
    await waitFor(() => expect(screen.getByTestId("status").textContent).toBe("unreachable"));
    const agent = buildWorkspace("approvals").agents[0];
    let id = "";
    act(() => {
      id = current().createVersion({ mandate: agent.mandate, provenance: agent.provenance }).id;
    });
    const setUp = () => current().setUps.find((s) => s.id === id)!;
    await waitFor(() => expect(setUp().created.phase).not.toBe("sent"), { timeout: 20_000 });
    expect(setUp().created.phase).not.toBe("recorded");
    act(() => current().confirmVersion(id, CONFIRM_RECORD, null));
    await waitFor(() => expect(setUp().confirmed).toMatchObject({ phase: "undelivered", reason: "not_created", stepUp: null }));
    act(() => current().confirmVersion("set-unknown", CONFIRM_RECORD, null));
    expect(current().setUps.some((s) => s.id === "set-unknown")).toBe(false);
    expect(posts().filter((p) => p.url.includes("/confirm"))).toHaveLength(0);
  });

  it("writes nothing to browser storage", async () => {
    const local = vi.spyOn(Storage.prototype, "setItem");
    mount();
    await ready();
    act(() => {
      current().send("pause", view().agents[0].agent_id);
    });
    await waitFor(() => expect(current().commands.at(-1)?.phase).toBe("recorded"));
    expect(local).not.toHaveBeenCalled();
  });

  it("opens the API session before the first read, and reads through its guard", async () => {
    const { session, asked } = fakeSession();
    const { calls } = mount("approvals", undefined, session);
    await ready();
    expect(asked[0]).toBe("open");
    expect(asked.filter((a) => a === "guard").length).toBeGreaterThanOrEqual(calls.filter((c) => c.method === "GET").length);
  });

  it("opens the session again once when a read answers 401, then reads again", async () => {
    const { session, asked } = fakeSession();
    let refusals = 1;
    const wrap = (inner: Fetch): Fetch => (input, init) =>
      (init.method ?? "GET") === "GET" && refusals > 0 && input.endsWith("/connections")
        ? ((refusals -= 1), Promise.resolve(new Response(JSON.stringify({ type: "about:blank", status: 401, title: "Not authenticated", code: "unauthenticated", effect: "none", retryable: false }), { status: 401 })))
        : inner(input, init);
    mount("approvals", wrap, session);
    await ready();
    expect(asked.filter((a) => a === "open")).toHaveLength(2);
  });

  it("says sign-in is needed, with no figure, when there is no session to open", async () => {
    const { session } = fakeSession(["signed_out"]);
    const { calls } = mount("approvals", undefined, session);
    await waitFor(() => expect(screen.getByTestId("status").textContent).toBe("signed_out"));
    expect(calls.filter((c) => c.method === "GET")).toHaveLength(0);
    expect(current().ws.agents).toEqual([]);
    expect(current().view).toBeNull();
    expect(screen.queryByTestId("gated")).toBeNull();
    expect(screen.getByRole("link", { name: /sign in/i }).getAttribute("href")).toMatch(/^\/login/);
  });

  it("shows an unreadable membership as its own state, not the deployment unreachable, and still sends Stop", async () => {
    const wrap = (inner: Fetch): Fetch => (input, init) =>
      (init.method ?? "GET") === "GET"
        ? Promise.resolve(new Response(JSON.stringify({ type: "about:blank", status: 503, title: "Membership unavailable", code: "membership_unavailable", effect: "none", retryable: true }), { status: 503 }))
        : inner(input, init);
    const { posts } = mount("approvals", wrap);
    await waitFor(() => expect(screen.getByTestId("status").textContent).toBe("membership_unavailable"));
    expect(current().ws.agents).toEqual([]);
    expect(current().view).toBeNull();
    expect(screen.queryByTestId("gated")).toBeNull();
    expect(screen.getByRole("heading", { name: "Your membership could not be read" })).toBeTruthy();
    expect(current().ws.environment, "nothing served named the environment, so it is not paper").toBe("unknown");
    act(() => {
      current().send("stop_all", null);
    });
    await waitFor(() => expect(posts()).toHaveLength(1));
    expect(new URL(posts()[0].url).pathname).toBe(`/v1/workspaces/${WS}/kill-switch`);
    expect(JSON.parse(posts()[0].body ?? "{}").environment_shown, "Stop says it showed no environment").toBeNull();
  });

  it("tries to open the session again on the next read after a refusal or no answer, and shows the workspace once it opens", async () => {
    const { session, asked } = fakeSession(["unreachable", "refused", "open"]);
    mount("approvals", undefined, session, { refreshMs: 20 });
    await waitFor(() => expect(asked.filter((a) => a === "open")).toHaveLength(3));
    await ready();
  });

  it("keeps the session alive after a refresh that got no answer, and tries again on the next beat", async () => {
    const { session, asked } = fakeSession(["open"], ["unreachable", "unreachable", "open"]);
    mount("approvals", undefined, session, { keepAliveMs: 15 });
    await ready();
    await waitFor(() => expect(asked.filter((a) => a === "refresh").length).toBeGreaterThanOrEqual(3));
    expect(current().ws.status).toBe("ready");
  });
});
