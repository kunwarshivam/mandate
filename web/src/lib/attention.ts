import type { OrderState, Workspace } from "@/fixtures/types";
import { MODE_LABEL } from "./labels";
import { type Level, agentLimits, nearLossLimits } from "./limits";
import { RESTRICTIONS, type RestrictionSource } from "./restrictions";

const HEALTH_WORD = { market_data: "Market data", broker: "Broker", deployment: "Deployment", relay: "Push relay" } as const;

function lowerFirst(text: string): string {
  return text.charAt(0).toLowerCase() + text.slice(1);
}

export interface AlertLine {
  text: string;
  /** A degraded feed, or whoever imposed an agent's restriction. */
  source: RestrictionSource | "feed";
}

/** Everything the Alerts screen lists, one line each: degraded feeds first, then agent conditions. */
export function alertLines(ws: Workspace): AlertLine[] {
  const feeds = (Object.keys(HEALTH_WORD) as Array<keyof typeof HEALTH_WORD>)
    .filter((k) => ws.health[k].state !== "ok")
    .map((k) => ({ text: `${HEALTH_WORD[k]} ${ws.health[k].state === "down" ? "down" : "stale"}`, source: "feed" as const }));
  const agents = ws.agents.flatMap((a) =>
    a.restrictions.map((r) => ({
      text: `${a.label}: ${lowerFirst(RESTRICTIONS[r.code].label)}${r.symbol ? ` (${r.symbol})` : ""}`,
      source: RESTRICTIONS[r.code].source,
    })),
  );
  return [...feeds, ...agents];
}

/**
 * Orders whose standing the broker has not confirmed (trading-domain spec §5.7): recorded or being
 * sent, a cancel or replace not yet answered, or unknown. An order resting at the broker, partly
 * filled or not, is confirmed; protective stops rest there all day.
 */
export const IN_FLIGHT: Record<OrderState, boolean> = {
  Intent: true,
  Submitting: true,
  Accepted: false,
  PartiallyFilled: false,
  PendingCancel: true,
  PendingReplace: true,
  Filled: false,
  Canceled: false,
  Rejected: false,
  Expired: false,
  Replaced: false,
  Abandoned: false,
  Unknown: true,
};

function limitWords(level: Level): string {
  switch (level.kind) {
    case "daily":
      return "its daily loss limit";
    case "floor":
      return "its lifetime floor";
    case "rung":
      return `its ${lowerFirst(level.label)} level`;
    case "high_water_mark":
    case "profit_stop":
      return `its ${lowerFirst(level.label)}`;
    default: {
      const unhandled: never = level.kind;
      throw new Error(`unhandled level ${String(unhandled)}`);
    }
  }
}

/**
 * Why the Stop control should stand out (DEC-206), in words; none means it stays quiet. Only risk
 * counts: an unreachable deployment; an alert, unless it is the owner's own pause or stop; an agent in
 * exits only; an agent near a loss limit (`nearLossLimits`); an order in flight. A stopped agent
 * holds nothing, so its limits no longer count. A request waiting for approval is not a reason:
 * approving is not stopping, and requests have their own count.
 */
export function stopAttention(ws: Workspace): string[] {
  const reasons: string[] = [];
  if (ws.status === "unreachable") reasons.push("the deployment is unreachable");
  const alerts = alertLines(ws).filter((a) => a.source !== "owner").length;
  if (alerts > 0) reasons.push(alerts === 1 ? "1 alert" : `${alerts} alerts`);
  for (const agent of ws.agents) {
    if (agent.mode === "stopped") continue;
    if (agent.mode === "exits_only" && agent.restrictions.length === 0) reasons.push(`${agent.label}: ${lowerFirst(MODE_LABEL.exits_only)}`);
    const [deepest] = nearLossLimits(agentLimits(agent));
    if (deepest) reasons.push(`${agent.label} ${deepest.reached ? "has reached" : "is near"} ${limitWords(deepest)}`);
  }
  const inFlight = ws.agents.reduce((n, a) => n + a.orders.filter((o) => IN_FLIGHT[o.state]).length, 0);
  if (inFlight > 0) reasons.push(inFlight === 1 ? "1 order the broker has not confirmed" : `${inFlight} orders the broker has not confirmed`);
  return reasons;
}

export function attentionText(reasons: readonly string[]): string {
  return `Needs attention: ${reasons.join(", ")}`;
}
