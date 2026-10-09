import type { Agent, Approval, GateDecision, Iso, ModelOutput, RiskFigure, TimelineEvent, Workspace } from "@/fixtures/types";
import { verdictLabel, verdictLine } from "./gate-reasons";
import { clock, clockShort, dateLabel, price, quantity, usd } from "./format";
import { APPROVAL_STATUS_LABEL, PURPOSE_LABEL, RISK_CAP_LABEL, RISK_FIGURE_LABEL } from "./labels";
import { approvalAt } from "./mock-runtime";

/**
 * Messages (DEC-479): each agent's thread, read from the journal. Nothing here is written by a model
 * or kept on the device; every item is a journaled event or request, at the time it was recorded.
 */

export type ThreadItem = { kind: "event"; id: string; at: Iso; event: TimelineEvent } | { kind: "request"; id: string; at: Iso; approval: Approval };

/**
 * An agent's thread, oldest first, as a chat reads. Each request becomes one request item at the time
 * it was made; the journal's own line for asking, skipping or expiring it is the same fact at the same
 * time, so it is not repeated.
 */
export function threadItems(ws: Workspace, agentId: string, now: Iso): ThreadItem[] {
  const approvals = ws.approvals.filter((a) => a.agent_id === agentId).map((a) => approvalAt(a, now));
  const covered = new Set(approvals.flatMap((a) => [a.requested_at, ...(a.resolution ? [a.resolution.at] : [])].map((at) => Date.parse(at))));
  const events: ThreadItem[] = (ws.timeline[agentId] ?? [])
    .filter((e) => !(e.kind === "approval" && covered.has(Date.parse(e.at))))
    .map((event) => ({ kind: "event", id: event.event_id, at: event.at, event }));
  const requests: ThreadItem[] = approvals.map((approval) => ({ kind: "request", id: approval.approval_id, at: approval.requested_at, approval }));
  return [...events, ...requests].sort((a, b) => Date.parse(a.at) - Date.parse(b.at));
}

export interface DayGroup<T> {
  day: string;
  label: string;
  items: T[];
}

/** Items in time order split by calendar day, in the fixture's own offset, "Today" for the day of `now`. */
export function byDay<T extends { at: Iso }>(items: T[], now: Iso): Array<DayGroup<T>> {
  const groups: Array<DayGroup<T>> = [];
  for (const item of items) {
    const day = item.at.slice(0, 10);
    const last = groups.at(-1);
    if (last?.day === day) last.items.push(item);
    else groups.push({ day, label: day === now.slice(0, 10) ? "Today" : dateLabel(item.at), items: [item] });
  }
  return groups;
}

/** A thread item in one line, for the inbox. */
export function itemLine(item: ThreadItem): string {
  if (item.kind === "event") return item.event.text;
  const { bound, status } = item.approval;
  const order = `buy ${quantity(bound.qty)} ${bound.symbol} at ${price(bound.limit)}`;
  return status === "delivered" ? `Asks you to approve ${order}.` : `Request to ${order}: ${APPROVAL_STATUS_LABEL[status].toLowerCase()}.`;
}

export interface InboxRow {
  agent: Agent;
  open: Approval[];
  last: ThreadItem | null;
}

export interface Inbox {
  needsYou: InboxRow[];
  earlier: InboxRow[];
}

/**
 * The inbox: agents with a request waiting, soonest deadline first, then every other agent by its
 * latest item. An agent with nothing journaled yet sorts last.
 */
export function inboxRows(ws: Workspace, now: Iso): Inbox {
  const rows = ws.agents.map((agent): InboxRow => {
    const items = threadItems(ws, agent.agent_id, now);
    const open = items.flatMap((i) => (i.kind === "request" && i.approval.status === "delivered" ? [i.approval] : []));
    open.sort((a, b) => Date.parse(a.deadline) - Date.parse(b.deadline));
    return { agent, open, last: items.at(-1) ?? null };
  });
  const latest = (r: InboxRow) => (r.last ? Date.parse(r.last.at) : Number.NEGATIVE_INFINITY);
  return {
    needsYou: rows.filter((r) => r.open.length > 0).sort((a, b) => Date.parse(a.open[0].deadline) - Date.parse(b.open[0].deadline)),
    earlier: rows.filter((r) => r.open.length === 0).sort((a, b) => latest(b) - latest(a)),
  };
}

export type DeskRole = "research" | "trader" | "risk" | "autonomy" | "you";

export interface DeskStep {
  role: DeskRole;
  title: string;
  /** When the journal recorded this step; null when it did not, and the screen says so. */
  at: Iso | null;
  lines: string[];
  /** Model output, quoted as the model wrote it (D14). Only the research step carries it. */
  quotes: ModelOutput[];
  figures: RiskFigure[];
  href: string | null;
}

export interface Desk {
  subject: "request" | "decision";
  title: string;
  steps: DeskStep[];
}

export function figureLine(f: RiskFigure): string {
  return f.cap ? `${RISK_FIGURE_LABEL[f.field]}: ${usd(f.value)} of the ${usd(f.cap)} ${RISK_CAP_LABEL[f.field]}` : `${RISK_FIGURE_LABEL[f.field]}: ${usd(f.value)}`;
}

