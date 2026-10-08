import { describe, expect, it } from "vitest";
import type { Watermark } from "./types";
import {
  dataSourceFrom,
  freshnessView,
  membershipsNotServedYet,
  pnlLabel,
  resolveWorkspace,
  shouldRefetch,
  streamAgeSeconds,
  unreachableWorkspace,
  type SourceEnv,
  type WorkspaceResolver,
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

describe("which workspace is read: the principal's memberships at run time (identity spec §3.2, §9.2)", () => {
  const api = { kind: "api", baseUrl: "https://ws.example" } as const;
  const members = (...ids: string[]): WorkspaceResolver => async () => ({ workspace_ids: ids });

  it.skip("pending E11-9: fixtures need no workspace, and the resolver is never asked", async () => {
    let asked = 0;
    const resolver: WorkspaceResolver = async () => {
      asked += 1;
      return { workspace_ids: ["ws_1"] };
    };
    expect(await resolveWorkspace({ kind: "fixtures" }, resolver, null)).toEqual({ kind: "fixtures" });
    expect((await resolveWorkspace({ kind: "unconfigured", reason: "no API configured" }, resolver, null)).kind).toBe("unconfigured");
    expect(asked).toBe(0);
  });

  it.skip("pending E11-9: one membership is the workspace read", async () => {
    expect(await resolveWorkspace(api, members("ws_1"), null)).toEqual({ kind: "workspace", baseUrl: "https://ws.example", workspaceId: "ws_1" });
  });

  it.skip("pending E11-9: until the identity service serves memberships, the API source is unconfigured", async () => {
    expect((await resolveWorkspace(api, membershipsNotServedYet, null)).kind).toBe("unconfigured");
    expect((await resolveWorkspace(api, async () => null, null)).kind).toBe("unconfigured");
  });

  it.skip("pending E11-9: a resolver that fails is unconfigured, never fixtures", async () => {
    const failing: WorkspaceResolver = async () => {
      throw new TypeError("Failed to fetch");
    };
    expect((await resolveWorkspace(api, failing, null)).kind).toBe("unconfigured");
  });

  it.skip("pending E11-9: a chosen workspace is read only when the principal is a member of it", async () => {
    expect(await resolveWorkspace(api, members("ws_1", "ws_2"), "ws_2")).toEqual({ kind: "workspace", baseUrl: "https://ws.example", workspaceId: "ws_2" });
    expect((await resolveWorkspace(api, members("ws_1"), "ws_other")).kind).toBe("unconfigured");
  });

  it.skip("pending E11-9: several memberships and none chosen asks the owner to choose", async () => {
    expect(await resolveWorkspace(api, members("ws_1", "ws_2"), null)).toEqual({ kind: "choose", baseUrl: "https://ws.example", workspaceIds: ["ws_1", "ws_2"] });
  });

  it.skip("pending E11-9: no membership, or an id that is not a path segment, is unconfigured", async () => {
    expect((await resolveWorkspace(api, members(), null)).kind).toBe("unconfigured");
    expect((await resolveWorkspace(api, members("../admin"), null)).kind).toBe("unconfigured");
    expect((await resolveWorkspace(api, members("ws_1", "ws 2"), "ws 2")).kind).toBe("unconfigured");
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
