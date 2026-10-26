import { createHash } from "node:crypto";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ApiAgent } from "./api-screens";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import { buildWorkspace } from "@/fixtures/workspace";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { RoleProvider } from "@/lib/roles";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";
const AGENT = buildWorkspace("normal").agents[0].agent_id;
const STREAM = `agent:${WS}:${AGENT}`;
const XYZ = "7b4a1c2e-1111-4a2b-9c3d-000000000001";
const ABC = "7b4a1c2e-2222-4a2b-9c3d-000000000002";

type Told = { type: string; at: string; payload: Record<string, unknown>; tamper?: boolean };
const sha = (text: string) => createHash("sha256").update(text, "utf8").digest("hex");
const thesisPayload = (over: Record<string, unknown>) => ({ model_id: "research.equity", model_version: "1.0.0", lineage_id: "L1", revision: 0, predecessor_thesis_id: null, instrument_id: XYZ, asset_class: "us_equity", direction: "long", as_of: "2026-10-26T09:00:00.000000000Z", expires_at: "2099-01-01T00:00:00.000000000Z", horizon_s: 86400, admitted: true, reason: null, ...over });

/** The journal page §4.8.1 serves for the agent's stream, with `told` as its records, each body hashed. */
function page(told: Told[], after: number, limit: number) {
  const events = told.map((e, i) => {
    const body = JSON.stringify({ event_type: e.type, payload: e.payload });
    return { seq: i + 1, event_id: `E${i + 1}`, event_type: e.type, recorded_at: e.at, prev_hash: "0".repeat(64), hash: e.tamper ? "f".repeat(64) : sha(body), body: Buffer.from(body).toString("base64") };
  });
  const served = events.filter((e) => e.seq > after).slice(0, limit);
  return { api_version: "1", build: `sha256:${"0".repeat(64)}`, served_at: "2026-10-26T15:00:00.000000000Z", as_of: [], stream_id: STREAM, head: { seq: events.length, hash: "0".repeat(64), recorded_at: "2026-10-26T15:00:00.000000000Z" }, events: served, next_after_seq: after + served.length, at_head: true };
}

function mount(told: Told[] | "forbidden") {
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const fetch: Fetch = (input, init) => {
    const url = new URL(input);
    if (url.pathname !== `/v1/workspaces/${WS}/journal/streams/${encodeURIComponent(STREAM)}/events` && url.pathname !== `/v1/workspaces/${WS}/journal/streams/${STREAM}/events`) return server.fetch(input, init);
    if (told === "forbidden") return Promise.resolve(new Response(JSON.stringify({ type: "about:blank", status: 403, title: "Forbidden", code: "forbidden", effect: "none", event_id: null, retryable: false }), { status: 403, headers: { "Content-Type": "application/problem+json" } }));
    const body = page(told, Number(url.searchParams.get("after_seq")), Number(url.searchParams.get("limit")));
    return Promise.resolve(new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } }));
  };
  render(
    <RoleProvider>
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        <ApiAgent agentId={AGENT} />
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
}

afterEach(() => cleanup());

describe("a research agent's theses on its page, from its stream's records (journal spec §9.4, §9.22)", () => {
  it("lists each thesis newest first with the admission's verdict in words: current, rejected with why, and invalidated", async () => {
    mount([
      { type: "AgentDeployed", at: "2026-10-26T08:00:00.000000000Z", payload: {} },
      { type: "ThesisProposed", at: "2026-10-26T09:00:00.000000000Z", payload: thesisPayload({ thesis_id: "T1", lineage_id: "L1" }) },
      { type: "ThesisProposed", at: "2026-10-26T10:00:00.000000000Z", payload: thesisPayload({ thesis_id: "T2", lineage_id: "L2", instrument_id: ABC, admitted: false, reason: "no_corroboration" }) },
      { type: "ThesisProposed", at: "2026-10-26T11:00:00.000000000Z", payload: thesisPayload({ thesis_id: "T3", lineage_id: "L3", instrument_id: ABC }) },
      { type: "ThesisInvalidated", at: "2026-10-26T12:00:00.000000000Z", payload: { thesis_id: "T3", lineage_id: "L3", instrument_id: ABC, cause: "source_revoked", candidate: null, evidence: [], source: "src_1" } },
    ]);
    const list = await screen.findByRole("list", { name: "Theses" });
    expect(screen.getByRole("heading", { name: "Theses" })).toBeInTheDocument();
    const items = within(list).getAllByRole("listitem");
    expect(items.map((li) => li.getAttribute("data-state"))).toEqual(["invalidated", "rejected", "current"]);
    expect(items[0]).toHaveTextContent("Invalidated 2026-10-26 12:00:00 UTC: a source it cites was revoked.");
    expect(items[1]).toHaveTextContent("Rejected: no independent source or market data corroborated it.");
    expect(items[2]).toHaveTextContent("Admitted. Current until 2099-01-01 00:00:00 UTC.");
    expect(items[2]).toHaveTextContent(XYZ);
    expect(list.textContent).not.toMatch(/no_corroboration|source_revoked/);
  });

  it("shows nothing from a record whose body does not match its hash, and says how many", async () => {
    mount([{ type: "ThesisProposed", at: "2026-10-26T09:00:00.000000000Z", payload: thesisPayload({ thesis_id: "T1" }), tamper: true }]);
    await waitFor(() => expect(document.querySelector("[data-slot=theses-unverified]")).toHaveTextContent("1 thesis record could not be checked against its hash and is not shown."));
    expect(screen.queryByRole("list", { name: "Theses" })).toBeNull();
  });

  it("shows no theses section for an agent whose records hold none, or for a role the journal refuses", async () => {
    mount([{ type: "AgentDeployed", at: "2026-10-26T08:00:00.000000000Z", payload: {} }]);
    await screen.findByRole("list", { name: "Journal" }, { timeout: 5000 });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(screen.queryByRole("heading", { name: "Theses" })).toBeNull();
    cleanup();
    mount("forbidden");
    await waitFor(() => expect(screen.getByText("Not available to your role.")).toBeInTheDocument());
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(screen.queryByRole("heading", { name: "Theses" })).toBeNull();
    expect(screen.queryByText("Theses not reported.")).toBeNull();
  });

  it("says when it read only the stream's latest records, so an older thesis is not silently missing", async () => {
    mount(Array.from({ length: 1001 }, (_, i) => ({ type: "DecisionMade", at: "2026-10-26T09:00:00.000000000Z", payload: { n: i } })));
    await waitFor(() => expect(document.querySelector("[data-slot=theses-window]")).toHaveTextContent("Read from the latest 1,000 records on the stream; a thesis recorded before them is not shown."), { timeout: 5000 });
  });
});
