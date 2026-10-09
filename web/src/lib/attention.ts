import type { OrderState, Workspace } from "@/fixtures/types";
import { MODE_LABEL } from "./labels";
import { agentLimits, levelNoun, nearLossLimits } from "./limits";
import { RESTRICTIONS, type RestrictionSource } from "./restrictions";
import { agentHref, orderHref } from "./screens";

const HEALTH_WORD = { market_data: "Market data", broker: "Broker", deployment: "Deployment", relay: "Push relay" } as const;

function lowerFirst(text: string): string {
  return text.charAt(0).toLowerCase() + text.slice(1);
}

export interface AlertLine {
  key: string;
  text: string;
  /** Where the alert is read in full: the Alerts screen for a feed, the agent for its condition. */
  href: string;
  /** A degraded feed, or whoever imposed an agent's restriction. */
  source: RestrictionSource | "feed";
}

/** Everything the Alerts screen lists, one line each: degraded feeds first, then agent conditions. */
export function alertLines(ws: Workspace): AlertLine[] {
  const feeds = (Object.keys(HEALTH_WORD) as Array<keyof typeof HEALTH_WORD>)
    .filter((k) => ws.health[k].state !== "ok")
    .map((k) => ({ key: `feed-${k}`, text: `${HEALTH_WORD[k]} ${ws.health[k].state === "down" ? "down" : "stale"}`, href: "/alerts", source: "feed" as const }));
  const agents = ws.agents.flatMap((a) =>
    a.restrictions.map((r) => ({
      key: `${a.agent_id}-${r.code}-${r.symbol ?? ""}`,
      text: `${a.label}: ${lowerFirst(RESTRICTIONS[r.code].label)}${r.symbol ? ` (${r.symbol})` : ""}`,
      href: `/agents/${a.agent_id}`,
      source: RESTRICTIONS[r.code].source,
    })),
  );
  return [...feeds, ...agents];
}

/**
 * What Needs you lists under the requests (DEC-513): the agents' conditions, one line per agent and
 * condition with every instrument it covers in one pair of brackets, and no feed. A degraded feed is
 * the status strip's and the Alerts screen's to say, and it asks nothing of the owner. An order whose
 * state is unknown turns Stop loud, so it is one of its agent's conditions too, with no instrument,
 * opening the order's record, or the agent's orders when there are several (C-25).
 */
export function needsYouLines(ws: Workspace): AlertLine[] {
  const lines: AlertLine[] = [];
  for (const a of ws.agents) {
    const byCode = new Map<string, { symbols: string[]; source: RestrictionSource }>();
    for (const r of a.restrictions) {
      const entry = byCode.get(r.code) ?? { symbols: [], source: RESTRICTIONS[r.code].source };
      if (r.symbol) entry.symbols.push(r.symbol);
      byCode.set(r.code, entry);
    }
    for (const [code, { symbols, source }] of byCode) {
      lines.push({
        key: `${a.agent_id}-${code}`,
        text: `${a.label}: ${lowerFirst(RESTRICTIONS[code as keyof typeof RESTRICTIONS].label)}${symbols.length > 0 ? ` (${symbols.join(", ")})` : ""}`,
        href: `/agents/${a.agent_id}`,
        source,
      });
    }
    const unknown = a.orders.filter((o) => o.state === "Unknown");
    if (unknown.length > 0) {
      lines.push({
        key: `${a.agent_id}-unknown-order`,
        text: `${a.label}: ${unknown.length === 1 ? "an order's state is unknown" : `${unknown.length} orders' states are unknown`}`,
        href: unknown.length === 1 ? orderHref(a.agent_id, unknown[0].client_order_id) : agentHref(a.agent_id, "orders"),
        source: "account",
      });
    }
  }
  return lines;
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
    if (deepest) reasons.push(`${agent.label} ${deepest.reached ? "has reached" : "is near"} its ${levelNoun(deepest)}`);
  }
  const inFlight = ws.agents.reduce((n, a) => n + a.orders.filter((o) => IN_FLIGHT[o.state]).length, 0);
  if (inFlight > 0) reasons.push(inFlight === 1 ? "1 order the broker has not confirmed" : `${inFlight} orders the broker has not confirmed`);
  return reasons;
}

export function attentionText(reasons: readonly string[]): string {
  return `Needs attention: ${reasons.join(", ")}`;
}
