/**
 * The checks an approval's dry run deferred to the executor's binding gate (DEC-2515 item 5,
 * mandate spec §6.4, journal spec §9.6 version 3): `ApprovalRequested` version 3 names them in its
 * content's `deferred_to_binding`, and the approver is shown, exactly, the sentence below, then
 * each check by name. A deferred check is never shown as passed (DEC-2378 item 11 condition 4).
 */

/** The fixed sentence, as the journal vectors' `approval_deferrals.approver_sentence` carries it. */
export const APPROVER_SENTENCE = "These checks have not been run yet. They run when the order is placed, and may still refuse it:";

/**
 * How each name reads to the approver: what the check is (DEC-2388 item 8, DEC-2378 item 11), never
 * whether it passed. Every name the
 * journal spec allows, and no other; a name the spec adds later is shown as itself until it has one.
 */
export const DEFERRED_CHECK_LABELS: Readonly<Record<string, string>> = {
  account_exposure: "Account exposure limits",
  agent_equity: "Position cap scaled by the agent's equity",
  agent_mode: "The agent's and the account's trading mode",
  conduct_history: "Market-conduct checks on today's orders",
  day_trade_budget: "Day-trade budget",
  eligibility: "Instrument eligibility",
  fee_reservation: "Fee reservation",
  halt: "Trading halt",
  liquidity_floor: "Liquidity floor",
  other_orders: "Checks over every working order on the account",
  participation: "Participation cap",
  quote_ask: "Price collar from the current ask",
  reentry_cooldown: "Re-entry cooldown",
  related_accounts: "Related accounts' resting orders",
};

/**
 * What a request's content says was deferred: nothing (versions 1 and 2 have no member), the names
 * in the order the request gives them, or a member that cannot be read. An unreadable member is
 * never read as nothing deferred: the screen then says it cannot tell, and offers no approve. The
 * read names the request's version (DEC-2518): version 3 always names its deferred checks and
 * versions 1 and 2 name none, so a version 3 request without its list, or an older one with a
 * list, is unreadable too.
 */
export type DeferredChecks = { kind: "none" } | { kind: "named"; names: string[] } | { kind: "unreadable" };

const NAME = /^[a-z][a-z0-9_]*$/;

export function deferredChecks(content: Record<string, unknown>, schemaVersion: number): DeferredChecks {
  const named = Object.hasOwn(content, "deferred_to_binding");
  if (schemaVersion !== 3) return named || (schemaVersion !== 1 && schemaVersion !== 2) ? { kind: "unreadable" } : { kind: "none" };
  if (!named) return { kind: "unreadable" };
  const value = content.deferred_to_binding;
  if (!Array.isArray(value) || value.length === 0) return { kind: "unreadable" };
  const names: string[] = [];
  for (const name of value) {
    if (typeof name !== "string" || !NAME.test(name) || names.includes(name)) return { kind: "unreadable" };
    names.push(name);
  }
  return { kind: "named", names };
}

/** A name as the approver reads it. */
export function deferredCheckLabel(name: string): string {
  return Object.hasOwn(DEFERRED_CHECK_LABELS, name) ? DEFERRED_CHECK_LABELS[name] : name;
}
