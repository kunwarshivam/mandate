import { describe, expect, it } from "vitest";
import { type Fetch } from "./client";
import { MY_WORKSPACES_ROUTE, pickWorkspace, resolveWorkspace, myWorkspaces } from "./memberships";

const answer = (status: number, body: unknown): Fetch => async () => new Response(JSON.stringify(body), { status });
const member = (id: string) => ({ workspace_id: id, label: null, roles: ["workspace_admin"] });

describe("the workspace from one's memberships (§4.10, DEC-2113)", () => {
  it("names the one workspace the session belongs to, reading the route with the session's cookie", async () => {
    const calls: Array<{ url: string; init: RequestInit }> = [];
    const fetch: Fetch = async (url, init) => {
      calls.push({ url, init });
      return new Response(JSON.stringify({ api_version: "v1", as_of: [], workspaces: [member("01KDEM0000000000000000000W")] }), { status: 200 });
    };
    expect(await resolveWorkspace(fetch, "https://api-staging.owlhead.ai")).toEqual({ ok: true, workspaceId: "01KDEM0000000000000000000W" });
    expect(calls).toEqual([{ url: `https://api-staging.owlhead.ai${MY_WORKSPACES_ROUTE}`, init: expect.objectContaining({ method: "GET", credentials: "include" }) }]);
  });

  it("names none for no membership or for several, and never guesses among them", () => {
    expect(pickWorkspace(myWorkspaces({ workspaces: [] }, ""))).toEqual({ ok: false, reason: "none" });
    expect(pickWorkspace(myWorkspaces({ workspaces: [member("ws_a"), member("ws_b")] }, ""))).toEqual({ ok: false, reason: "several" });
    expect(pickWorkspace(myWorkspaces({ workspaces: [member("ws_a"), { ...member("ws_a"), roles: ["viewer"] }] }, ""))).toEqual({ ok: true, workspaceId: "ws_a" });
  });

  it("reads a 401 as signed out, and any other failure or unreadable answer as unavailable", async () => {
    expect(await resolveWorkspace(answer(401, { code: "unauthenticated" }), "")).toEqual({ ok: false, reason: "signed_out" });
    expect(await resolveWorkspace(answer(503, { code: "membership_unavailable" }), "")).toEqual({ ok: false, reason: "unavailable" });
    expect(await resolveWorkspace(answer(200, { workspaces: [{ workspace_id: "../x", label: null, roles: [] }] }), "")).toEqual({ ok: false, reason: "unavailable" });
    expect(await resolveWorkspace(answer(200, { workspaces: [{ ...member("ws_a"), roles: ["owner"] }] }), "")).toEqual({ ok: false, reason: "unavailable" });
    const lost: Fetch = async () => {
      throw new TypeError("Failed to fetch");
    };
    expect(await resolveWorkspace(lost, "")).toEqual({ ok: false, reason: "unavailable" });
  });
});
