import type { AgentMode, ApprovalStatus, AutonomyRule, CancelReason, ChangeClass, OrderState, Provenance, Purpose, RiskFigure } from "@/fixtures/types";
import { usd } from "./format";

/** A mode is named by what the agent may do (DEC-512), so "Trading" needs no "Selling only" beside it to be understood. */
export const MODE_LABEL: Record<AgentMode, string> = {
  normal: "Trading",
  exits_only: "Selling only",
  paused: "Paused",
  stopped: "Stopped",
};

export const MODE_MEANING: Record<AgentMode, string> = {
  normal: "Trades within its mandate.",
  exits_only: "Sells and protects only; no openings or increases.",
  paused: "No new orders; resting protection stays in place.",
  stopped: "Ended. Places no orders.",
};

/** Mandate spec §2.1, in short words (DEC-513). A dashed badge marks what the platform, not the owner, authored. */
export const PROVENANCE_LABEL: Record<Provenance, string> = {
  user_stated: "You said",
  user_entered: "You entered",
  template_structure: "Template",
  platform_proposed: "Proposed",
  platform_default: "Default",
};

export function isPlatformAuthored(p: Provenance): boolean {
  return p === "platform_proposed" || p === "platform_default";
}

export const PURPOSE_LABEL: Record<Purpose, string> = {
  open: "Open",
  increase: "Increase",
  discretionary_exit: "Signal exit",
  owner_exit: "Your exit",
  risk_exit: "Risk exit",
  protective: "Protection",
};

/** Trading-domain spec §5.7 states in owner words. `Unknown` stays "unknown", never a guess. */
export const ORDER_STATE_LABEL: Record<OrderState, string> = {
  Intent: "Recorded, not sent",
  Submitting: "Sending",
  Accepted: "Resting",
  PartiallyFilled: "Partly filled",
  PendingCancel: "Canceling",
  PendingReplace: "Replacing",
  Filled: "Filled",
  Canceled: "Canceled",
  Rejected: "Rejected by the broker",
  Expired: "Expired",
  Replaced: "Replaced",
  Abandoned: "Not sent",
  Unknown: "Unknown",
};

export const APPROVAL_STATUS_LABEL: Record<ApprovalStatus, string> = {
  delivered: "Waiting for you",
  acted: "Approved and submitted",
  gate_skipped: "Approved, skipped by the gate",
  rejected: "Skipped by you",
  expired: "Skipped at the deadline",
  superseded: "Canceled",
};

export const CANCEL_REASON_LABEL: Record<CancelReason, string> = {
  version_applied: "a new mandate version applied",
  mode_tightened: "the agent's mode changed",
  owner_pause: "you paused the agent",
  owner_stop: "you stopped the agent",
  kill_switch: "a kill switch was used",
};

export const RISK_FIGURE_LABEL: Record<RiskFigure["field"], string> = {
  order_usd: "This order",
  position_usd_after: "Position after it fills",
  gross_usd_after: "Total holdings after it fills",
  bought_today_usd: "Bought today, including this order",
};

export const RISK_CAP_LABEL: Record<RiskFigure["field"], string> = {
  order_usd: "order limit",
  position_usd_after: "position limit",
  gross_usd_after: "total holdings limit",
  bought_today_usd: "",
};

/** Mandate spec §9.2, in the brief's words (A6). */
export const CHANGE_CLASS_LABEL: Record<ChangeClass, string> = {
  risk_increasing: "Risk-increasing",
  risk_reducing: "Risk-reducing",
  neutral: "Neutral",
};

/** What an autonomy rule has the agent do, as the rule's sentence begins. */
const RULE_THEN: Record<AutonomyRule["then"], string> = {
  ask: "ask",
  auto: "submit without asking",
  deny: "never allow it",
};

/** A rule's field, named as the request names it. Any other field is named by its words. */
const RULE_FIELD: Record<string, string> = {
  order_usd: "the order value",
  combined_score: "the combined model score",
  purpose: "the purpose",
};

/** One value compared: the words and the value for exactly one value, `null` for any other count. */
const one = (words: string) => (vs: string[]) => (vs.length === 1 ? `${words} ${vs[0]}` : null);

/**
 * A rule's comparison and its values as the sentence reads them, `null` when the values do not fit
 * the comparison: none, or several for a comparison of one value.
 */
const RULE_OP: Record<string, (values: string[]) => string | null> = {
  eq: one("is"),
  ne: one("is not"),
  gt: one("is above"),
  gte: one("is at or above"),
  lt: one("is below"),
  lte: one("is at or below"),
  in: (vs) => (vs.length === 0 ? null : `is ${vs.join(" or ")}`),
  not_in: (vs) => {
    if (vs.length === 0) return null;
    if (vs.length === 1) return `is not ${vs[0]}`;
    if (vs.length === 2) return `is neither ${vs[0]} nor ${vs[1]}`;
    return `is none of ${vs.slice(0, -1).join(", ")} or ${vs.at(-1)}`;
  },
};

/** What a rule says when its operator is one this page does not know, or its values do not fit it: never the raw operator. */
const RULE_CONDITION_UNREAD = "meets this rule's condition";

/** A rule's value as the owner wrote it: dollars for a dollar field, whole dollars without cents; a purpose by its label. */
function ruleValue(field: string, value: string): string {
  if (field.endsWith("_usd")) return usd(value).replace(/\.00$/, "");
  if (field === "purpose" && Object.hasOwn(PURPOSE_LABEL, value)) return PURPOSE_LABEL[value as Purpose].toLowerCase();
  return value;
}

/**
 * The owner's own autonomy rule as its sentence, "ask when the combined model score is below 0.65",
 * wherever the owner reads the rule; its id stays in the record and the audit (critique C-6). It
 * says only what the owner's rule says, never what the owner should do.
 */
export function ruleSentence(rule: AutonomyRule): string {
  const { field, op, value } = rule.when;
  const values = (Array.isArray(value) ? value : [value]).map((v) => ruleValue(field, v));
  const compare = (Object.hasOwn(RULE_OP, op) ? RULE_OP[op](values) : null) ?? RULE_CONDITION_UNREAD;
  return `${RULE_THEN[rule.then]} when ${RULE_FIELD[field] ?? `the ${field.replaceAll("_", " ")}`} ${compare}`;
}

/**
 * A recorded line with each of the agent's rules named by its sentence, not its id: "your rule
 * “low_score”" reads "your rule: ask when the combined model score is below 0.65" (critique C-6).
 */
export function withRuleSentences(text: string, rules: readonly AutonomyRule[]): string {
  return text.replace(/rule “([^”]+)”/g, (named, id: string) => {
    const rule = rules.find((r) => r.id === id);
    return rule ? `rule: ${ruleSentence(rule)}` : named;
  });
}
