import { describe, expect, it } from "vitest";
import type { Watermark } from "./types";
import type { Fetch } from "./client";
import {
  createIdentityApi,
  dataSourceFrom,
  freshnessView,
  identityNotServedYet,
  pnlLabel,
  resolveWorkspace,
  shouldRefetch,
  streamAgeSeconds,
  unreachableWorkspace,
  type IdentityAnswer,
  type IdentityApi,
  type Membership,
  type SessionInfo,
  type SourceEnv,
} from "./workspace-source";

const HASH = `sha256:${"0f".repeat(32)}` as const;
const SERVED = "2026-09-28T18:05:20.000000000Z";
const mark = (stream_id: string, seq: number, recorded_at = SERVED): Watermark => ({ stream_id, seq, hash: HASH, recorded_at });
const AGENT = "agent:ws_1:agt_01JB3K9P2H6SD4F8G1E3W7XYZB";
const ACCOUNT = "acct:ws_1:01JB3K7N4C6D8F0G2H4J6K8M0N";

describe("the data source, chosen at build time (DEC-736)", () => {
  it.skip("pending E11-9: an https API URL chooses the API", () => {
    expect(dataSourceFrom({ NODE_ENV: "production", NEXT_PUBLIC_WORKSPACE_API_URL: "https://ws.example" })).toEqual({ kind: "api", baseUrl: "https://ws.example" });
  });

  it.skip("pending E11-9: a workspace id in the build is ignored, never used to pick a tenant", () => {
    const env = { NODE_ENV: "production", NEXT_PUBLIC_WORKSPACE_API_URL: "https://ws.example", NEXT_PUBLIC_WORKSPACE_ID: "ws_1" } as SourceEnv;
    expect(dataSourceFrom(env)).toEqual({ kind: "api", baseUrl: "https://ws.example" });
  });

  it.skip("pending E11-9: a production build with no API URL is unconfigured, never fixtures", () => {
    const source = dataSourceFrom({ NODE_ENV: "production" });
    expect(source.kind).toBe("unconfigured");
    expect(dataSourceFrom({ NODE_ENV: "production", NEXT_PUBLIC_WORKSPACE_API_URL: "" }).kind).toBe("unconfigured");
    expect(dataSourceFrom({ NODE_ENV: "production", NEXT_PUBLIC_WORKSPACE_API_URL: "   " }).kind).toBe("unconfigured");
  });

  it.skip("pending E11-9: fixtures only when asked for by name", () => {
    expect(dataSourceFrom({ NODE_ENV: "production", NEXT_PUBLIC_WORKSPACE_SOURCE: "fixtures" })).toEqual({ kind: "fixtures" });
    expect(dataSourceFrom({ NODE_ENV: "development" })).toEqual({ kind: "fixtures" });
    expect(dataSourceFrom({ NODE_ENV: "test" })).toEqual({ kind: "fixtures" });
  });

  it.skip("pending E11-9: asking for fixtures and naming an API at once is unconfigured", () => {
    const both = { NEXT_PUBLIC_WORKSPACE_SOURCE: "fixtures", NEXT_PUBLIC_WORKSPACE_API_URL: "https://ws.example" };
    expect(dataSourceFrom({ NODE_ENV: "production", ...both }).kind).toBe("unconfigured");
    expect(dataSourceFrom({ NODE_ENV: "development", ...both }).kind).toBe("unconfigured");
  });

  it.skip("pending E11-9: an API URL that is not https is unconfigured, except plain http to this machine", () => {
    expect(dataSourceFrom({ NODE_ENV: "production", NEXT_PUBLIC_WORKSPACE_API_URL: "http://ws.example" }).kind).toBe("unconfigured");
    expect(dataSourceFrom({ NODE_ENV: "production", NEXT_PUBLIC_WORKSPACE_API_URL: "ftp://ws.example" }).kind).toBe("unconfigured");
    expect(dataSourceFrom({ NODE_ENV: "production", NEXT_PUBLIC_WORKSPACE_API_URL: "not a url" }).kind).toBe("unconfigured");
    expect(dataSourceFrom({ NODE_ENV: "development", NEXT_PUBLIC_WORKSPACE_API_URL: "http://localhost:8080" })).toEqual({ kind: "api", baseUrl: "http://localhost:8080" });
  });

  it.skip("pending E11-9: an unknown source name is unconfigured", () => {
    expect(dataSourceFrom({ NODE_ENV: "development", NEXT_PUBLIC_WORKSPACE_SOURCE: "fixture" }).kind).toBe("unconfigured");
    expect(dataSourceFrom({ NODE_ENV: "production", NEXT_PUBLIC_WORKSPACE_SOURCE: "api" }).kind).toBe("unconfigured");
  });
});

