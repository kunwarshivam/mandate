import { type Dec, div, mul } from "@/lib/decimal";
import { percent, usd } from "@/lib/format";
import type { Asked, CompilerInput, OwnerMessage, Reading, Turn } from "./compiler";
import {
  type Draft,
  type DraftSection,
  type Loss,
  MODELS,
  NO_STRATEGY,
  SECTION_KEYS,
  type SectionKey,
  type Strategy,
  checkLoss,
  compile,
  lossCeilingUsd,
  readStated,
  readStrategy,
  readSymbols,
} from "./draft";

/**
 * Setting up an agent as one conversation (brief A0 to A2, DEC-473). The owner answers in their own
 * words, one question at a time or all at once; the compiler's model reads each message, and this
 * module, deterministic and pure, decides what that changes, what it refuses, and what to ask next.
 * The log only grows: a past question, card or answer stays as it was shown.
 */

/** What the account knows that a value must fit: the money no agent uses (V-002), and who trades a symbol (V-006). */
export interface Checks {
  room: Dec;
  claimedBy: (symbol: string) => string | null;
}

export type Step =
  | { kind: "check" }
  | { kind: "money" }
  | { kind: "goal" }
  | { kind: "loss" }
  | { kind: "symbols" }
  | { kind: "model" }
  | { kind: "param"; key: string }
  | { kind: "section"; key: SectionKey }
  | { kind: "ready" };

export interface NotedItem {
  label: string;
  value: string;
  quote: string;
  provenance: "user_stated" | "user_entered";
}

/** A value read from words, not figures, held until the owner says it is right. */
export type Pending = { field: "money"; value: Dec; quote: string; messageId: string } | { field: "loss"; loss: Loss; quote: string; messageId: string };

export type Entry =
  | { id: string; kind: "intro" }
  | { id: string; kind: "ask"; step: Step; ceiling?: Dec; pending?: Pending; snapshot?: DraftSection }
  | { id: string; kind: "owner"; text: string; action: boolean }
  | { id: string; kind: "reply"; text: string }
  | { id: string; kind: "withheld" }
  | { id: string; kind: "noted"; items: NotedItem[] }
  | { id: string; kind: "refused"; text: string }
  | { id: string; kind: "unread"; text: string }
  | { id: string; kind: "draft" }
  | { id: string; kind: "failed"; reason: "unreachable" | "invalid"; messageId: string };

type Stated<T> = T & { quote: string; messageId: string };

export interface Conversation {
  entries: Entry[];
  messages: OwnerMessage[];
  money: Stated<{ value: Dec }> | null;
  loss: Stated<{ loss: Loss }> | null;
  goal: Stated<object> | null;
  /** Quotes the compiler marked as constraints no field can express. */
  notes: string[];
  symbols: string[] | null;
  strategy: Strategy;
  confirmed: Record<SectionKey, boolean>;
  pending: Pending[];
  /** Reading without the model, after the owner chose to when it did not answer. */
  figuresOnly: boolean;
  /** An envelope value changed since the draft was last shown. */
  redraft: boolean;
  serial: number;
}

const NONE_CONFIRMED: Record<SectionKey, boolean> = { money: false, limits: false, strategy: false, autonomy: false, universe: false };

export function sameStep(a: Step, b: Step): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === "param" && b.kind === "param") return a.key === b.key;
  if (a.kind === "section" && b.kind === "section") return a.key === b.key;
  return true;
}

function nextId(c: Conversation): [string, Conversation] {
  return [`e${c.serial}`, { ...c, serial: c.serial + 1 }];
}

type NewEntry = Entry extends infer E ? (E extends Entry ? Omit<E, "id"> : never) : never;

function push(c: Conversation, ...entries: NewEntry[]): Conversation {
  let next = c;
  for (const e of entries) {
    const [id, after] = nextId(next);
    next = { ...after, entries: [...after.entries, { ...e, id } as Entry] };
  }
  return next;
}

export function nextStep(c: Conversation): Step {
  if (c.pending.length > 0) return { kind: "check" };
  if (!c.money) return { kind: "money" };
  if (!c.goal) return { kind: "goal" };
  if (!c.loss) return { kind: "loss" };
  if (!c.symbols) return { kind: "symbols" };
  const model = MODELS.find((m) => m.id === c.strategy.model);
  if (!model) return { kind: "model" };
  const unset = model.params.find((p) => !c.strategy.params[p.key]);
  if (unset) return { kind: "param", key: unset.key };
  const open = SECTION_KEYS.find((k) => !c.confirmed[k]);
  return open ? { kind: "section", key: open } : { kind: "ready" };
}

