/**
 * The agent's chat over the workspace API (spec §4.6.2, DEC-2293): Home, a thread with an agent or a
 * setup, Stop, goals, and the chat settings. Reads go through the workspace client and its envelope;
 * the four writes are sent here, each with its own `Idempotency-Key` and the CSRF header, and a write
 * whose answer is lost is `unknown`, never success (API-13).
 *
 * The cards are typed data the server wrote. Model text is only ever `text`, shown as quoted text,
 * and never becomes a button, an amount or a link (rule 4).
 */
import { CSRF_HEADER, type Fetch, type WorkspaceClient, newIdempotencyKey } from "./client";
import { array, integer, isRecord, nullable, object, string } from "./decode";
import type { Decoded, Decoder } from "./types";
import { percent, usd } from "@/lib/format";

/** Who wrote a message. */
export type Author = "owner" | "agent";

/** A record or item a message relied on: `ev:<event_id>`, `news:<id>`, `brief:<date>`, or a URL. */
export interface Source {
  label: string;
  ref: string;
}

/** One message in a thread (`GET …/messages`). `card` is the server's typed data, or `null`. */
export interface ChatMessage {
  id: string;
  seq: number;
  at: string;
  from: Author;
  text: string | null;
  card: Card | null;
  sources: Source[];
  proactive: boolean;
}

/** What a card shows: each kind the server writes, and any other kind as `unknown`, shown as nothing. */
export type Card =
  | { kind: "fill"; side: string; symbol: string; qty: string; price: string }
  | { kind: "approval"; approval_id: string; side: string; symbol: string; qty: string; limit: string | null }
  | { kind: "proposal"; side: string; symbol: string; qty: string | null; reason: string }
  | { kind: "confirm_mandate"; version: string; name: string | null; lines: { label: string; value: string }[] }
  | ConfirmOrderCard
  | { kind: "unknown" };

/**
 * The owner's buy the chat prepared and did not send (`confirm_order`, DEC-2859): the instrument, the
 * quantity the owner asked for, the last trusted close and the estimate at it, or `null` when no
 * close is served, and the server's note. Only the owner's tap on it submits the request.
 */
export interface ConfirmOrderCard {
  kind: "confirm_order";
  card_id: string;
  symbol: string;
  instrument_id: string;
  qty: string;
  reference_price: string | null;
  estimated_cost_usd: string | null;
  note: string;
}

/** What the owner's tap on a `confirm_order` card came to: the owner request it recorded. */
export interface Confirmed {
  command_id: string;
}

/** A thread as read: its messages after the cursor asked for, whether a run is in flight, and the next cursor. */
export interface ThreadRead {
  messages: ChatMessage[];
  running: boolean;
  run_id: string | null;
  cursor: number;
}

/** One agent on Home. */
export interface HomeAgent {
  agent_id: string;
  name: string;
  mode: string;
  needs_you: boolean;
  unread: number;
  last_message: { text: string | null; from: Author } | null;
}

export interface Goals {
  goals: { text: string; status: "active" | "done" }[];
  plan: string | null;
  watching: string[];
}

/** A chat model the owner may choose, with its prices per million tokens in USD. */
export interface ChatModel {
  model_id: string;
  label: string;
  input_per_million_usd: string;
  output_per_million_usd: string;
}

export interface ChatSettings {
  chat_model: string | null;
  daily_cap_usd: string;
}

/** What a send came to: the run it started (and the setup it named), or why not. */
export interface Sent {
  message_id: string;
  run_id: string;
  setup_id: string | null;
}

/** A thread: an agent's, or a setup's (`null` before its first message). */
export type ChatTarget = { agent: string } | { setup: string | null };

export type Answer<T> = { ok: true; value: T } | { ok: false; reason: string; effect: "none" | "unknown" };

/** The chat's reads and writes, over the API or the fixtures. */
export interface ChatPort {
  home: () => Promise<Answer<HomeAgent[]>>;
  thread: (target: ChatTarget, after: number) => Promise<Answer<ThreadRead>>;
  send: (target: ChatTarget, text: string) => Promise<Answer<Sent>>;
  stop: (agentId: string, runId: string) => Promise<Answer<null>>;
  goals: (agentId: string) => Promise<Answer<Goals>>;
  models: () => Promise<Answer<ChatModel[]>>;
  settings: (agentId: string) => Promise<Answer<ChatSettings>>;
  saveSettings: (agentId: string, settings: { chat_model: string; daily_cap_usd: string }) => Promise<Answer<ChatSettings>>;
  /** The owner's tap on the `confirm_order` card of `messageId` (§4.6.2's *Confirm an order*). */
  confirmOrder: (agentId: string, messageId: string) => Promise<Answer<Confirmed>>;
}

/** The longest message the server takes, in characters (§4.6.2). */
export const MAX_MESSAGE_CHARS = 4000;