describe("which workspace is read: the session, then its memberships (identity spec §4.5, §6.2, §6.4 route 2)", () => {
  const api = { kind: "api", baseUrl: "https://ws.example" } as const;
  const EXPIRES = "2026-09-28T19:05:20.000000000Z";
  const listed = (...ids: string[]) => ids.map((id, i) => ({ workspace_id: id, label: `Workspace ${i + 1}` }));

  /** An identity API answering from fixed values, counting each route's reads. */
  function identity(session: IdentityAnswer<SessionInfo> | null, memberships: IdentityAnswer<Membership[]> | null = null) {
    const reads = { session: 0, memberships: 0 };
    const value: IdentityApi = {
      session: async () => {
        reads.session += 1;
        return session;
      },
      memberships: async () => {
        reads.memberships += 1;
        return memberships;
      },
    };
    return { value, reads };
  }
  const full = (...ids: string[]) =>
    identity({ ok: true, value: { kind: "full", expires_at: EXPIRES } }, { ok: true, value: listed(...ids).map((w) => ({ ...w, roles: ["operator"] })) });
  const reduction = (...ids: string[]) => identity({ ok: true, value: { kind: "reduction_only", workspaces: listed(...ids), expires_at: EXPIRES } });

  it.skip("pending E11-9: fixtures and an unconfigured source ask the identity routes nothing", async () => {
    const id = full("ws_1");
    expect(await resolveWorkspace({ kind: "fixtures" }, id.value, null)).toEqual({ kind: "fixtures" });
    expect((await resolveWorkspace({ kind: "unconfigured", reason: "no API configured" }, id.value, null)).kind).toBe("unconfigured");
    expect(id.reads).toEqual({ session: 0, memberships: 0 });
  });

  it.skip("pending E11-9: a full session with one membership reads that workspace", async () => {
    const id = full("ws_1");
    expect(await resolveWorkspace(api, id.value, null)).toEqual({ kind: "workspace", baseUrl: "https://ws.example", workspaceId: "ws_1", label: "Workspace 1" });
    expect(id.reads).toEqual({ session: 1, memberships: 1 });
  });

  it.skip("pending E11-9: a full session with several offers them by label, the last-used one as the default", async () => {
    expect(await resolveWorkspace(api, full("ws_1", "ws_2").value, "ws_2")).toEqual({ kind: "choose", baseUrl: "https://ws.example", workspaces: listed("ws_1", "ws_2"), preferred: "ws_2" });
  });

  it.skip("pending E11-9: a last-used workspace that is no longer a membership is ignored", async () => {
    expect(await resolveWorkspace(api, full("ws_1", "ws_2").value, "ws_gone")).toMatchObject({ kind: "choose", preferred: null });
    expect(await resolveWorkspace(api, full("ws_1").value, "ws_gone")).toMatchObject({ kind: "workspace", workspaceId: "ws_1" });
  });

  it.skip("pending E11-9: a reduction-only session uses its own list and never reads the membership index", async () => {
    const one = reduction("ws_1");
    expect(await resolveWorkspace(api, one.value, null)).toEqual({ kind: "workspace", baseUrl: "https://ws.example", workspaceId: "ws_1", label: "Workspace 1" });
    expect(one.reads).toEqual({ session: 1, memberships: 0 });
    const several = reduction("ws_1", "ws_2");
    expect(await resolveWorkspace(api, several.value, "ws_1")).toEqual({ kind: "choose", baseUrl: "https://ws.example", workspaces: listed("ws_1", "ws_2"), preferred: "ws_1" });
    expect(await resolveWorkspace(api, several.value, "ws_3")).toMatchObject({ kind: "choose", preferred: null });
    expect(several.reads.memberships).toBe(0);
  });

  it.skip("pending E11-9: the session read failing is unreachable, never fixtures, and memberships are not read", async () => {
    const id = identity({ ok: false, code: "network", retryable: true });
    expect(await resolveWorkspace(api, id.value, null)).toMatchObject({ kind: "unreachable", retryable: true });
    expect(id.reads.memberships).toBe(0);
  });

  it.skip("pending E11-9: membership_unavailable is unreachable and retryable, never fixtures", async () => {
    const id = identity({ ok: true, value: { kind: "full", expires_at: EXPIRES } }, { ok: false, code: "membership_unavailable", retryable: true });
    expect(await resolveWorkspace(api, id.value, null)).toEqual({ kind: "unreachable", reason: "membership_unavailable", retryable: true });
  });

  it.skip("pending E11-9: until the identity routes are served, the API source is unconfigured", async () => {
    expect((await resolveWorkspace(api, identityNotServedYet, null)).kind).toBe("unconfigured");
    expect((await resolveWorkspace(api, identity(null).value, null)).kind).toBe("unconfigured");
    expect((await resolveWorkspace(api, identity({ ok: true, value: { kind: "full", expires_at: EXPIRES } }, null).value, null)).kind).toBe("unconfigured");
  });

  it.skip("pending E11-9: no workspace, a duplicate, or an id that is not a path segment is unconfigured", async () => {
    expect((await resolveWorkspace(api, full().value, null)).kind).toBe("unconfigured");
    expect((await resolveWorkspace(api, full("ws_1", "ws_1").value, null)).kind).toBe("unconfigured");
    expect((await resolveWorkspace(api, full("../admin").value, null)).kind).toBe("unconfigured");
    expect((await resolveWorkspace(api, reduction().value, null)).kind).toBe("unconfigured");
    expect((await resolveWorkspace(api, reduction("ws 2").value, null)).kind).toBe("unconfigured");
  });
});

