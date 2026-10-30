/**
 * The chat over the recorded fixtures (DEC-975's stand-in): each agent's thread opens with what the
 * fixture workspace already holds, as typed cards, and every message gets the same recorded answer,
 * since no model runs here. It reads nothing the owner writes: no rule-based language parsing
 * (`AGENTS.md`, how to work, item 3).
 */
import type { Answer, ChatMessage, ChatModel, ChatPort, ChatSettings, ChatTarget, Goals, HomeAgent, Sent, ThreadRead } from "@/api/chat";
import type { Workspace } from "@/fixtures/types";

/** The fixture's one answer to anything an owner writes. */
export const FIXTURE_ANSWER = "This is recorded demo data, so there is no model to answer you here. Connect a workspace and I'll reply in plain words, act on what you ask within my mandate, and message you first when something happens.";

export const FIXTURE_MODELS: ChatModel[] = [
  { model_id: "llm.chat_sonnet", label: "claude-sonnet-5.5", input_per_million_usd: "3", output_per_million_usd: "15" },
  { model_id: "llm.chat_haiku", label: "claude-haiku-5.5", input_per_million_usd: "1", output_per_million_usd: "5" },
];

const DEFAULT_CAP = "5";

type Key = string;

const keyOf = (target: ChatTarget): Key => ("agent" in target ? `agent:${target.agent}` : `setup:${target.setup ?? ""}`);

/** The opening messages of an agent's fixture thread: its fills, then the approvals it waits on. */
function opening(ws: Workspace, agentId: string): ChatMessage[] {
  const agent = ws.agents.find((a) => a.agent_id === agentId);
  if (!agent) return [];
  const messages: ChatMessage[] = [];
  const at = ws.now;
  for (const fill of agent.fills.slice(-3)) {
    messages.push({
      id: `fx_fill_${fill.fill_id}`,
      seq: 0,
      at: fill.at,
      from: "agent",
      text: `${fill.side === "sell" ? "Sold" : "Bought"} ${fill.qty} ${fill.instrument.symbol} at ${fill.price}.`,
      card: { kind: "fill", side: fill.side, symbol: fill.instrument.symbol, qty: fill.qty, price: fill.price },
      sources: [],
      proactive: true,
    });
  }
  for (const approval of ws.approvals.filter((a) => a.agent_id === agentId && a.status === "delivered")) {
    messages.push({
      id: `fx_approval_${approval.approval_id}`,
      seq: 0,
      at,
      from: "agent",
      text: `I'd like your OK to ${approval.bound.side} ${approval.bound.qty} ${approval.bound.symbol}.`,
      card: { kind: "approval", approval_id: approval.approval_id, side: approval.bound.side, symbol: approval.bound.symbol, qty: approval.bound.qty, limit: approval.bound.limit },
      sources: [],
      proactive: true,
    });
  }
  return messages.map((m, i) => ({ ...m, seq: i + 1 }));
}

/** The fixture chat over `ws`; its threads live as long as the runtime that holds it. */
export function fixtureChat(ws: Workspace): ChatPort {
  const threads = new Map<Key, ChatMessage[]>();
  const settings = new Map<string, ChatSettings>();
  let seq = 1000;
  let setups = 0;
  const thread = (target: ChatTarget): ChatMessage[] => {
    const key = keyOf(target);
    let messages = threads.get(key);
    if (!messages) {
      messages = "agent" in target ? opening(ws, target.agent) : [];
      threads.set(key, messages);
    }
    return messages;
  };
  const ok = <T>(value: T): Promise<Answer<T>> => Promise.resolve({ ok: true, value });
  return {
    home: () =>
      ok<HomeAgent[]>(
        ws.agents.map((agent) => {
          const messages = thread({ agent: agent.agent_id });
          const last = messages.at(-1);
          return {
            agent_id: agent.agent_id,
            name: agent.label,
            mode: agent.mode === "normal" ? "active" : agent.mode,
            needs_you: ws.approvals.some((a) => a.agent_id === agent.agent_id && a.status === "delivered"),
            unread: messages.filter((m) => m.from === "agent").length,
            last_message: last ? { text: last.text, from: last.from } : null,
          };
        }),
      ),
    thread: (target, after) => {
      const messages = thread(target);
      const cursor = messages.at(-1)?.seq ?? after;
      return ok<ThreadRead>({ messages: messages.filter((m) => m.seq > after), running: false, run_id: null, cursor: Math.max(cursor, after) });
    },
    send: (target, text) => {
      const resolved: ChatTarget = "setup" in target && target.setup === null ? { setup: `setup_fixture_${(setups += 1)}` } : target;
      const messages = thread(resolved);
      const at = new Date(Date.parse(ws.now) + seq * 1000).toISOString();
      seq += 1;
      messages.push({ id: `fx_owner_${seq}`, seq, at, from: "owner", text, card: null, sources: [], proactive: false });
      seq += 1;
      messages.push({ id: `fx_agent_${seq}`, seq, at, from: "agent", text: FIXTURE_ANSWER, card: null, sources: [], proactive: false });
      return ok<Sent>({ message_id: `fx_owner_${seq - 1}`, run_id: `run_fixture_${seq}`, setup_id: "setup" in resolved ? resolved.setup : null });
    },
    stop: () => ok(null),
    goals: () => ok<Goals>({ goals: [], plan: null, watching: [] }),
    models: () => ok(FIXTURE_MODELS),
    settings: (agentId) => ok(settings.get(agentId) ?? { chat_model: null, daily_cap_usd: DEFAULT_CAP }),
    saveSettings: (agentId, next) => {
      settings.set(agentId, next);
      return ok<ChatSettings>(next);
    },
    confirmOrder: (agentId, messageId) =>
      ws.agents.some((a) => a.agent_id === agentId) ? ok({ command_id: `fx_request_${messageId}` }) : Promise.resolve({ ok: false, reason: "not_found", effect: "none" }),
  };
}
