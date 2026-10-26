import { Toasty } from "@cloudflare/kumo/components/toast";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { type Hold, acknowledgeStepUpDigest } from "@/api/acknowledgments";
import { type Fetch } from "@/api/client";
import { MOCK_PASSKEY_ID, createMockServer } from "@/api/mock-server";
import { buildWorkspace } from "@/fixtures/workspace";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { type Role, RoleProvider } from "@/lib/roles";
import { mockAuthenticator } from "@/test/set-up";
import { AgentHolds } from "./holds";

/**
 * The owner's acknowledgment of a hold from the agent's screen (workspace API spec §3.11 item 12,
 * DEC-2144), against the contract-faithful mock: each standing hold in words, an acknowledgment sent
 * only with passkey step-up bound to the control stream's head, and announced as lifted only once the
 * executor's lift is journaled (`applied`), never on `recorded`. Every refusal is in words.
 */
const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";
const AGENT = buildWorkspace("normal").agents[0].agent_id;
const EVENT = "01JBH3BTZ8K2M4N6P8R0S2T4V6";

const HOLD: Hold = {
  event: EVENT,
  subject: "reconciliation",
  agent_id: AGENT,
  account: `acct:${WS}:acc_paper_01`,
  reason: { restriction: "reconciliation:incomplete", kind: "incomplete", instrument: null },
  mode: "exits_only",
  raised_at: "2026-10-26T14:30:00.000000000Z",
  independent_required: null,
  request: null,
};

interface Mounted {
  posts: Array<{ path: string; body: Record<string, unknown> }>;
  challenges: Array<Record<string, unknown>>;
}

function mount({ holds = [HOLD], role = "owner", wrap = (send) => send }: { holds?: Hold[]; role?: Role; wrap?: (send: Fetch) => Fetch } = {}): Mounted {
  const server = createMockServer({ workspaceId: WS, scenario: "normal", holds });
  const posts: Mounted["posts"] = [];
  const challenges: Mounted["challenges"] = [];
  const fetch: Fetch = (input, init) => {
    if (init.method === "POST") {
      const body = JSON.parse(String(init.body)) as Record<string, unknown>;
      if (new URL(input).pathname.endsWith("/step-up/challenges")) challenges.push(body);
      else posts.push({ path: new URL(input).pathname, body });
    }
    return server.fetch(input, init);
  };
  render(
    <Toasty>
      <RoleProvider initial={role}>
        <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={wrap(fetch)} refreshMs={60_000} statusPollMs={5} retryMs={5} authenticator={mockAuthenticator(MOCK_PASSKEY_ID).get}>
          <main>
            <AgentHolds agentId={AGENT} />
          </main>
        </ApiRuntimeProvider>
      </RoleProvider>
    </Toasty>,
  );
  return { posts, challenges };
}

const region = () => screen.findByRole("region", { name: "Holds to acknowledge" });

afterEach(() => cleanup());

