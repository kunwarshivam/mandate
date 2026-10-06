import type { Agent, Iso, Workspace } from "@/fixtures/types";
import { findAgent } from "@/fixtures/workspace";
import { gateRule, verdictLabel } from "./gate-reasons";
import { clock, price, quantity, usd } from "./format";
import { MODE_LABEL, MODE_MEANING } from "./labels";
import { agentLimits, headroomLine, headroomRows } from "./limits";
import { type DeskRole, deskFor, figureLine, threadItems } from "./messages";
import { approvalAt } from "./mock-runtime";
import { RESTRICTIONS } from "./restrictions";
import { agentHref, decisionHref, positionHref } from "./screens";

/**
 * Asking the record (DEC-476): the owner's words matched to a fixed set of questions, each answered
 * by code from the journal and the mandate, with a link to every record it read. No model writes an
 * answer, nothing is kept after the page closes, and no answer places an order, changes a mandate
 * or advises a trade. Only Pause is sent from a message, and only when the owner asked for it.
 */

export interface Cite {
  href: string;
  label: string;
}

export type Reply =
  | { kind: "answer"; lines: string[]; cites: Cite[]; follow: string[] }
  /** The owner asked to pause: a card with Pause, which lowers risk and needs no passkey. */
  | { kind: "pause"; agentId: string }
  /** Stop and resume need a passkey, so the reply opens the Stop sheet rather than sending anything. */
  | { kind: "stop"; agentId: string | null; resume: boolean }
  /**
   * Creating an agent ends at its summary, confirmed with a passkey, so the reply hands over to
   * setup. `text` is the owner's words when they say more than "create an agent", else null.
   */
  | { kind: "create"; text: string | null };

export interface AskContext {
  ws: Workspace;
  now: Iso;
  /** The thread the owner is in, if any. */
  agentId: string | null;
}

const ORDER_WORDS = /\b(buy|sell|short|purchase|place (an? )?order|trade (it|this|that)|go long|close (my|the|a) position)\b/;
const PAUSE_WORDS = /\b(pause|hold off|freeze|halt)\b/;
const STOP_WORDS = /\b(stop|kill|end (it|the agent)|shut (it )?down|close everything|release)\b/;
const RESUME_WORDS = /\b(resume|unpause|un-pause|restart|start (it )?again)\b/;
/** A question about the record, never a request to act: "Is Agent 2 paused?" asks, it does not pause. */
const QUESTION = /^(is|are|was|were|why|what|how|when|did|does|do|has|have|who|which|where)\b|\bwhy\b/;
const ADVICE_WORDS = /\b(should|good (time|idea)|worth|recommend|advi[cs]e)\b/;
const CREATE_WORDS = /\b(new agent|create (an? )?agent|make (an? )?agent|set up (an? )?agent|another agent|build (an? )?agent)\b/;

const FILLER = new Set(["please", "can", "you", "could", "want", "would", "like", "lets", "let's", "for", "the", "and", "now", "help", "create", "make", "set", "build", "new", "another", "agent"]);