const fail = (path: string): Decoded<never> => ({ ok: false, issue: { path, problem: "wrong_type", value: null, allowed: null } });
const boolean: Decoder<boolean> = (value, path) => (typeof value === "boolean" ? { ok: true, value } : fail(path));
const author: Decoder<Author> = (value, path) => (value === "owner" || value === "agent" ? { ok: true, value } : fail(path));
const sources = array(object<Source>({ label: string, ref: string }));

const text = (value: Record<string, unknown>, name: string): string => (typeof value[name] === "string" ? (value[name] as string) : "");
const optional = (value: Record<string, unknown>, name: string): string | null => (typeof value[name] === "string" ? (value[name] as string) : null);

/** A card as the server wrote it; a kind this page does not know, or one missing a member it shows, is `unknown`. */
export function cardOf(value: unknown): Card | null {
  if (value === null || value === undefined) return null;
  if (!isRecord(value)) return { kind: "unknown" };
  switch (value.kind) {
    case "fill":
      return { kind: "fill", side: text(value, "side"), symbol: text(value, "symbol"), qty: text(value, "qty"), price: text(value, "price") };
    case "approval": {
      const order = isRecord(value.order) ? value.order : {};
      const id = optional(value, "approval_id");
      if (id === null) return { kind: "unknown" };
      return { kind: "approval", approval_id: id, side: text(order, "side"), symbol: optional(value, "symbol") ?? text(order, "instrument"), qty: text(order, "qty"), limit: optional(order, "limit") };
    }
    case "proposal":
      return { kind: "proposal", side: text(value, "side"), symbol: text(value, "symbol"), qty: optional(value, "qty"), reason: text(value, "reason") };
    case "confirm_order": {
      const id = optional(value, "card_id");
      const instrument = optional(value, "instrument_id");
      const qty = optional(value, "qty");
      if (id === null || instrument === null || qty === null || value.side !== "buy") return { kind: "unknown" };
      return {
        kind: "confirm_order",
        card_id: id,
        symbol: text(value, "symbol"),
        instrument_id: instrument,
        qty,
        reference_price: optional(value, "reference_price"),
        estimated_cost_usd: optional(value, "estimated_cost_usd"),
        note: text(value, "note"),
      };
    }
    case "confirm_mandate": {
      const version = optional(value, "version");
      if (version === null) return { kind: "unknown" };
      const summary = isRecord(value.summary) ? value.summary : {};
      const mandate = isRecord(summary.mandate) ? summary.mandate : {};
      return { kind: "confirm_mandate", version, name: optional(summary, "name"), lines: summaryLines(mandate) };
    }
    default:
      return { kind: "unknown" };
  }
}

/** The mandate summary's members a confirm card shows, in this order, with their labels. */
const SUMMARY_LABELS: readonly (readonly [string, string, "usd" | "fraction" | "plain"])[] = [
  ["allocation_usd", "Capital", "usd"],
  ["goal", "Goal", "plain"],
  ["max_position_usd", "Largest position", "usd"],
  ["max_order_usd", "Largest order", "usd"],
  ["max_orders_per_day", "Orders a day, at most", "plain"],
  ["max_daily_loss", "Daily loss limit", "fraction"],
  ["max_drawdown", "Drawdown limit", "fraction"],
  ["max_loss_from_allocation", "Most it may lose", "fraction"],
];

/** A summary value as words: dollars, a percentage of the allocation, or the value itself. */
function summaryValue(value: unknown, kind: "usd" | "fraction" | "plain"): string | null {
  if (typeof value === "number") return String(value);
  if (typeof value !== "string" || value === "") return null;
  const numeric = /^\d+(\.\d+)?$/.test(value);
  if (kind === "usd" && numeric) return usd(value);
  if (kind === "fraction" && numeric) return percent(value, 2).replace(/\.?0+%$/, "%");
  return value.replaceAll("_", " ");
}

/** The confirm card's lines, from the server's mandate summary; a member it does not hold is left out. */
export function summaryLines(mandate: Record<string, unknown>): { label: string; value: string }[] {
  const lines: { label: string; value: string }[] = [];
  for (const [name, label, kind] of SUMMARY_LABELS) {
    const value = summaryValue(mandate[name], kind);
    if (value !== null) lines.push({ label, value });
  }
  const classes = mandate.asset_classes;
  if (Array.isArray(classes) && classes.every((c) => typeof c === "string")) lines.push({ label: "Trades", value: classes.map((c) => c.replaceAll("_", " ")).join(", ") });
  return lines;
}

const card: Decoder<Card | null> = (value) => ({ ok: true, value: cardOf(value) });

export const messageRead = object<ChatMessage>({
  id: string,
  seq: integer,
  at: string,
  from: author,
  text: nullable(string),
  card,
  sources,
  proactive: boolean,
});

export const threadRead = object<ThreadRead>({ messages: array(messageRead), running: boolean, run_id: nullable(string), cursor: integer });

