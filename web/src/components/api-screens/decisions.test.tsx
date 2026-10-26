import { createHash } from "node:crypto";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ApiDecisions } from "./decisions";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import { WorkspaceGate } from "@/components/screens/common";
import { buildWorkspace } from "@/fixtures/workspace";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { RoleProvider } from "@/lib/roles";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";
const fixture = buildWorkspace("normal");
const AGENT = fixture.agents[0].agent_id;
const ACCOUNT = `acct:${WS}:${fixture.connection.connection_id}`;

type Told = { type: string; payload: Record<string, unknown>; causation?: string; at?: string; tamper?: boolean };
const sha = (text: string) => createHash("sha256").update(text, "utf8").digest("hex");

/** A §4.8.1 page of `told` for `stream`, each body hashed, served from `after` up to `limit`. */
function page(stream: string, told: Told[], after: number, limit: number) {
  const events = told.map((e, i) => {
    const body = JSON.stringify({ ...(e.causation ? { causation_id: e.causation } : {}), event_id: `${stream}#${i + 1}`, event_type: e.type, payload: e.payload });
    return { seq: i + 1, event_id: `${stream}#${i + 1}`, event_type: e.type, recorded_at: e.at ?? `2026-10-26T10:0${i}:00.000000000Z`, prev_hash: "0".repeat(64), hash: e.tamper ? "f".repeat(64) : sha(body), body: Buffer.from(body).toString("base64") };
  });
  const served = events.filter((e) => e.seq > after).slice(0, limit);
  return { api_version: "1", build: `sha256:${"0".repeat(64)}`, served_at: "2026-10-26T15:00:00.000000000Z", as_of: [], stream_id: stream, head: { seq: events.length, hash: "0".repeat(64), recorded_at: "2026-10-26T15:00:00.000000000Z" }, events: served, next_after_seq: after + served.length, at_head: true };
}

/** Mounts the screen over the mock, `streams` standing in for the journal pages it names, or every page refused. */
function mount(streams: Record<string, Told[]> | "forbidden" | null = null) {
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const fetch: Fetch = (input, init) => {
    const url = new URL(input);
    const match = decodeURIComponent(url.pathname).match(/\/journal\/streams\/(.+)\/events$/);
    if (!match || streams === null) return server.fetch(input, init);
    if (streams === "forbidden") return Promise.resolve(new Response(JSON.stringify({ type: "about:blank", status: 403, title: "Forbidden", code: "forbidden", effect: "none", event_id: null, retryable: false }), { status: 403, headers: { "Content-Type": "application/problem+json" } }));
    const told = streams[match[1]];
    if (!told) return server.fetch(input, init);
    return Promise.resolve(new Response(JSON.stringify(page(match[1], told, Number(url.searchParams.get("after_seq")), Number(url.searchParams.get("limit")))), { status: 200, headers: { "Content-Type": "application/json" } }));
  };
  render(
    <RoleProvider>
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        <WorkspaceGate api>
          <ApiDecisions />
        </WorkspaceGate>
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
}

const decision = (over: Record<string, unknown> = {}) => ({ instrument_id: "XYZ", side: "buy", type: "limit", tif: "day", qty: "3", limit_price: "101.5", purpose: "open", exit_origin: null, dry_run: "allow", reason_code: null, autonomy: "auto", ask_suppressed: null, decided_by: "default", delegation_id: null, requested_by: "agent", client_id: null, ...over });

afterEach(() => cleanup());

describe("gate decisions over the workspace API, from the journal (journal spec §9.1, §9.5)", () => {
  it("lists every agent's decisions as the mock's journal holds them, newest first, never a binding verdict it does not hold", async () => {
    mount();
    const list = await screen.findByRole("list", { name: "Gate decisions" }, { timeout: 5000 });
    const items = within(list).getAllByRole("listitem");
    expect(items).toHaveLength(fixture.decisions.length);
    for (const item of items) expect(item.getAttribute("data-binding")).toBe("none");
    expect(document.querySelector("[data-slot=fixture-tag]")).toBeNull();
  });

  it("joins a decision through its intent to the binding gate's verdict, naming the check that did not pass", async () => {
    const agentStream = `agent:${WS}:${AGENT}`;
    mount({
      [agentStream]: [
        { type: "DecisionMade", payload: decision({ deferred_to_binding: ["liquidity_floor"] }) },
        { type: "IntentProposed", payload: { instrument_id: "XYZ", side: "buy", type: "limit", tif: "day", qty: "3", limit_price: "101.5", purpose: "open" }, causation: `${agentStream}#1` },
      ],
      [ACCOUNT]: [{ type: "GateDecided", payload: { intent_id: `${agentStream}#2`, verdict: "deny", reason_code: "halt", data_profile: "p", quotes_used: [], marks_used: [], checks: [{ id: "mandate_version", result: "pass", inputs: {}, computed: {} }, { id: "liquidity_floor", result: "fail", inputs: {}, computed: {} }], risk_clock: "2026-10-26T10:02:00.000000000Z" } }],
    });
    const item = await waitFor(() => {
      const found = document.querySelector(`[data-agent="${AGENT}"][data-binding=deny]`);
      if (!found) throw new Error("not yet");
      return found;
    }, { timeout: 5000 });
    expect(item).toHaveTextContent("The dry run allowed it; it acted without asking.");
    expect(item).toHaveTextContent("Left to the executor's binding gate, which the dry run could not see: liquidity floor.");
    expect(item).toHaveTextContent("The binding gate denied it");
    expect(item).toHaveTextContent("It ran 2 checks; not passed: liquidity floor (fail).");
  });

  it("says the binding verdict is not found when the intent was sent but the account's records read hold none", async () => {
    const agentStream = `agent:${WS}:${AGENT}`;
    mount({
      [agentStream]: [
        { type: "DecisionMade", payload: decision() },
        { type: "IntentProposed", payload: {}, causation: `${agentStream}#1` },
      ],
      [ACCOUNT]: [],
    });
    await waitFor(() => expect(document.querySelector(`[data-agent="${AGENT}"] [data-slot=binding]`)).toHaveTextContent("Sent to the executor; its binding verdict is not among the account's latest records read."), { timeout: 5000 });
  });

  it("counts a decision record that does not match its hash, and shows nothing from it", async () => {
    mount({ [`agent:${WS}:${AGENT}`]: [{ type: "DecisionMade", payload: decision(), tamper: true }] });
    await waitFor(() => expect(document.querySelector("[data-slot=decisions-unreadable]")).toHaveTextContent("1 decision record could not be checked against its hash or read, and is not shown."), { timeout: 5000 });
    expect(document.querySelector(`[data-agent="${AGENT}"]`)).toBeNull();
  });

  it("says the decisions are not available to a role the journal refuses", async () => {
    mount("forbidden");
    await waitFor(() => expect(screen.getByText("Not available to your role.")).toBeInTheDocument(), { timeout: 5000 });
    expect(screen.queryByRole("list", { name: "Gate decisions" })).toBeNull();
  });
});