describe("the identity routes over HTTP (identity spec §4.5)", () => {
  function serving(answers: Record<string, () => Response>) {
    const calls: Array<{ url: string; credentials: RequestCredentials | undefined }> = [];
    const fetch: Fetch = async (input, init) => {
      calls.push({ url: input, credentials: init.credentials });
      const answer = answers[new URL(input).pathname];
      return answer ? answer() : new Response(JSON.stringify({ code: "not_found", effect: "none" }), { status: 404 });
    };
    return { fetch, calls };
  }
  const json = (status: number, body: unknown) => new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
  const EXPIRES = "2026-09-28T19:05:20.000000000Z";

  it.skip("pending E11-9: reads its own session and memberships at the API origin, with the session cookie and no workspace in the path", async () => {
    const http = serving({
      "/v1/me/session": () => json(200, { kind: "full", expires_at: EXPIRES }),
      "/v1/me/workspaces": () => json(200, [{ workspace_id: "ws_1", label: "Home", roles: ["operator", "approver"] }]),
    });
    const api = createIdentityApi({ fetch: http.fetch, baseUrl: "https://ws.example" });
    expect(await api.session()).toEqual({ ok: true, value: { kind: "full", expires_at: EXPIRES } });
    expect(await api.memberships()).toEqual({ ok: true, value: [{ workspace_id: "ws_1", label: "Home", roles: ["operator", "approver"] }] });
    expect(http.calls).toEqual([
      { url: "https://ws.example/v1/me/session", credentials: "include" },
      { url: "https://ws.example/v1/me/workspaces", credentials: "include" },
    ]);
  });

  it.skip("pending E11-9: a reduction-only session lists the workspaces it covers", async () => {
    const http = serving({ "/v1/me/session": () => json(200, { kind: "reduction_only", workspaces: [{ workspace_id: "ws_1", label: "Home" }], expires_at: EXPIRES }) });
    expect(await createIdentityApi({ fetch: http.fetch, baseUrl: "https://ws.example" }).session()).toEqual({
      ok: true,
      value: { kind: "reduction_only", workspaces: [{ workspace_id: "ws_1", label: "Home" }], expires_at: EXPIRES },
    });
  });

  it.skip("pending E11-9: 503 membership_unavailable is a retryable failure", async () => {
    const http = serving({ "/v1/me/workspaces": () => json(503, { code: "membership_unavailable", effect: "none", retryable: true }) });
    expect(await createIdentityApi({ fetch: http.fetch, baseUrl: "https://ws.example" }).memberships()).toEqual({ ok: false, code: "membership_unavailable", retryable: true });
  });

  it.skip("pending E11-9: roles is a list; a single role, or an unknown session kind, is refused", async () => {
    const http = serving({
      "/v1/me/session": () => json(200, { kind: "partial", expires_at: EXPIRES }),
      "/v1/me/workspaces": () => json(200, [{ workspace_id: "ws_1", label: "Home", role: "operator" }]),
    });
    const api = createIdentityApi({ fetch: http.fetch, baseUrl: "https://ws.example" });
    expect(await api.session()).toMatchObject({ ok: false, retryable: false });
    expect(await api.memberships()).toMatchObject({ ok: false, retryable: false });
  });

  it.skip("pending E11-9: no answer at all is a retryable failure, never an empty list", async () => {
    const fetch: Fetch = async () => {
      throw new TypeError("Failed to fetch");
    };
    const api = createIdentityApi({ fetch, baseUrl: "https://ws.example" });
    expect(await api.session()).toEqual({ ok: false, code: "network", retryable: true });
    expect(await api.memberships()).toEqual({ ok: false, code: "network", retryable: true });
  });
});

