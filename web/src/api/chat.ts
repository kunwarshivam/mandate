/**
 * The agent chat (`POST /agents/{id}/chat`, and `POST /chat` for creating an agent; builder 4's
 * route, its contract still being agreed): the owner's words go to the deployment, which asks the
 * model through the gateway and answers with the model's reply and at most one proposed action. The
 * reply is the model's text, shown as quoted text and never parsed. The action is a typed card: the
 * web builds every request from it itself, through the paths the rest of the app uses, and only when
 * the owner taps Send. A card the web cannot read is dropped, never half-shown.
 */
import { closedEnum, decimal, isRecord, string } from "./decode";
import type { ContentRef, Decoded, DecodeProblem, Decoder } from "./types";

export const CHAT_ACTIONS = ["ask_to_buy", "pause", "resume", "stop", "owner_exit", "kill_switch", "create_agent"] as const;

export type ChatCard =
  | { action: "ask_to_buy"; instrument_id: string; symbol: string; size: string }
  | { action: "pause" | "resume" | "stop" | "kill_switch" }
  | { action: "owner_exit"; instruments: Array<{ instrument_id: string; symbol: string; qty: string }> }
  | { action: "create_agent"; mandate_version: ContentRef };

export interface ChatAnswer {
  /** The model's words, as the deployment quotes them: plain text, shown as the assistant's. */
  reply: string;
  card: ChatCard | null;
}

export const agentChatRoute = (agentId: string) => `/agents/${encodeURIComponent(agentId)}/chat` as const;
export const WORKSPACE_CHAT_ROUTE = "/chat";

const ASSET_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const CONTENT_REF = /^sha256:[0-9a-f]{64}$/;
const MAX_REPLY = 4000;

const fail = (path: string, problem: DecodeProblem): Decoded<never> => ({ ok: false, issue: { path, problem, value: null, allowed: null } });

const assetId: Decoder<string> = (value, path) => (typeof value === "string" && ASSET_ID.test(value) ? { ok: true, value } : fail(path, typeof value === "string" ? "not_canonical" : "wrong_type"));
const symbol: Decoder<string> = (value, path) => (typeof value === "string" && value.length >= 1 && value.length <= 32 ? { ok: true, value } : fail(path, typeof value === "string" ? "not_canonical" : "wrong_type"));
const positive: Decoder<string> = (value, path) => {
  const read = decimal(value, path);
  if (!read.ok) return read;
  return read.value.startsWith("-") || /^0(\.0*)?$/.test(read.value) ? fail(path, "not_canonical") : read;
};

/** A card's typed summary for its action; anything off its shape drops the card. */
function card(value: unknown, path: string): Decoded<ChatCard> {
  if (!isRecord(value)) return fail(path, "wrong_type");
  const action = closedEnum(CHAT_ACTIONS)(value.action, `${path}/action`);
  if (!action.ok) return action;
  const summary = value.summary;
  const at = `${path}/summary`;
  if (!isRecord(summary)) return fail(at, "wrong_type");
  switch (action.value) {
    case "ask_to_buy": {
      const id = assetId(summary.instrument_id, `${at}/instrument_id`);
      if (!id.ok) return id;
      const sym = symbol(summary.symbol, `${at}/symbol`);
      if (!sym.ok) return sym;
      const size = positive(summary.size, `${at}/size`);
      if (!size.ok) return size;
      return { ok: true, value: { action: "ask_to_buy", instrument_id: id.value, symbol: sym.value, size: size.value } };
    }
    case "owner_exit": {
      if (!Array.isArray(summary.instruments) || summary.instruments.length === 0) return fail(`${at}/instruments`, "wrong_type");
      const instruments: Array<{ instrument_id: string; symbol: string; qty: string }> = [];
      for (const [i, item] of summary.instruments.entries()) {
        const p = `${at}/instruments/${i}`;
        if (!isRecord(item)) return fail(p, "wrong_type");
        const id = assetId(item.instrument_id, `${p}/instrument_id`);
        if (!id.ok) return id;
        const sym = symbol(item.symbol, `${p}/symbol`);
        if (!sym.ok) return sym;
        const qty = positive(item.qty, `${p}/qty`);
        if (!qty.ok) return qty;
        instruments.push({ instrument_id: id.value, symbol: sym.value, qty: qty.value });
      }
      return { ok: true, value: { action: "owner_exit", instruments } };
    }
    case "create_agent": {
      const v = summary.mandate_version;
      return typeof v === "string" && CONTENT_REF.test(v) ? { ok: true, value: { action: "create_agent", mandate_version: v as ContentRef } } : fail(`${at}/mandate_version`, "not_canonical");
    }
    default:
      return { ok: true, value: { action: action.value } };
  }
}

/** The chat's answer: the reply always, and a card only when every member of it reads. */
export const chatRead: Decoder<ChatAnswer> = (value, path) => {
  if (!isRecord(value)) return fail(path, "wrong_type");
  if (!isRecord(value.reply)) return fail(`${path}/reply`, "wrong_type");
  const text = string(value.reply.text, `${path}/reply/text`);
  if (!text.ok) return text;
  if (text.value.length > MAX_REPLY) return fail(`${path}/reply/text`, "not_canonical");
  if (value.card === null || value.card === undefined) return { ok: true, value: { reply: text.value, card: null } };
  const read = card(value.card, `${path}/card`);
  return { ok: true, value: { reply: text.value, card: read.ok ? read.value : null } };
};
