import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { type Hold, acknowledgeStepUpDigest, acknowledgmentsRead, holdWords, prepareAcknowledge } from "./acknowledgments";
import { createWorkspaceClient } from "./client";
import type { PasskeyStepUp } from "./passkey";

/**
 * The owner's acknowledgment of a hold over the API (workspace API spec §3.11 item 12, DEC-2144):
 * the step-up digest the server binds, reproduced from the spec's worked vectors; the list of holds
 * read strictly, every member required; and the command body the request schema allows, with the
 * passkey evidence and nothing else.
 */
const AGENT = "agt_01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const EVENT = "01JBH3BTZ8K2M4N6P8R0S2T4V6";
const REQUEST = "01JBH3BTZ8K2M4N6P8R0S2T4V7";

const HOLD: Hold = {
  event: EVENT,
  subject: "reconciliation",
  agent_id: AGENT,
  account: "acct:ws_01J9ZQ4M0000000000000000AB:acc_paper_01",
  reason: { restriction: "reconciliation:incomplete", kind: "incomplete", instrument: null },
  mode: "exits_only",
  raised_at: "2026-10-26T14:30:00.000000000Z",
  independent_required: null,
  request: null,
};

const STEP_UP: PasskeyStepUp = {
  step_up: { assertion_id: "01JC0000000000000000000001", authenticated_at: "2026-10-26T14:31:00.000000000Z", method: "passkey" },
  step_up_assertion: { passkey_id: "yMnKy8zNzs_Q0dLT1NXW1w", authenticator_data: "AAAA", client_data_json: "e30", signature: "AAAA" },
};

describe("the acknowledgment's step-up digest (§3.11 item 12)", () => {
  it("reproduces the spec's three worked vectors", () => {
    expect(acknowledgeStepUpDigest("acknowledge", AGENT, 42, EVENT, "reconciliation", null)).toBe("sha256:5f5506a3ac32cae637a2cc222393f852035c699430656870ba024702431260a7");
    expect(acknowledgeStepUpDigest("acknowledge", AGENT, 42, EVENT, "reconciliation", REQUEST)).toBe("sha256:b9367c62d1201059c5a9f2cf0bf35e637d27a5934e174b779276a2fcbc7af765");
    expect(acknowledgeStepUpDigest("request_ack", AGENT, 42, EVENT, "reconciliation", null)).toBe("sha256:b15c59e4e09394ddf0f5430c04569276c27c7a957ac68c683e53b15e6b393ebc");
  });

  it("binds the control stream's head, the hold and its subject, so evidence for one never counts for another", () => {
    const base = acknowledgeStepUpDigest("acknowledge", AGENT, 42, EVENT, "reconciliation", null);
    expect(acknowledgeStepUpDigest("acknowledge", AGENT, 43, EVENT, "reconciliation", null)).not.toBe(base);
    expect(acknowledgeStepUpDigest("acknowledge", AGENT, 42, REQUEST, "reconciliation", null)).not.toBe(base);
    expect(acknowledgeStepUpDigest("acknowledge", AGENT, 42, EVENT, "limit", null)).not.toBe(base);
  });
});

describe("the holds to acknowledge (`read-models/acknowledgments`)", () => {
  const served = (holds: unknown[]) => ({ acknowledgments: holds });

  it("reads each subject's hold with its reason, and its open request", () => {
    const limit = { ...HOLD, subject: "limit", reason: { limit: "max_daily_loss" }, mode: null, independent_required: true, request: { event_id: REQUEST, requested_by: "usr_01" } };
    const restriction = { ...HOLD, subject: "restriction", reason: { cause: "pattern_day_trader" }, mode: "exits_only" };
    const read = acknowledgmentsRead(served([HOLD, limit, restriction]), "");
    expect(read.ok && read.value.acknowledgments.map((h) => h.subject)).toEqual(["reconciliation", "limit", "restriction"]);
    expect(read.ok && read.value.acknowledgments[1].request).toEqual({ event_id: REQUEST, requested_by: "usr_01" });
  });

  it("refuses a hold missing a member, a subject the enum does not list, or a reconciliation kind it does not know", () => {
    const missing: Partial<Hold> = { ...HOLD };
    delete missing.request;
    expect(acknowledgmentsRead(served([missing]), "").ok).toBe(false);
    expect(acknowledgmentsRead(served([{ ...HOLD, subject: "tripwire" }]), "").ok).toBe(false);
    expect(acknowledgmentsRead(served([{ ...HOLD, reason: { ...HOLD.reason, kind: "mystery" } }]), "").ok).toBe(false);
    expect(acknowledgmentsRead(served([{ ...HOLD, mode: "stopped" }]), "").ok).toBe(false);
  });
});