const homeAgent = object<HomeAgent>({
  agent_id: string,
  name: string,
  mode: string,
  needs_you: boolean,
  unread: integer,
  last_message: nullable(object<{ text: string | null; from: Author }>({ text: nullable(string), from: author })),
});

export const homeRead = object<{ agents: HomeAgent[] }>({ agents: array(homeAgent) });

const status: Decoder<"active" | "done"> = (value, path) => (value === "active" || value === "done" ? { ok: true, value } : fail(path));
export const goalsRead = object<Goals>({ goals: array(object({ text: string, status })), plan: nullable(string), watching: array(string) });

export const modelsRead = object<{ models: ChatModel[] }>({
  models: array(object<ChatModel>({ model_id: string, label: string, input_per_million_usd: string, output_per_million_usd: string })),
});

export const settingsRead = object<ChatSettings>({ chat_model: nullable(string), daily_cap_usd: string });

const sentRead = object<Sent>({ message_id: string, run_id: string, setup_id: nullable(string) });

const confirmedRead = object<Confirmed>({ command_id: string });

/** The route of `target`'s thread under the workspace. */
export function threadRoute(target: ChatTarget): `/${string}` {
  if ("agent" in target) return `/agents/${encodeURIComponent(target.agent)}/messages`;
  return target.setup === null ? "/setup/messages" : `/setup/${encodeURIComponent(target.setup)}/messages`;
}

/** The route a message to `target` is posted to. */
export function sendRoute(target: ChatTarget): `/${string}` {
  if ("agent" in target) return threadRoute(target);
  return target.setup === null ? "/setup/messages" : `/setup/messages?setup_id=${encodeURIComponent(target.setup)}`;
}

/**
 * The chat over a deployment's workspace API. `fetch` and `baseUrl` are the client's own, so a write
 * carries the same credentials as every command.
 */
export function apiChat(client: WorkspaceClient, fetch: Fetch, baseUrl: string): ChatPort {
  const credentials: RequestCredentials = baseUrl === "" ? "same-origin" : "include";
  async function read<T>(route: `/${string}`, decoder: Decoder<T>): Promise<Answer<T>> {
    const answer = await client.read(route, decoder);
    return answer.ok ? { ok: true, value: answer.value } : { ok: false, reason: answer.error.code, effect: "none" };
  }
  async function write<T>(method: "POST" | "PUT", route: string, body: unknown, decoder: Decoder<T>): Promise<Answer<T>> {
    let response: Response;
    try {
      response = await fetch(baseUrl + client.path(route as `/${string}`), {
        method,
        credentials,
        headers: { "Content-Type": "application/json", Accept: "application/json", [CSRF_HEADER]: "1", "Idempotency-Key": newIdempotencyKey() },
        body: JSON.stringify(body),
      });
    } catch {
      return { ok: false, reason: "network", effect: "unknown" };
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(await response.text()) as unknown;
    } catch {
      parsed = undefined;
    }
    if (!response.ok) {
      const code = isRecord(parsed) && typeof parsed.code === "string" ? parsed.code : "invalid_response";
      const effect = isRecord(parsed) && parsed.effect === "none" ? "none" : response.status < 500 ? "none" : "unknown";
      return { ok: false, reason: code, effect };
    }
    const value = decoder(parsed, "");
    return value.ok ? { ok: true, value: value.value } : { ok: false, reason: "invalid_response", effect: "unknown" };
  }
  return {
    home: async () => {
      const answer = await read("/home", homeRead);
      return answer.ok ? { ok: true, value: answer.value.agents } : answer;
    },
    thread: (target, after) => {
      if ("setup" in target && target.setup === null) return Promise.resolve({ ok: true, value: { messages: [], running: false, run_id: null, cursor: 0 } });
      return read(`${threadRoute(target)}?after=${after}`, threadRead);
    },
    send: (target, said) => {
      return write("POST", sendRoute(target), { text: said }, sentRead);
    },
    stop: (agentId, runId) => write("POST", `/agents/${encodeURIComponent(agentId)}/runs/${encodeURIComponent(runId)}/stop`, {}, () => ({ ok: true, value: null })),
    goals: (agentId) => read(`/agents/${encodeURIComponent(agentId)}/goals`, goalsRead),
    models: async () => {
      const answer = await read("/models", modelsRead);
      return answer.ok ? { ok: true, value: answer.value.models } : answer;
    },
    settings: (agentId) => read(`/agents/${encodeURIComponent(agentId)}/settings`, settingsRead),
    saveSettings: (agentId, settings) => write("PUT", `/agents/${encodeURIComponent(agentId)}/settings`, settings, settingsRead),
    confirmOrder: (agentId, messageId) => write("POST", `/agents/${encodeURIComponent(agentId)}/messages/${encodeURIComponent(messageId)}/confirm`, {}, confirmedRead),
  };
}
