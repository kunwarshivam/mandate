import type { Agent, Approval, GateDecision, Workspace } from "@/fixtures/types";
import { canOpen } from "./access";
import { actionSentence, gateRule, isExit } from "./gate-reasons";
import { price, quantity } from "./format";
import { APPROVAL_STATUS_LABEL } from "./labels";
import { approvalAt } from "./mock-runtime";
import { fillsOf, findOrder } from "./orders";
import { RESTRICTIONS } from "./restrictions";
import type { Role } from "./roles";
import { agentHref, decisionHref } from "./screens";

/** Where an item stands, always in words: exits are held or waiting, never blocked (brief §5, rule 13). */
export type WireState = "waiting_for_you" | "blocked" | "held" | "waiting" | "done" | "paused" | "stopped";

export const WIRE_STATE_LABEL: Record<WireState, string> = {
  waiting_for_you: "Waiting for you",
  blocked: "Blocked",
  held: "Held",
  waiting: "Waiting",
  done: "Done",
  paused: "Paused",
  stopped: "Stopped",
};

export interface WireItem {
  key: string;
  at: string;
  /** The agent's platform-assigned label. */
  agent: string;
  /** What happened, after the agent's name: "asked you to buy 2 XYZ at $141.30". */
  phrase: string;
  state: WireState;
  href: string;
}

/** The wire carries at most this many items; the audit screens carry the rest. */
export const WIRE_MAX = 12;

function clause(sentence: string): string {
  return (sentence.charAt(0).toLowerCase() + sentence.slice(1)).replace(/\.$/, "");
}

function requestPhrase(a: Approval): string {
  const { qty, symbol, limit, purpose } = a.bound;
  return `asked you to ${clause(actionSentence({ side: "buy", qty, symbol, limit_price: limit, purpose }))}`;
}

function allowPhrase(d: GateDecision, agent: Agent | undefined, action: string, request: Approval | undefined): string {
  if (d.approval_id) return request ? `asked you to ${action}: ${clause(APPROVAL_STATUS_LABEL[request.status])}` : `asked you to ${action}`;
  if (!d.client_order_id || !agent) return `was allowed to ${action}`;
  const order = findOrder(agent, d.client_order_id);
  if (order?.state !== "Filled") return `placed an order to ${action}`;
  const fills = fillsOf(agent, d.client_order_id);
  const verb = d.action.side === "buy" ? "bought" : "sold";
  if (fills.length === 1) return `${verb} ${quantity(fills[0].qty)} ${fills[0].instrument.symbol} at ${price(fills[0].price)}`;
  return `${verb} ${quantity(order.qty)} ${order.instrument.symbol}`;
}

function decisionItem(d: GateDecision, agent: Agent | undefined, request: Approval | undefined): Pick<WireItem, "phrase" | "state"> {
  const action = clause(actionSentence(d.action));
  const rule = d.reason_code && agent ? clause(gateRule(d.reason_code, agent.mandate)) : null;
  switch (d.verdict) {
    case "allow":
      return { phrase: allowPhrase(d, agent, action, request), state: "done" };
    case "deny":
      if (isExit(d.action.purpose)) return { phrase: rule ? `holds ${action}: ${rule}` : `holds ${action}`, state: "held" };
      return { phrase: `blocked: ${rule ?? action}`, state: "blocked" };
    case "defer":
      return { phrase: rule ? `holds ${action}: ${rule}` : `holds ${action}`, state: "waiting" };
    default: {
      const unhandled: never = d.verdict;
      throw new Error(`unhandled verdict ${String(unhandled)}`);
    }
  }
}

/**
 * What the agents are doing today, from the same journal as the dashboard's recent activity: requests
 * waiting for you first, then decisions and your own pauses and stops, newest first. Each item links
 * where recent activity does. Prices appear as the order stated them; no amount won or lost ever does.
 */
export function wireItems(ws: Workspace, now: string, role: Role): WireItem[] {
  const label = (id: string) => ws.agents.find((a) => a.agent_id === id)?.label ?? "An agent";
  // Both times are written in the fixture's ET offset, so their date part is the ET day.
  const today = (at: string) => at.slice(0, 10) === now.slice(0, 10);
  const byNewest = (a: WireItem, b: WireItem) => Date.parse(b.at) - Date.parse(a.at);

  const requests = ws.approvals.map((a) => approvalAt(a, now));
  const open = requests.filter((a) => a.status === "delivered");
  const openIds = new Set(open.map((a) => a.approval_id));
  const waiting: WireItem[] = open.map((a) => ({
    key: a.approval_id,
    at: a.requested_at,
    agent: label(a.agent_id),
    phrase: requestPhrase(a),
    state: "waiting_for_you",
    href: `/approvals/${a.approval_id}`,
  }));

  const decisions: WireItem[] = ws.decisions
    .filter((d) => today(d.at) && !(d.approval_id && openIds.has(d.approval_id)))
    .map((d) => ({
      key: d.event_id,
      at: d.at,
      agent: label(d.agent_id),
      ...decisionItem(
        d,
        ws.agents.find((a) => a.agent_id === d.agent_id),
        requests.find((a) => a.approval_id === d.approval_id),
      ),
      href: decisionHref(d.agent_id, d.event_id),
    }));

  const yours: WireItem[] = ws.agents.flatMap((agent) =>
    agent.restrictions
      .filter((r) => (r.code === "owner_pause" || r.code === "stopped") && today(r.since))
      .map((r) => ({
        key: `${agent.agent_id}-${r.code}`,
        at: r.since,
        agent: agent.label,
        phrase: r.code === "stopped" ? "stopped by you" : clause(RESTRICTIONS.owner_pause.label),
        state: r.code === "stopped" ? ("stopped" as const) : ("paused" as const),
        href: agentHref(agent.agent_id, "overview"),
      })),
  );

  return [...waiting.sort(byNewest), ...[...decisions, ...yours].sort(byNewest)].filter((i) => canOpen(role, i.href)).slice(0, WIRE_MAX);
}
