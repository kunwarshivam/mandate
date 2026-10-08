import type { GateDecision, Mandate, Purpose, ReasonCode } from "@/fixtures/types";
import { percent, price, quantity, seconds, usd } from "./format";

/**
 * Gate reason codes (trading-domain.yaml) as the rule they enforce, in plain language. A denial is
 * shown as a rule, never as an error to retry (brief §5, rule 10).
 */
export function gateRule(code: ReasonCode, mandate: Mandate): string {
  const risk = mandate.risk;
  switch (code) {
    case "account_restricted":
      return "The broker has restricted the account to closing orders.";
    case "account_trading_blocked":
      return "Trading is blocked on the account at the broker.";
    case "agent_exits_only":
      return "The agent is selling only, so it opens and adds to nothing.";
    case "agent_paused":
      return "The agent is paused, so it sends no new orders except protection.";
    case "agent_stopped":
      return "The agent is stopped.";
    case "not_in_universe":
      return "The instrument is not in this agent's universe.";
    case "concentration_limit":
      return `A position is at most ${usd(risk.max_position_usd)} or ${percent(risk.max_position_fraction, 0)} of equity, whichever is lower.`;
    case "max_order_size":
      return `Orders are at most ${usd(risk.max_order_usd)}.`;
    case "reentry_cooldown":
      return `No re-entry within ${seconds(risk.reentry_cooldown_s)} of an exit in the same instrument.`;
    case "session_not_allowed":
      return "This mandate does not trade in the current session.";
    case "extended_hours_opening_not_allowed":
      return "Opening orders only in the regular session.";
    case "auction_window":
      return "No new orders during the opening or closing auction.";
    case "instrument_halted":
      return "Trading in the instrument is halted.";
    case "unknown_order_in_flight":
      return "An order in this instrument has an unknown state; nothing else is sent in it until the broker answers.";
    case "stale_mark":
      return "No openings in an instrument whose price is stale.";
    case "price_outside_collar":
      return "The limit price is too far from the current price.";
    case "max_orders_per_day":
      return `At most ${risk.max_orders_per_day} orders a day.`;
    case "close_window":
      return "No opening orders in the last 10 minutes of the regular session.";
    case "discretionary_exit_regular_session_only":
      return "Waiting for the regular session.";
    case "owner_confirmation_required":
      return "This needs your confirmation first.";
    case "gross_exposure_limit":
      return `Total holdings are at most ${usd(risk.max_gross_exposure_usd)} or equity, whichever is lower.`;
    case "insufficient_buying_power":
      return "Not enough buying power on the account.";
    default: {
      const unhandled: never = code;
      throw new Error(`unhandled reason code ${String(unhandled)}`);
    }
  }
}

const EXIT_PURPOSES: ReadonlySet<Purpose> = new Set(["discretionary_exit", "owner_exit", "risk_exit", "protective"]);

export function isExit(purpose: Purpose): boolean {
  return EXIT_PURPOSES.has(purpose);
}

/** Exits are "held" or "waiting", never "denied" (brief §5, rule 13). The gate's own word, for a sentence about the gate. */
export function verdictLabel(decision: GateDecision): string {
  switch (decision.verdict) {
    case "allow":
      return "Allowed";
    case "defer":
      return "Waiting";
    case "deny":
      return isExit(decision.action.purpose) ? "Held" : "Not allowed";
    default: {
      const unhandled: never = decision.verdict;
      throw new Error(`unhandled verdict ${String(unhandled)}`);
    }
  }
}

/**
 * The word a decision wears in a list (DEC-503): an allow the mandate turned into a request reads
 * "Asked you", so "Allowed" beside an action only ever means the gate passed it and it went out.
 */
export function verdictBadge(decision: GateDecision): string {
  return decision.verdict === "allow" && decision.approval_id ? "Asked you" : verdictLabel(decision);
}

export function actionSentence(action: GateDecision["action"]): string {
  const side = action.side === "buy" ? "Buy" : "Sell";
  return `${side} ${quantity(action.qty)} ${action.symbol} at ${price(action.limit_price)}`;
}