export function askedFor(step: Step): Asked {
  switch (step.kind) {
    case "check":
    case "money":
    case "goal":
    case "loss":
    case "symbols":
    case "model":
      return step.kind;
    case "param":
      return `param:${step.key}`;
    case "section":
    case "ready":
      return "review";
    default: {
      const unhandled: never = step;
      throw new Error(`unhandled step ${JSON.stringify(unhandled)}`);
    }
  }
}

function lastAsk(c: Conversation): Extract<Entry, { kind: "ask" }> | null {
  for (let i = c.entries.length - 1; i >= 0; i--) {
    const e = c.entries[i];
    if (e.kind === "ask") return e;
  }
  return null;
}

/** The step the log shows as current: the last question asked, while it is still the next step. */
export function activeAsk(c: Conversation): Extract<Entry, { kind: "ask" }> | null {
  const ask = lastAsk(c);
  return ask && sameStep(ask.step, nextStep(c)) ? ask : null;
}

/** Asks the next question when it changed, after showing the draft again if the owner's values changed during the review. */
function advance(c: Conversation): Conversation {
  const step = nextStep(c);
  const ask = lastAsk(c);
  if (ask && sameStep(ask.step, step)) return c;
  let next = c;
  const reviewing = step.kind === "section" || step.kind === "ready";
  if (reviewing && next.redraft) next = { ...push(next, { kind: "draft" }), redraft: false };
  const extra = step.kind === "loss" && next.money ? { ceiling: lossCeilingUsd(next.money.value) } : step.kind === "check" ? { pending: next.pending[0] } : {};
  return push(next, { kind: "ask", step, ...extra });
}

export function startConversation(): Conversation {
  const empty: Conversation = {
    entries: [],
    messages: [],
    money: null,
    loss: null,
    goal: null,
    notes: [],
    symbols: null,
    strategy: NO_STRATEGY,
    confirmed: NONE_CONFIRMED,
    pending: [],
    figuresOnly: false,
    redraft: true,
    serial: 0,
  };
  return advance(push(empty, { kind: "intro" }));
}

/** Adds the owner's message to the log and returns what the compiler is asked to read. */
export function say(c: Conversation, text: string): [Conversation, CompilerInput] {
  const [id, withId] = nextId(c);
  const message = { id, text };
  const next: Conversation = { ...withId, messages: [...withId.messages, message], entries: [...withId.entries, { id, kind: "owner", text, action: false }] };
  return [next, inputFor(next, message.id)];
}

/** The compiler's input for one of the owner's messages, again on a retry. */
export function inputFor(c: Conversation, messageId: string): CompilerInput {
  const upTo = c.messages.findIndex((m) => m.id === messageId);
  return { messages: c.messages.slice(0, upTo + 1), asked: askedFor(nextStep(c)), models: MODELS.map((m) => ({ id: m.id, name: m.name })) };
}

const unconfirm = (c: Conversation, ...keys: SectionKey[]): Conversation => ({
  ...c,
  confirmed: { ...c.confirmed, ...Object.fromEntries(keys.map((k) => [k, false])) },
  redraft: true,
});

function moneyRoom(value: Dec, checks: Checks): string | null {
  return value > checks.room ? `Your paper account has ${usd(checks.room)} that no agent uses, less than the ${usd(value)} this agent would use. Write a smaller amount.` : null;
}

/** A stated loss in words: dollars with its share of the money, or a share with its dollars once the money is known. */
export function lossWords(loss: Loss, money: Dec | null): string {
  if (loss.kind === "usd") return money ? `${usd(loss.value)}, ${percent(div(loss.value, money))} of the money` : usd(loss.value);
  return money ? `${percent(loss.value)} of the money, ${usd(mul(money, loss.value))}` : `${percent(loss.value)} of the money`;
}

function setMoney(c: Conversation, value: Dec, quote: string, messageId: string, checks: Checks): [Conversation, NotedItem | string] {
  const full = moneyRoom(value, checks);
  if (full) return [c, full];
  return [{ ...unconfirm(c, "money", "limits"), money: { value, quote, messageId } }, { label: "Money it may use", value: usd(value), quote, provenance: "user_stated" }];
}

function setLoss(c: Conversation, loss: Loss, quote: string, messageId: string): [Conversation, NotedItem | string] {
  if (c.money) {
    const fits = checkLoss(loss, c.money.value);
    if (!fits.ok) return [c, fits.error];
  }
  return [{ ...unconfirm(c, "money", "limits"), loss: { loss, quote, messageId } }, { label: "Most it may lose, in total", value: lossWords(loss, c.money?.value ?? null), quote, provenance: "user_stated" }];
}