/** Whether a request to create an agent says anything about the agent, beyond asking for one. */
function describes(text: string): boolean {
  return text
    .replace(CREATE_WORDS, " ")
    .split(/[^a-z0-9$%']+/)
    .some((w) => w.length > 2 && !FILLER.has(w));
}

/** Questions an answer may offer next. Plain questions, never a suggestion to trade. */
export const STARTERS = ["What needs me?", "Why did it ask?", "How close is it to its limits?", "What happened today?", "What does it hold?"] as const;

function mentioned(ws: Workspace, text: string): Agent | undefined {
  const match = /\bagent\s*(\d+)\b/.exec(text);
  return match ? ws.agents.find((a) => a.label.toLowerCase() === `agent ${match[1]}`) : undefined;
}

function scope(ctx: AskContext, text: string): Agent[] {
  const named = mentioned(ctx.ws, text);
  if (named) return [named];
  const here = ctx.agentId ? findAgent(ctx.ws, ctx.agentId) : undefined;
  return here ? [here] : ctx.ws.agents;
}

function answer(lines: string[], cites: Cite[] = [], follow: string[] = []): Reply {
  return { kind: "answer", lines, cites, follow };
}

function needsYou(ctx: AskContext, agents: Agent[]): Reply {
  const ids = new Set(agents.map((a) => a.agent_id));
  const open = ctx.ws.approvals
    .map((a) => approvalAt(a, ctx.now))
    .filter((a) => a.status === "delivered" && ids.has(a.agent_id))
    .sort((a, b) => Date.parse(a.deadline) - Date.parse(b.deadline));
  if (open.length === 0) return answer(["Nothing is waiting for you."], [{ href: "/approvals", label: "Approvals" }], ["What happened today?"]);
  const label = (id: string) => findAgent(ctx.ws, id)?.label ?? "An agent";
  return answer(
    [
      `${open.length === 1 ? "One request is" : `${open.length} requests are`} waiting for you.`,
      ...open.map((a) => `${label(a.agent_id)}: buy ${quantity(a.bound.qty)} ${a.bound.symbol} at ${price(a.bound.limit)}. Skipped at ${clock(a.deadline)} if you do nothing.`),
    ],
    open.map((a) => ({ href: `/approvals/${a.approval_id}`, label: `${label(a.agent_id)}'s request` })),
    ["Why did it ask?"],
  );
}

const CITE_LABEL: Record<DeskRole, string> = {
  research: "Model output",
  trader: "The order",
  risk: "Gate decision",
  autonomy: "The rule in the mandate",
  you: "The request",
};

function why(ctx: AskContext, agents: Agent[]): Reply {
  if (agents.length !== 1) return answer(["Which agent? Name it, for example \u201cWhy did Agent 2 ask?\u201d"], [], agents.map((a) => `Why did ${a.label} ask?`));
  const [agent] = agents;
  const desk = deskFor(ctx.ws, agent.agent_id, ctx.now);
  if (!desk) return answer([`Nothing is recorded for ${agent.label} yet: no request and no gate decision.`]);
  const risk = desk.steps.find((s) => s.role === "risk");
  const rules = desk.steps.find((s) => s.role === "autonomy");
  const research = desk.steps.find((s) => s.role === "research");
  const cites: Cite[] = desk.steps.flatMap((s) => (s.href ? [{ href: s.href, label: CITE_LABEL[s.role] }] : []));
  const lines = [`${agent.label}'s latest: ${desk.title}.`];
  if (rules) lines.push(...rules.lines);
  if (risk) lines.push(`Risk gate: ${risk.lines[0]}`, ...risk.figures.map(figureLine));
  if (research && research.quotes.length > 0) lines.push(`${research.quotes.length === 1 ? "One model" : `${research.quotes.length} models`} gave output; the desk quotes it as written.`);
  else lines.push("No model output is recorded with it.");
  return answer(lines, dedupe(cites), ["How close is it to its limits?"]);
}

function dedupe(cites: Cite[]): Cite[] {
  return cites.filter((c, i) => cites.findIndex((d) => d.href === c.href) === i);
}

function limits(agents: Agent[]): Reply {
  if (agents.length === 0) return answer(["You have no agents yet."]);
  const lines: string[] = [];
  for (const agent of agents) {
    const l = agentLimits(agent);
    lines.push(`${agent.label}: ${headroomLine(agent)}.`);
    for (const row of headroomRows(l)) lines.push(`${row.label}: ${row.headroom} headroom under a limit of ${row.limit}. At the limit: ${row.atCap.toLowerCase()}.`);
    lines.push(`Orders today: ${l.ordersToday} of ${l.ordersCap}.`);
  }
  return answer(
    lines,
    agents.map((a) => ({ href: agentHref(a.agent_id, "mandate"), label: `${a.label}'s mandate` })),
    ["What does it hold?"],
  );
}

function holdings(agents: Agent[], now: Iso): Reply {
  const held = agents.flatMap((agent) => agent.positions.map((p) => ({ agent, p })));
  if (held.length === 0) return answer([agents.length === 1 ? `${agents[0].label} holds nothing.` : "Your agents hold nothing."], [{ href: "/positions", label: "Positions" }]);
  return answer(
    held.map(({ agent, p }) => `${agent.label}: ${quantity(p.qty)} ${p.instrument.symbol}, worth ${usd(p.market_value)} at the ${clock(p.mark_as_of)} mark${p.mark_as_of.slice(0, 10) === now.slice(0, 10) ? "" : ` of ${p.mark_as_of.slice(0, 10)}`}.`),
    held.map(({ agent, p }) => ({ href: positionHref(agent.agent_id, p.instrument.asset_id), label: `${p.instrument.symbol} position` })),
    ["How close is it to its limits?"],
  );
}

function happened(ctx: AskContext, agents: Agent[]): Reply {
  const today = ctx.now.slice(0, 10);
  const items = agents
    .flatMap((agent) => threadItems(ctx.ws, agent.agent_id, ctx.now).map((item) => ({ agent, item })))
    .filter(({ item }) => item.at.slice(0, 10) === today)
    .sort((a, b) => Date.parse(b.item.at) - Date.parse(a.item.at))
    .slice(0, 5);
  if (items.length === 0) return answer(["Nothing is recorded today."], agents.map((a) => ({ href: agentHref(a.agent_id, "activity"), label: `${a.label}'s activity` })));
  const many = agents.length > 1;
  return answer(
    items.map(({ agent, item }) => {
      const what = item.kind === "event" ? item.event.text : `Request to buy ${quantity(item.approval.bound.qty)} ${item.approval.bound.symbol} at ${price(item.approval.bound.limit)}.`;
      return `${clock(item.at)}${many ? ` ${agent.label}` : ""}: ${what}`;
    }),
    agents.map((a) => ({ href: agentHref(a.agent_id, "activity"), label: `${a.label}'s activity` })),
    ["Why did it ask?"],
  );
}

function status(agents: Agent[]): Reply {
  if (agents.length === 0) return answer(["You have no agents yet."]);
  return answer(
    agents.flatMap((a) => [`${a.label} is ${MODE_LABEL[a.mode].toLowerCase()}. ${MODE_MEANING[a.mode]}`, ...a.restrictions.map((r) => `Since ${clock(r.since)}: ${RESTRICTIONS[r.code].label}.`)]),
    agents.map((a) => ({ href: agentHref(a.agent_id, "overview"), label: a.label })),
  );
}

function denied(ctx: AskContext, agents: Agent[]): Reply {
  const ids = new Set(agents.map((a) => a.agent_id));
  const denials = ctx.ws.decisions.filter((d) => ids.has(d.agent_id) && d.verdict !== "allow").sort((a, b) => Date.parse(b.at) - Date.parse(a.at));
  if (denials.length === 0) return answer(["The gate has not held or refused anything recorded here."]);
  const d = denials[0];
  const agent = findAgent(ctx.ws, d.agent_id);
  if (!agent) return answer(["Not recorded."]);
  return answer(
    [`At ${clock(d.at)}, ${agent.label}: ${d.action.side} ${quantity(d.action.qty)} ${d.action.symbol}. ${verdictLabel(d)}${d.reason_code ? `: ${gateRule(d.reason_code, agent.mandate)}` : "."}`],
    [{ href: decisionHref(agent.agent_id, d.event_id), label: "Gate decision" }],
  );
}

function pick(ctx: AskContext, text: string): Agent | "many" | null {
  const agents = scope(ctx, text);
  if (agents.length === 1) return agents[0];
  return agents.length === 0 ? null : "many";
}

/**
 * The reply to one message. Matching is by fixed phrases, so the same words always get the same
 * reply; anything unmatched says what can be asked rather than guessing.
 */
export function interpret(said: string, ctx: AskContext): Reply {
  const text = said.toLowerCase().replace(/\s+/g, " ").trim();
  const agents = scope(ctx, text);
  const asking = QUESTION.test(text);

  if (CREATE_WORDS.test(text)) return { kind: "create", text: describes(text) ? said.trim() : null };
  if (RESUME_WORDS.test(text) && !asking) {
    const one = pick(ctx, text);
    return { kind: "stop", agentId: one && one !== "many" ? one.agent_id : null, resume: true };
  }
  if (PAUSE_WORDS.test(text) && !asking) {
    const one = pick(ctx, text);
    if (one === null) return answer(["You have no agents to pause."]);
    if (one === "many") return answer(["Which agent? To pause every agent at once, use Stop."], [], ctx.ws.agents.filter((a) => a.mode === "normal" || a.mode === "exits_only").map((a) => `Pause ${a.label}`));
    if (one.mode === "paused") return answer([`${one.label} is already paused.`], [{ href: agentHref(one.agent_id, "activity"), label: `${one.label}'s activity` }]);
    if (one.mode === "stopped") return answer([`${one.label} is stopped, so there is nothing to pause.`]);
    return { kind: "pause", agentId: one.agent_id };
  }
  if (STOP_WORDS.test(text) && !asking) {
    const one = pick(ctx, text);
    return { kind: "stop", agentId: one && one !== "many" ? one.agent_id : null, resume: false };
  }
  if (ORDER_WORDS.test(text) && (!asking || ADVICE_WORDS.test(text))) {
    return answer(
      [
        "Orders aren't placed from a message, and I don't suggest trades.",
        "An agent proposes orders within its mandate, the risk gate checks each one, and your rules decide which come to you as a request.",
      ],
      [],
      ["What needs me?"],
    );
  }
  if (/\b(needs? me|waiting|approv|request|pending)/.test(text) && !/\bwhy\b/.test(text)) return needsYou(ctx, agents);
  if (/\bwhy\b/.test(text) && /\b(den|refus|not allowed|held|block|reject)/.test(text)) return denied(ctx, agents);
  if (/\bwhy\b/.test(text)) return why(ctx, agents);
  if (/\b(limit|headroom|how close|loss today|budget|room)/.test(text)) return limits(agents);
  if (/\b(hold|position|own)/.test(text)) return holdings(agents, ctx.now);
  if (/\b(happen|today|recent|activit|latest|update|doing)/.test(text)) return happened(ctx, agents);
  if (/\b(status|mode|running|state)\b|\bis it (paused|running|stopped|on)\b/.test(text)) return status(agents);
  if (/\b(help|what can you|how do i|what do you)\b/.test(text)) {
    return answer(
      [
        "I answer from your record: what needs you, why an agent asked, how close it is to its limits, what it holds, and what happened today. Each answer links to the records it read.",
        "I can pause an agent when you ask, open Stop, or start setting up a new agent. I don't place orders or change a mandate.",
      ],
      [],
      [...STARTERS],
    );
  }
  return answer(["That isn't something I can find in the record.", "Try one of these, or open the records directly."], [], [...STARTERS]);
}