describe("holds to acknowledge on the agent's screen (§3.11 item 12)", () => {
  it("shows each standing hold in words, what it blocks, and an Acknowledge action; nothing when none stands", async () => {
    mount();
    const held = await region();
    expect(held).toHaveTextContent("A reconciliation run or a broker answer could not be recorded whole.");
    expect(held).toHaveTextContent("It holds new openings; exits and protective orders still run.");
    expect(within(held).getByRole("button", { name: "Acknowledge" })).toBeVisible();
    expect(held).not.toHaveTextContent("reconciliation:incomplete");
    cleanup();
    mount({ holds: [] });
    await act(async () => {});
    await waitFor(() => expect(screen.queryByRole("region", { name: "Holds to acknowledge" })).toBeNull());
  });

  it("sends the acknowledgment with passkey step-up bound to the hold, its subject and the control stream's head, and announces the lift only once the executor applied it", async () => {
    const { posts, challenges } = mount();
    fireEvent.click(within(await region()).getByRole("button", { name: "Acknowledge" }));
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0].path).toBe(`/v1/workspaces/${WS}/agents/${AGENT}/acknowledgments`);
    expect(posts[0].body).toMatchObject({ acknowledged: EVENT, subject: "reconciliation", request: null, record: null, step_up: { method: "passkey" } });
    expect(challenges).toEqual([{ action: { kind: "acknowledge", digest: acknowledgeStepUpDigest("acknowledge", AGENT, 0, EVENT, "reconciliation", null) } }]);
    expect(await screen.findByText("Lifted: the executor journaled the lift. The agent goes back to what its other holds allow.")).toBeVisible();
    expect(await screen.findByText("Hold lifted")).toBeVisible();
  });

  it("raises no toast while the acknowledgment is only recorded or taken, which lifts nothing yet", async () => {
    const pending: (send: Fetch) => Fetch = (send) => (input, init) =>
      /\/commands\/[0-9A-Z]{26}$/.test(new URL(input).pathname) ? Promise.resolve(new Response(JSON.stringify({ api_version: "1", build: `sha256:${"0".repeat(64)}`, served_at: "2026-10-26T14:31:00.000000000Z", as_of: [], phase: "taken", steps: [] }), { status: 200, headers: { "Content-Type": "application/json" } })) : send(input, init);
    mount({ wrap: pending });
    fireEvent.click(within(await region()).getByRole("button", { name: "Acknowledge" }));
    expect(await screen.findByText(/Recorded in the journal\. The hold lifts only once the executor accepts it/)).toBeVisible();
    expect(screen.queryByText("Hold lifted")).toBeNull();
  });

  it("says in words when the executor refuses the copy, names why, and announces nothing", async () => {
    const refused: (send: Fetch) => Fetch = (send) => (input, init) =>
      /\/commands\/[0-9A-Z]{26}$/.test(new URL(input).pathname)
        ? Promise.resolve(new Response(JSON.stringify({ api_version: "1", build: `sha256:${"0".repeat(64)}`, served_at: "2026-10-26T14:31:00.000000000Z", as_of: [], phase: "refused", steps: [{ stream_id: `acct:${WS}:acc_paper_01`, seq: 9, event_type: "OwnerCommandRefused", recorded_at: "2026-10-26T14:31:00.000000000Z", reason: "not_independent" }] }), { status: 200, headers: { "Content-Type": "application/json" } }))
        : send(input, init);
    mount({ wrap: refused });
    fireEvent.click(within(await region()).getByRole("button", { name: "Acknowledge" }));
    expect(await screen.findByText(/Refused by the executor: your workspace requires a second member to acknowledge it\. The hold stands\./)).toBeVisible();
    expect(screen.queryByText("Hold lifted")).toBeNull();
  });

  it("says in words when the hold no longer stands, and records nothing", async () => {
    const lifted: (send: Fetch) => Fetch = (send) => (input, init) =>
      init.method === "POST" && new URL(input).pathname.endsWith("/acknowledgments")
        ? Promise.resolve(new Response(JSON.stringify({ type: "https://mandate.dev/problems/acknowledgment_unavailable", title: "Nothing to acknowledge", status: 409, code: "acknowledgment_unavailable", effect: "none", retryable: false }), { status: 409, headers: { "Content-Type": "application/problem+json" } }))
        : send(input, init);
    mount({ wrap: lifted });
    fireEvent.click(within(await region()).getByRole("button", { name: "Acknowledge" }));
    expect(await screen.findByText(/Not recorded: this hold no longer stands/)).toBeVisible();
    expect(screen.queryByText("Hold lifted")).toBeNull();
  });

  it("sends nothing when the control stream moves while the passkey is asked, and says to try again", async () => {
    let asked = false;
    const moving: (send: Fetch) => Fetch = (send) => async (input, init) => {
      const answer = await send(input, init);
      if (init.method === "POST" && new URL(input).pathname.endsWith("/step-up/challenges")) asked = true;
      if ((init.method ?? "GET") !== "GET" || new URL(input).pathname !== `/v1/workspaces/${WS}/agents/${AGENT}` || !asked) return answer;
      const body = (await answer.json()) as { as_of: Array<{ stream_id: string; seq: number }> };
      body.as_of = body.as_of.map((w) => (w.stream_id === `ctl:${WS}` ? { ...w, seq: w.seq + 1 } : w));
      return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
    };
    const { posts } = mount({ wrap: moving });
    fireEvent.click(within(await region()).getByRole("button", { name: "Acknowledge" }));
    expect(await screen.findByText(/Not sent: something was recorded for this workspace while your passkey was asked/)).toBeVisible();
    expect(posts).toHaveLength(0);
  });

  it("sends nothing when the agent's read names no control-stream position to bind the passkey to", async () => {
    const blind: (send: Fetch) => Fetch = (send) => async (input, init) => {
      const answer = await send(input, init);
      if ((init.method ?? "GET") !== "GET" || new URL(input).pathname !== `/v1/workspaces/${WS}/agents/${AGENT}`) return answer;
      const body = (await answer.json()) as { as_of: Array<{ stream_id: string }> };
      body.as_of = body.as_of.filter((w) => w.stream_id !== `ctl:${WS}`);
      return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
    };
    const { posts, challenges } = mount({ wrap: blind });
    fireEvent.click(within(await region()).getByRole("button", { name: "Acknowledge" }));
    expect(await screen.findByText(/Not sent: your deployment did not report its journal's position/)).toBeVisible();
    expect(posts).toHaveLength(0);
    expect(challenges).toHaveLength(0);
  });

  it("offers no Acknowledge to a role that may not acknowledge, and says so", async () => {
    mount({ role: "approver" });
    const held = await region();
    expect(within(held).queryByRole("button", { name: "Acknowledge" })).toBeNull();
    expect(held).toHaveTextContent("Only the workspace owner or an operator can acknowledge a hold.");
  });

  it("says another member must acknowledge where the policy requires it, and offers no Acknowledge", async () => {
    mount({ holds: [{ ...HOLD, independent_required: true }] });
    const held = await region();
    expect(within(held).queryByRole("button", { name: "Acknowledge" })).toBeNull();
    expect(held).toHaveTextContent("A second member must acknowledge this hold, or one has been asked to");
  });

  it("says the holds are not reported when the read fails, and never shows none", async () => {
    const down: (send: Fetch) => Fetch = (send) => (input, init) => (new URL(input).pathname.endsWith("/acknowledgments") && (init.method ?? "GET") === "GET" ? Promise.reject(new TypeError("down")) : send(input, init));
    mount({ wrap: down });
    expect(await screen.findByText("Holds on this agent are not reported. If one stands, it is shown here once your deployment answers.")).toBeVisible();
  });
});