function apply(c: Conversation, r: Reading, messageId: string, checks: Checks): [Conversation, NotedItem | string | null] {
  switch (r.field) {
    case "money":
      if (r.check) return [{ ...c, pending: [...c.pending, { field: "money", value: r.value, quote: r.quote, messageId }] }, null];
      return setMoney(c, r.value, r.quote, messageId, checks);
    case "loss":
      if (r.check) return [{ ...c, pending: [...c.pending, { field: "loss", loss: r.loss, quote: r.quote, messageId }] }, null];
      return setLoss(c, r.loss, r.quote, messageId);
    case "goal":
      return [{ ...unconfirm(c, "money", "limits"), goal: { quote: r.quote, messageId } }, { label: "Goal", value: "In your words, as the mandate's description.", quote: r.quote, provenance: "user_stated" }];
    case "symbols": {
      const read = readSymbols(r.symbols.join(" "));
      if (!read.ok) return [c, read.error];
      for (const s of read.value) {
        const holder = checks.claimedBy(s);
        if (holder) return [c, `${s} is already traded by ${holder}. One agent trades an instrument on an account; choose another.`];
      }
      return [{ ...unconfirm(c, "universe"), symbols: read.value }, { label: "What it may trade", value: read.value.join(", "), quote: r.quote, provenance: "user_entered" }];
    }
    case "model": {
      const model = MODELS.find((m) => m.id === r.model);
      if (!model || c.strategy.model === model.id) return [c, null];
      return [chosen(c, model.id), { label: "How it decides", value: `${model.name} (${model.id} ${model.version})`, quote: r.quote, provenance: "user_entered" }];
    }
    case "param": {
      const param = MODELS.find((m) => m.id === c.strategy.model)?.params.find((p) => p.key === r.key);
      if (!param) return [c, null];
      const read = param.read(r.value);
      if (!read.ok) return [c, read.error];
      return [
        { ...unconfirm(c, "strategy"), strategy: { ...c.strategy, params: { ...c.strategy.params, [r.key]: read.value } } },
        { label: param.label, value: read.value, quote: r.quote, provenance: "user_entered" },
      ];
    }
    default: {
      const unhandled: never = r;
      throw new Error(`unhandled reading ${JSON.stringify(unhandled)}`);
    }
  }
}

const FIELD_ORDER: Record<Reading["field"], number> = { money: 0, loss: 1, goal: 2, symbols: 3, model: 4, param: 5 };

/** What to say when a message changed nothing and the model said nothing either. */
function unreadFor(step: Step): string {
  switch (step.kind) {
    case "money":
      return "We could not find an amount in that. Write how much money it may use, in dollars, in figures.";
    case "goal":
      return "Say what this agent is for, in your own words.";
    case "loss":
      return "We could not find a loss in that. Write it in figures: dollars, or a percentage of the money.";
    case "symbols":
      return "We could not find a symbol in that. Write the symbols themselves, separated by commas or spaces.";
    case "model":
      return "Choose one of the models above, or write its name.";
    case "param":
      return "Write it as a number, in figures.";
    case "check":
      return "Answer the question above, or write the amount in figures.";
    case "section":
    case "ready":
      return "We could not read a change in that. You can change the money, the goal, the loss, the symbols or the model by saying so.";
    default: {
      const unhandled: never = step;
      throw new Error(`unhandled step ${JSON.stringify(unhandled)}`);
    }
  }
}

/** The compiler's checked turn, applied: what it noted, what it refused, its reply, then the next question. */
export function applyTurn(c: Conversation, messageId: string, turn: Turn, checks: Checks): Conversation {
  const step = nextStep(c);
  let next = c;
  const noted: NotedItem[] = [];
  const refused: string[] = [];
  for (const r of [...turn.readings].sort((a, b) => FIELD_ORDER[a.field] - FIELD_ORDER[b.field])) {
    const [after, outcome] = apply(next, r, messageId, checks);
    next = after;
    if (typeof outcome === "string") refused.push(outcome);
    else if (outcome) noted.push(outcome);
  }
  if (next.money && next.loss && next.money !== c.money) {
    const fits = checkLoss(next.loss.loss, next.money.value);
    if (!fits.ok) {
      refused.push(`With ${usd(next.money.value)}, the loss you stated no longer fits. ${fits.error}`);
      next = { ...next, loss: null };
    }
  }
  next = { ...next, notes: [...next.notes, ...turn.notes.filter((n) => !next.notes.includes(n))] };
  if (turn.reply) next = push(next, { kind: "reply", text: turn.reply });
  if (turn.withheld) next = push(next, { kind: "withheld" });
  if (noted.length > 0) next = push(next, { kind: "noted", items: noted });
  for (const text of refused) next = push(next, { kind: "refused", text });
  const changed = noted.length > 0 || refused.length > 0 || next.pending.length !== c.pending.length;
  if (!changed && !turn.reply && !turn.withheld) next = push(next, { kind: "unread", text: unreadFor(step) });
  return advance(next);
}

/** The compiler did not answer, or answered outside its schema: the owner's words stay, and nothing changed. */
export function failTurn(c: Conversation, messageId: string, reason: "unreachable" | "invalid"): Conversation {
  return push(c, { kind: "failed", reason, messageId });
}