function decisionFor(ws: Workspace, approval: Approval): GateDecision | undefined {
  return ws.decisions.find((d) => d.approval_id === approval.approval_id);
}

function requestDesk(ws: Workspace, agent: Agent, approval: Approval): Desk {
  const { bound } = approval;
  const decision = decisionFor(ws, approval);
  const order = `Buy ${quantity(bound.qty)} ${bound.symbol} at a limit of ${price(bound.limit)}`;
  const you: DeskStep =
    approval.status === "delivered"
      ? { role: "you", title: "You", at: null, lines: ["Waiting for you."], quotes: [], figures: [], href: `/approvals/${approval.approval_id}` }
      : {
          role: "you",
          title: "You",
          at: approval.resolution?.at ?? null,
          lines: [approval.resolution?.text ?? APPROVAL_STATUS_LABEL[approval.status]],
          quotes: [],
          figures: [],
          href: `/approvals/${approval.approval_id}`,
        };
  return {
    subject: "request",
    title: `${order}, ${approval.status === "delivered" ? "waiting for you" : APPROVAL_STATUS_LABEL[approval.status].toLowerCase()}`,
    steps: [
      {
        role: "research",
        title: "Signal models",
        at: approval.evidence.length > 0 ? approval.evidence.reduce((a, b) => (Date.parse(b.produced_at) > Date.parse(a.produced_at) ? b : a)).produced_at : null,
        lines: approval.evidence.length > 0 ? [] : ["No model output was recorded for this request."],
        quotes: approval.evidence,
        figures: [],
        href: null,
      },
      {
        role: "trader",
        title: "Order builder",
        at: null,
        lines: [`${order}. Purpose: ${PURPOSE_LABEL[bound.purpose].toLowerCase()}.`, `Combined score ${bound.combined_score}, from the models' fixed weights.`],
        quotes: [],
        figures: [],
        href: null,
      },
      decision
        ? {
            role: "risk",
            title: "Risk gate",
            at: decision.at,
            lines: [verdictLine(decision, agent)],
            quotes: [],
            figures: approval.risk_impact,
            href: `/agents/${agent.agent_id}/decisions/${decision.event_id}`,
          }
        : { role: "risk", title: "Risk gate", at: null, lines: ["The gate's decision is not recorded here."], quotes: [], figures: approval.risk_impact, href: null },
      { role: "autonomy", title: "Your rules", at: approval.requested_at, lines: [approval.trigger], quotes: [], figures: [], href: `/agents/${agent.agent_id}/mandate` },
      you,
    ],
  };
}

function decisionDesk(agent: Agent, decision: GateDecision): Desk {
  const { action } = decision;
  const order = `${action.side === "buy" ? "Buy" : "Sell"} ${quantity(action.qty)} ${action.symbol} at a limit of ${price(action.limit_price)}`;
  return {
    subject: "decision",
    title: `${order}, ${verdictLabel(decision).toLowerCase()}`,
    steps: [
      { role: "research", title: "Signal models", at: null, lines: ["No model output is recorded with this decision."], quotes: [], figures: [], href: null },
      { role: "trader", title: "Order builder", at: null, lines: [`${order}. Purpose: ${PURPOSE_LABEL[action.purpose].toLowerCase()}.`], quotes: [], figures: [], href: null },
      {
        role: "risk",
        title: "Risk gate",
        at: decision.at,
        lines: [verdictLine(decision, agent), ...(decision.then ? [decision.then] : [])],
        quotes: [],
        figures: [],
        href: `/agents/${agent.agent_id}/decisions/${decision.event_id}`,
      },
    ],
  };
}

/**
 * The desk: how the agent's latest request was made, step by step, from the journal (DEC-479).
 * Model output is quoted; every other line is written by code from a recorded fact, and a step the
 * journal did not time carries no time. With no request yet, the latest gate decision; with neither, null.
 */
export function deskFor(ws: Workspace, agentId: string, now: Iso): Desk | null {
  const agent = ws.agents.find((a) => a.agent_id === agentId);
  if (!agent) return null;
  const approvals = ws.approvals.filter((a) => a.agent_id === agentId).map((a) => approvalAt(a, now));
  const open = approvals.filter((a) => a.status === "delivered").sort((a, b) => Date.parse(a.deadline) - Date.parse(b.deadline));
  const latest = open[0] ?? approvals.sort((a, b) => Date.parse(b.requested_at) - Date.parse(a.requested_at))[0];
  if (latest) return requestDesk(ws, agent, latest);
  const decision = ws.decisions.filter((d) => d.agent_id === agentId).sort((a, b) => Date.parse(b.at) - Date.parse(a.at))[0];
  return decision ? decisionDesk(agent, decision) : null;
}

/** Times in the thread: the clock alone for today, the date before it otherwise. */
export function stamp(at: Iso, now: Iso): string {
  return at.slice(0, 10) === now.slice(0, 10) ? clock(at) : `${dateLabel(at)}, ${clock(at)}`;
}

/** Times in a list of threads: hours and minutes for today, the month and day otherwise. */
export function listStamp(at: Iso, now: Iso): string {
  return at.slice(0, 10) === now.slice(0, 10) ? clockShort(at) : dateLabel(at).replace(/, \d{4}$/, "");
}
