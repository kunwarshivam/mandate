import type { AgentMode, ApprovalStatus, CancelReason, OrderState, Provenance, Purpose, RiskFigure } from "@/fixtures/types";

export const MODE_LABEL: Record<AgentMode, string> = {
  normal: "Normal",
  exits_only: "Exits only",
  paused: "Paused",
  stopped: "Stopped",
};

export const MODE_MEANING: Record<AgentMode, string> = {
  normal: "Trades within its mandate.",
  exits_only: "Sells and protects only; no openings or increases.",
  paused: "No new orders; resting protection stays in place.",
  stopped: "Ended. Places no orders.",
};

/** Mandate spec §2.1. A dashed badge marks what the platform, not the owner, authored. */
export const PROVENANCE_LABEL: Record<Provenance, string> = {
  user_stated: "You said",
  user_entered: "You entered",
  template_structure: "From template",
  platform_proposed: "Proposed by the platform",
  platform_default: "Platform default",
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
