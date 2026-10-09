import type { Agent, GateDecision, Mandate, ReasonCode } from "@/fixtures/types";
import { dec, mul } from "./decimal";
import { usd } from "./format";
import { gateRule } from "./gate-reasons";
import { mandateAt } from "./mandate-history";

export type CheckResult = "passed" | "failed" | "waiting" | "not_run";

export interface GateCheck {
  key: string;
  label: string;
  result: CheckResult;
  /** The rule the check enforces, in owner words, with the values of the mandate version the gate decided under. */
  rule: string;
}

interface CheckSpec {
  key: string;
  label: string;
  codes: ReasonCode[];
  rule: (m: Mandate | null, d: GateDecision) => string;
}

/** The gate's checks in the order it runs them; the first that fails ends the run. */
const CHECKS: CheckSpec[] = [
  { key: "account", label: "The account can trade", codes: ["account_restricted", "account_trading_blocked"], rule: () => "The broker lets this account open and close positions." },
  { key: "mode", label: "The agent's mode allows it", codes: ["agent_exits_only", "agent_paused", "agent_stopped"], rule: () => "Normal agents may open, add and exit; exits-only agents may only sell and protect." },
  { key: "universe", label: "The instrument is in the universe", codes: ["not_in_universe"], rule: (_m, d) => `${d.action.symbol} is one of the instruments this mandate may trade.` },
  {
    key: "session",
    label: "The session allows it",
    codes: ["session_not_allowed", "extended_hours_opening_not_allowed", "auction_window", "close_window", "discretionary_exit_regular_session_only"],
    rule: () => "Openings only in the regular session, not in an auction or the last 10 minutes.",
  },
  { key: "halt", label: "The instrument is trading", codes: ["instrument_halted"], rule: () => "Trading in the instrument is not halted." },
  { key: "in-flight", label: "No order in the instrument is unknown", codes: ["unknown_order_in_flight"], rule: () => "Every earlier order in this instrument has a known state." },
  { key: "price", label: "The price is fresh and near the limit", codes: ["stale_mark", "price_outside_collar"], rule: () => "The mark is recent, and the limit is close to it." },
  { key: "cooldown", label: "Re-entry cooldown", codes: ["reentry_cooldown"], rule: (m) => gateRule("reentry_cooldown", m) },
  { key: "order-size", label: "Order size", codes: ["max_order_size"], rule: (m, d) => `This order is ${usd(mul(dec(d.action.qty), dec(d.action.limit_price)))}; ${gateRule("max_order_size", m).toLowerCase()}` },
  { key: "position", label: "Position limit", codes: ["concentration_limit"], rule: (m) => gateRule("concentration_limit", m) },
  { key: "gross", label: "Total holdings limit", codes: ["gross_exposure_limit"], rule: (m) => gateRule("gross_exposure_limit", m) },
  { key: "orders-today", label: "Orders today", codes: ["max_orders_per_day"], rule: (m) => gateRule("max_orders_per_day", m) },
  { key: "buying-power", label: "Buying power", codes: ["insufficient_buying_power"], rule: () => "The account has the buying power for it." },
  { key: "autonomy", label: "Your autonomy rules", codes: ["owner_confirmation_required"], rule: () => "Your rules decide whether it goes without asking, asks you, or is not allowed." },
];

/**
 * Each check the gate ran for a decision, with its result. Checks after a failure did not run. Limits
 * read as the mandate version the gate decided under, without a figure when it cannot be rebuilt.
 */
export function gateChecks(decision: GateDecision, agent: Agent): GateCheck[] {
  const mandate = mandateAt(agent, decision.mandate_version);
  const failing = decision.reason_code ? CHECKS.findIndex((c) => c.codes.includes(decision.reason_code as ReasonCode)) : -1;
  return CHECKS.map((c, i) => {
    const result: CheckResult = failing < 0 || i < failing ? "passed" : i === failing ? (decision.verdict === "defer" ? "waiting" : "failed") : "not_run";
    const rule = result === "failed" || result === "waiting" ? gateRule(decision.reason_code as ReasonCode, mandate) : c.rule(mandate, decision);
    return { key: c.key, label: c.label, result, rule };
  });
}

export const CHECK_RESULT_LABEL: Record<CheckResult, string> = {
  passed: "Passed",
  failed: "Not allowed",
  waiting: "Waiting",
  not_run: "Not run",
};
