import { describe, expect, it } from "vitest";
import { decisionsOf } from "./decisions";
import type { JournalRecord } from "./records";

let seq = 0;
const at = (m: number) => `2026-10-26T10:${String(m).padStart(2, "0")}:00.000000000Z`;
function rec(event_type: string, payload: Record<string, unknown> | null, recorded: string, causation_id: string | null = null, matches = true): JournalRecord {
  seq += 1;
  return { seq, event_id: `E${seq}`, event_type, recorded_at: recorded, payload, matches, causation_id };
}
/** A `DecisionMade` as journal spec §9.1 closes it, at version 1 unless `deferred` is given. */
const decided = (over: Record<string, unknown>, recorded = at(1)) =>
  rec("DecisionMade", { instrument_id: "XYZ", side: "buy", type: "limit", tif: "day", qty: "3", limit_price: "101.5", purpose: "open", exit_origin: null, dry_run: "allow", reason_code: null, autonomy: "auto", ask_suppressed: null, decided_by: "default", delegation_id: null, requested_by: "agent", client_id: null, ...over }, recorded);
const proposed = (decision: JournalRecord, recorded = at(2)) => rec("IntentProposed", { instrument_id: "XYZ", side: "buy", type: "limit", tif: "day", qty: "3", limit_price: "101.5", purpose: "open" }, recorded, decision.event_id);
const gated = (intent: JournalRecord, over: Record<string, unknown> = {}, recorded = at(3)) => rec("GateDecided", { intent_id: intent.event_id, verdict: "allow", reason_code: null, data_profile: "p", quotes_used: [], marks_used: [], checks: [{ id: "mandate_version", result: "pass", inputs: {}, computed: {} }, { id: "max_order_usd", result: "pass", inputs: {}, computed: {} }], risk_clock: recorded, ...over }, recorded);

describe("gate decisions from the journal (journal spec §9.1, §9.5)", () => {
  it("joins an allowed, automatic decision to its intent and the binding gate's verdict and checks", () => {
    const d = decided({});
    const i = proposed(d);
    const g = gated(i);
    const view = decisionsOf([d, i], [g]);
    expect(view.unreadable).toBe(0);
    expect(view.decisions).toHaveLength(1);
    expect(view.decisions[0]).toMatchObject({ event_id: d.event_id, dry_run: "allow", autonomy: "auto", intent: i.event_id, binding: { verdict: "allow", reason_code: null, checks: [{ id: "mandate_version", result: "pass" }, { id: "max_order_usd", result: "pass" }] } });
  });

  it("names the binding gate's denial and the check that failed, which the dry run did not see", () => {
    const d = decided({ deferred_to_binding: ["liquidity_floor", "participation"] });
    const i = proposed(d);
    const g = gated(i, { verdict: "deny", reason_code: "liquidity_floor", checks: [{ id: "liquidity_floor", result: "fail", inputs: {}, computed: {} }] });
    const [one] = decisionsOf([d, i], [g]).decisions;
    expect(one).toMatchObject({ deferred: ["liquidity_floor", "participation"], binding: { verdict: "deny", reason_code: "liquidity_floor" } });
  });

  it("joins no verdict to a decision whose intent the account stream does not hold, or that proposed none", () => {
    const sent = decided({});
    const intent = proposed(sent);
    const asked = decided({ autonomy: "ask" }, at(4));
    const denied = decided({ dry_run: "deny", reason_code: "max_order_usd", autonomy: null }, at(5));
    const view = decisionsOf([sent, intent, asked, denied], []);
    expect(view.decisions.map((x) => [x.event_id, x.intent, x.binding])).toEqual([
      [denied.event_id, null, null],
      [asked.event_id, null, null],
      [sent.event_id, intent.event_id, null],
    ]);
  });

  it("never joins a verdict for another intent, or an intent that follows another decision", () => {
    const d = decided({});
    const other = decided({}, at(2));
    const i = proposed(other);
    const g = gated(i, { verdict: "deny", reason_code: "halt" });
    const view = decisionsOf([d, other, i], [g]);
    expect(view.decisions.find((x) => x.event_id === d.event_id)).toMatchObject({ intent: null, binding: null });
    expect(view.decisions.find((x) => x.event_id === other.event_id)).toMatchObject({ binding: { verdict: "deny" } });
  });

  it("lists newest first, and counts a decision record that did not verify or does not decode, never reading it", () => {
    const old = decided({}, at(1));
    const tampered = { ...decided({}, at(2)), matches: false, payload: null };
    const malformed = decided({ dry_run: "maybe" }, at(3));
    const fresh = decided({}, at(4));
    const view = decisionsOf([old, tampered, malformed, fresh], null);
    expect(view.decisions.map((x) => x.event_id)).toEqual([fresh.event_id, old.event_id]);
    expect(view.unreadable).toBe(2);
  });

  it("refuses a deferred check off journal spec §9.1's vocabulary, an empty list, or an unknown ask suppression", () => {
    for (const over of [{ deferred_to_binding: ["vibes"] }, { deferred_to_binding: [] }, { autonomy: "ask", ask_suppressed: "tired" }]) {
      expect(decisionsOf([decided(over)], null).unreadable, JSON.stringify(over)).toBe(1);
    }
    expect(decisionsOf([decided({ autonomy: "ask", ask_suppressed: "skipped_today" })], null).decisions[0]).toMatchObject({ ask_suppressed: "skipped_today" });
  });

  it("reads no verdict when the account stream was not read, and an unreadable verdict as none", () => {
    const d = decided({});
    const i = proposed(d);
    expect(decisionsOf([d, i], null).decisions[0]).toMatchObject({ intent: i.event_id, binding: null });
    const broken = { ...gated(i), matches: false, payload: null };
    expect(decisionsOf([d, i], [broken]).decisions[0]).toMatchObject({ binding: null });
  });
});