export function readWithoutModel(c: Conversation): Conversation {
  return { ...c, figuresOnly: true };
}

/** The owner's answer to a value read from words: yes keeps it as stated, no drops it and asks again. */
export function answerCheck(c: Conversation, yes: boolean, checks: Checks): Conversation {
  const [first, ...rest] = c.pending;
  if (!first) return c;
  const valueText = first.field === "money" ? usd(first.value) : lossWords(first.loss, c.money?.value ?? null);
  let next = push({ ...c, pending: rest }, { kind: "owner", text: yes ? `Yes, ${valueText}` : "No", action: true });
  if (!yes) return advance(push(next, { kind: "unread", text: "Then write it in figures, and it is read exactly as written." }));
  const [after, outcome] = first.field === "money" ? setMoney(next, first.value, first.quote, first.messageId, checks) : setLoss(next, first.loss, first.quote, first.messageId);
  next = typeof outcome === "string" ? push(after, { kind: "refused", text: outcome }) : push(after, { kind: "noted", items: [outcome] });
  return advance(next);
}

function chosen(c: Conversation, model: Strategy["model"]): Conversation {
  const spec = MODELS.find((m) => m.id === model);
  const params = Object.fromEntries((spec?.params ?? []).filter((p) => c.strategy.params[p.key]).map((p) => [p.key, c.strategy.params[p.key]]));
  return { ...unconfirm(c, "strategy"), strategy: { model, params } };
}

export function chooseModel(c: Conversation, model: Strategy["model"]): Conversation {
  const spec = MODELS.find((m) => m.id === model);
  if (!spec) return c;
  return advance(push(chosen(c, model), { kind: "owner", text: spec.name, action: true }));
}

/** The draft as the owner's values stand, or null while money, goal or loss is missing. */
export function draftOf(c: Conversation): Draft | null {
  if (!c.money || !c.loss || !c.goal) return null;
  const labels: Record<"money" | "goal" | "loss", string> = { money: "money", goal: "goal", loss: "loss you can stand" };
  const from = (["money", "goal", "loss"] as const).map((k) => ({ key: k, messageId: c[k]!.messageId }));
  const answers = c.messages
    .filter((m) => from.some((f) => f.messageId === m.id))
    .map((m) => {
      const named = from.filter((f) => f.messageId === m.id).map((f) => labels[f.key]);
      const label = named.length === 1 ? named[0] : `${named.slice(0, -1).join(", ")} and ${named.at(-1)}`;
      return { label: label[0].toUpperCase() + label.slice(1), quote: m.text.trim() };
    });
  const read = readStated({ money: c.money, loss: c.loss, goal: c.goal.quote, answers, notes: c.notes });
  if (!read.ok) return null;
  const strategy = readStrategy(c.strategy);
  return compile(read.value, c.symbols ?? [], strategy.ok ? strategy.value : null);
}

/** Why a section cannot be confirmed as it stands, or null. The runtime repeats V-002 and V-006 when it deploys. */
export function sectionBlocker(c: Conversation, key: SectionKey, checks: Checks): string | null {
  switch (key) {
    case "money":
      return c.money ? moneyRoom(c.money.value, checks) : "Say how much money it may use first.";
    case "strategy": {
      const read = readStrategy(c.strategy);
      return read.ok ? null : read.error;
    }
    case "universe": {
      if (!c.symbols || c.symbols.length === 0) return "Add at least one symbol. Only you choose what it may trade.";
      for (const s of c.symbols) {
        const holder = checks.claimedBy(s);
        if (holder) return `${s} is already traded by ${holder}. One agent trades an instrument on an account; choose another.`;
      }
      return null;
    }
    case "limits":
    case "autonomy":
      return null;
    default: {
      const unhandled: never = key;
      throw new Error(`unhandled section ${String(unhandled)}`);
    }
  }
}

/** The owner confirms the section the conversation is on; a section that cannot be confirmed says why and stays open. */
export function confirmSection(c: Conversation, key: SectionKey, checks: Checks): Conversation {
  const draft = draftOf(c);
  const section = draft?.sections.find((s) => s.key === key);
  if (!draft || !section) return c;
  const why = sectionBlocker(c, key, checks);
  if (why) return push(c, { kind: "refused", text: why });
  const shown = activeAsk(c);
  const entries = c.entries.map((e) => (e.kind === "ask" && e.id === shown?.id ? { ...e, snapshot: section } : e));
  return advance(push({ ...c, entries, confirmed: { ...c.confirmed, [key]: true } }, { kind: "owner", text: `Confirm ${section.title.toLowerCase()}`, action: true }));
}

export function allConfirmed(c: Conversation): boolean {
  return SECTION_KEYS.every((k) => c.confirmed[k]);
}