describe("unreachable: no cached content (G1, G4, spec §7)", () => {
  it.skip("pending E11-9: shows no agent, approval, decision, timeline, or position, and marks the deployment down", () => {
    const ws = unreachableWorkspace(SERVED, "paper", "2026-09-28T17:58:02.000000000Z");
    expect(ws.status).toBe("unreachable");
    expect(ws.agents).toEqual([]);
    expect(ws.approvals).toEqual([]);
    expect(ws.decisions).toEqual([]);
    expect(ws.timeline).toEqual({});
    expect(ws.external_positions).toEqual([]);
    expect(ws.health.deployment).toEqual({ state: "down", as_of: "2026-09-28T17:58:02.000000000Z" });
  });

  it.skip("pending E11-9: carries no account figure, so no number from an earlier load survives", () => {
    const ws = unreachableWorkspace(SERVED, "paper", null);
    expect(JSON.stringify(ws)).not.toMatch(/"account_equity":"[0-9]/);
    for (const feed of [ws.health.market_data, ws.health.broker, ws.health.deployment, ws.health.relay]) expect(feed.state).not.toBe("ok");
  });
});

describe("stale values (brief §3.1, spec §6.1)", () => {
  it.skip("pending E11-9: a stale value is shown stale with its age to served_at, never as current", () => {
    const view = freshnessView({ observed_at: "2026-09-28T18:02:11.000000000Z", stale: true, age_seconds: 189, limit_source: "data_profile.max_mark_age_s" });
    expect(view).toEqual({ as_of: "2026-09-28T18:02:11.000000000Z", stale: true, age: "3 min" });
  });

  it.skip("pending E11-9: the age is the server's, measured to served_at, not the viewer's clock", () => {
    const view = freshnessView({ observed_at: "2026-09-28T18:05:01.000000000Z", stale: false, age_seconds: 19, limit_source: "account_snapshot.max_age_s" });
    expect(view).toEqual({ as_of: "2026-09-28T18:05:01.000000000Z", stale: false, age: "15 s" });
  });

  it.skip("pending E11-9: the server's stale flag wins even when the age looks small", () => {
    expect(freshnessView({ observed_at: SERVED, stale: true, age_seconds: 0, limit_source: "account_snapshot.max_age_s" }).stale).toBe(true);
  });
});

describe("per-stream as_of (spec §6.1)", () => {
  const asOf = [mark(AGENT, 8813, "2026-09-28T18:05:19.000000000Z"), mark(ACCOUNT, 977, "2026-09-28T18:01:20.000000000Z")];

  it.skip("pending E11-9: one response is current on one stream and behind on another", () => {
    expect(streamAgeSeconds(asOf, AGENT, SERVED)).toBe(1);
    expect(streamAgeSeconds(asOf, ACCOUNT, SERVED)).toBe(240);
  });

  it.skip("pending E11-9: a stream the response did not read has no age, not a current one", () => {
    expect(streamAgeSeconds(asOf, "ctl:ws_1", SERVED)).toBeNull();
  });
});

describe("paper is labelled simulated wherever P&L shows (brief D1)", () => {
  it.skip("pending E11-1: paper P&L is simulated; live carries no such label", () => {
    expect(pnlLabel("paper")).toBe("Simulated");
    expect(pnlLabel("live")).toBeNull();
  });
});

describe("refetch on change (spec §3.1, §6.3)", () => {
  const view = [mark(AGENT, 8813), mark(ACCOUNT, 977)];

  it.skip("pending E11-9: a newer watermark on a stream the view read refetches it", () => {
    expect(shouldRefetch(view, [{ stream_id: AGENT, seq: 8814 }])).toBe(true);
    expect(shouldRefetch(view, [{ stream_id: "ctl:ws_1", seq: 3 }, { stream_id: ACCOUNT, seq: 978 }])).toBe(true);
  });

  it.skip("pending E11-9: nothing refetches when no watermark is newer", () => {
    expect(shouldRefetch(view, [{ stream_id: AGENT, seq: 8813 }])).toBe(false);
    expect(shouldRefetch(view, [{ stream_id: ACCOUNT, seq: 970 }])).toBe(false);
    expect(shouldRefetch(view, [{ stream_id: "ctl:ws_1", seq: 99 }])).toBe(false);
    expect(shouldRefetch(view, [])).toBe(false);
  });
});