/** The schema's own examples (`schemas/workspace-api/examples/read-models.acknowledgments*.json`), which the schema checks hold to the JSON Schema: an oracle written against the schema, not against this client. */
const EXAMPLES = join(__dirname, "..", "..", "..", "schemas", "workspace-api", "examples");
const example = (name: string): unknown => JSON.parse(readFileSync(join(EXAMPLES, name), "utf8"));

function withSet(base: unknown, set: Record<string, unknown>): unknown {
  const doc = structuredClone(base) as Record<string, unknown>;
  for (const [pointer, value] of Object.entries(set)) {
    const path = pointer.split("/").slice(1);
    let node = doc;
    for (const token of path.slice(0, -1)) node = node[token] as Record<string, unknown>;
    node[path[path.length - 1]] = structuredClone(value);
  }
  return doc;
}

describe("the holds read against the schema's examples", () => {
  it("decodes the base example and every valid variant, so a body the server may serve is never read as not reported", () => {
    const base = example("read-models.acknowledgments.json");
    expect(acknowledgmentsRead(base, "").ok).toBe(true);
    for (const variant of example("read-models.acknowledgments.valid.json") as Array<{ label: string; set?: Record<string, unknown> }>) {
      expect(acknowledgmentsRead(withSet(base, variant.set ?? {}), "").ok, variant.label).toBe(true);
    }
  });
});

describe("the hold in words", () => {
  it("says what each reconciliation kind holds and what it blocks, never a raw code", () => {
    const words = (reason: Hold["reason"], mode: Hold["mode"] = "exits_only") => holdWords({ ...HOLD, reason, mode });
    expect(words({ restriction: "reconciliation:incomplete", kind: "incomplete", instrument: null }).what).toMatch(/could not be recorded whole/);
    expect(words({ restriction: "reconciliation:cash", kind: "cash", instrument: null }).what).toMatch(/cash/);
    expect(words({ restriction: "reconciliation:external_activity", kind: "external_activity", instrument: null }).what).toMatch(/outside the platform/);
    expect(words({ restriction: "reconciliation:SPY", kind: "instrument", instrument: "SPY" }).what).toMatch(/SPY/);
    expect(words({ restriction: "reconciliation:x", kind: "incomplete", instrument: null }).blocks).toMatch(/new openings/);
    expect(words({ restriction: "reconciliation:x", kind: "incomplete", instrument: null }, "paused").blocks).toMatch(/paused/);
  });

  it("names a limit or a restriction it does not know generically, not by its code", () => {
    const limit = holdWords({ ...HOLD, subject: "limit", reason: { limit: "some_new_limit" }, mode: null });
    expect(limit.what).not.toContain("some_new_limit");
    const restriction = holdWords({ ...HOLD, subject: "restriction", reason: { cause: "some_new_cause" } });
    expect(restriction.what).not.toContain("some_new_cause");
  });
});

describe("the acknowledgment's command (`commands/acknowledge-request`)", () => {
  const client = createWorkspaceClient({ workspaceId: "ws_01J9ZQ4M0000000000000000AB", fetch: () => Promise.reject(new Error("not sent")), baseUrl: "https://api.owlhead.ai" });

  it("posts to the hold's own agent the hold, its subject, no request, and the passkey evidence with its assertion", () => {
    const prepared = prepareAcknowledge(client, HOLD, STEP_UP);
    expect(prepared.path).toBe(`/v1/workspaces/ws_01J9ZQ4M0000000000000000AB/agents/${AGENT}/acknowledgments`);
    expect(JSON.parse(prepared.body)).toEqual({ acknowledged: EVENT, subject: "reconciliation", request: null, record: null, step_up: STEP_UP.step_up, step_up_assertion: STEP_UP.step_up_assertion });
  });
});
