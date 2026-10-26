/**
 * Gate decisions from the journal (journal spec §9.1, §9.5): every `DecisionMade` on an agent's
 * stream, with what the runtime proposed, its dry run's answer and reason, what autonomy chose and
 * who asked, and for a version 3 the checks it left to the executor's binding gate. Where the
 * decision proposed an intent, its `IntentProposed` names it as `causation_id`, and the account
 * stream's `GateDecided` names that intent: the binding gate's verdict and the checks it ran, which
 * the dry run never stands in for. Only records whose body matched its hash are read; a decision
 * that does not verify or decode is counted, never shown.
 */
import type { JournalRecord } from "./records";

export type DryRun = "allow" | "deny" | "defer";

/** Journal spec §9.1's `deferred_to_binding` vocabulary (DEC-2378 item 11, DEC-2388 item 8). */
export const DEFERRED_CHECKS = ["account_exposure", "agent_equity", "agent_mode", "conduct_history", "day_trade_budget", "eligibility", "fee_reservation", "halt", "liquidity_floor", "other_orders", "participation", "quote_ask", "reentry_cooldown", "related_accounts"] as const;
export type Autonomy = "auto" | "ask" | "deny";

export interface BindingVerdict {
  verdict: "allow" | "deny" | "hold" | "defer";
  reason_code: string | null;
  checks: Array<{ id: string; result: string }>;
  recorded_at: string;
}

export interface Decision {
  event_id: string;
  recorded_at: string;
  instrument_id: string;
  side: "buy" | "sell";
  qty: string;
  limit_price: string | null;
  purpose: string;
  dry_run: DryRun;
  reason_code: string | null;
  autonomy: Autonomy | null;
  requested_by: "agent" | "owner" | "client";
  decided_by: string | null;
  /** Why an `ask` was classified but not asked (mandate spec §6.4), or `null`. */
  ask_suppressed: "budget" | "skipped_today" | "recent_timeout" | null;
  /** A version 3's checks left to the binding gate; empty for any other. */
  deferred: string[];
  /** The intent it proposed (`IntentProposed`'s event id), or `null` when the stream read holds none. */
  intent: string | null;
  /** The binding gate's verdict on that intent, or `null` when the account stream read holds none. */
  binding: BindingVerdict | null;
}

export interface DecisionsView {
  /** Newest first. */
  decisions: Decision[];
  /** `DecisionMade` records not read: a body that did not match its hash, or a payload that does not decode. */
  unreadable: number;
}

const text = (v: unknown): v is string => typeof v === "string" && v.length > 0;
const nullableText = (v: unknown): v is string | null => v === null || text(v);
const oneOf = <T extends string>(v: unknown, allowed: readonly T[]): v is T => typeof v === "string" && (allowed as readonly string[]).includes(v);

function decisionOf(r: JournalRecord): Omit<Decision, "intent" | "binding"> | null {
  const p = r.payload;
  if (!r.matches || !p) return null;
  const { instrument_id, side, qty, limit_price, purpose, dry_run, reason_code, autonomy, requested_by, decided_by, ask_suppressed } = p;
  if (!text(instrument_id) || !oneOf(side, ["buy", "sell"] as const) || !text(qty) || !nullableText(limit_price) || !text(purpose)) return null;
  if (!oneOf(dry_run, ["allow", "deny", "defer"] as const) || !nullableText(reason_code) || !(autonomy === null || oneOf(autonomy, ["auto", "ask", "deny"] as const))) return null;
  if (!oneOf(requested_by, ["agent", "owner", "client"] as const) || !nullableText(decided_by)) return null;
  if (!(ask_suppressed === null || ask_suppressed === undefined || oneOf(ask_suppressed, ["budget", "skipped_today", "recent_timeout"] as const))) return null;
  const deferred = p.deferred_to_binding;
  if (deferred !== undefined && !(Array.isArray(deferred) && deferred.length > 0 && deferred.every((d) => oneOf(d, DEFERRED_CHECKS)))) return null;
  return { event_id: r.event_id, recorded_at: r.recorded_at, instrument_id, side, qty, limit_price, purpose, dry_run, reason_code, autonomy, requested_by, decided_by, ask_suppressed: ask_suppressed ?? null, deferred: (deferred as string[] | undefined) ?? [] };
}

function verdictOf(r: JournalRecord): [string, BindingVerdict] | null {
  const p = r.payload;
  if (!r.matches || !p) return null;
  const { intent_id, verdict, reason_code, checks } = p;
  if (!text(intent_id) || !oneOf(verdict, ["allow", "deny", "hold", "defer"] as const) || !nullableText(reason_code) || !Array.isArray(checks)) return null;
  const read: BindingVerdict["checks"] = [];
  for (const c of checks) {
    if (typeof c !== "object" || c === null || !text((c as Record<string, unknown>).id) || !text((c as Record<string, unknown>).result)) return null;
    read.push({ id: (c as Record<string, string>).id, result: (c as Record<string, string>).result });
  }
  return [intent_id, { verdict, reason_code, checks: read, recorded_at: r.recorded_at }];
}

/** The decisions `agent` holds, each joined to its intent there and its verdict in `account` (`null`: not read), oldest record first in. */
export function decisionsOf(agent: readonly JournalRecord[], account: readonly JournalRecord[] | null): DecisionsView {
  const intents = new Map<string, string>();
  for (const r of agent) if (r.event_type === "IntentProposed" && r.matches && r.causation_id) intents.set(r.causation_id, r.event_id);
  const verdicts = new Map<string, BindingVerdict>();
  for (const r of account ?? []) {
    if (r.event_type !== "GateDecided") continue;
    const read = verdictOf(r);
    if (read) verdicts.set(read[0], read[1]);
  }
  let unreadable = 0;
  const decisions: Decision[] = [];
  for (const r of agent) {
    if (r.event_type !== "DecisionMade") continue;
    const d = decisionOf(r);
    if (!d) {
      unreadable += 1;
      continue;
    }
    const intent = intents.get(d.event_id) ?? null;
    decisions.push({ ...d, intent, binding: intent ? (verdicts.get(intent) ?? null) : null });
  }
  return { decisions: decisions.reverse(), unreadable };
}
