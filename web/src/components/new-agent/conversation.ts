import { type Dec, div, mul } from "@/lib/decimal";
import { percent, usd } from "@/lib/format";
import type { Asked, CompilerInput, OwnerMessage, Reading, Turn } from "./compiler";
import { type Draft, LOSS_CEILING, type Loss, MODELS, NO_STRATEGY, type Strategy, checkLoss, compile, lossCeilingUsd, readStated, readStrategy, readSymbols } from "./draft";

/**
 * Setting up an agent as one conversation (brief A0 to A2, DEC-476, DEC-477). The owner answers in
 * their own words, one question at a time or all at once; the compiler's model reads each message,
 * and this module, deterministic and pure, decides what that changes, what it refuses, and what to
 * say next. Once nothing is missing it shows the whole agent once, for the owner to create or change.
 * The log only grows: a past message stays as it was shown.
 */

/**
 * What the account knows that a value must fit: the money no agent uses (V-002), and who trades a
 * symbol (V-006); and whether the workspace's independent-approval policy holds (V-047, rule 3:
 * anything but a stated `false`), under which no draft can be created here.
 */
export interface Checks {
  room: Dec;
  claimedBy: (symbol: string) => string | null;
  independentApproval: boolean;
}

export type Step =
  | { kind: "money" }
  | { kind: "goal" }
  | { kind: "loss" }
  | { kind: "symbols" }
  | { kind: "model" }
  | { kind: "param"; key: string }
  | { kind: "ready" };

export type Entry =
  | { id: string; kind: "owner"; text: string }
  /** The platform's reply: deterministic sentences, with the model's own reply first when it gave one. `asks` is the step it leaves open; `unread` marks a reply to a message that changed nothing. */
  | { id: string; kind: "said"; lines: string[]; asks: Step; unread?: true }
  /** The whole agent as it stood at `revision`, for the owner to create. */
  | { id: string; kind: "summary"; revision: number }
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
  /** Reading without the model, after the owner chose to when it did not answer. */
  figuresOnly: boolean;
  /** Counts every change to the owner's values, so a summary shown before a change is never the one created. */
  revision: number;
  serial: number;
}

export function sameStep(a: Step, b: Step): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === "param" && b.kind === "param") return a.key === b.key;
  return true;
}

type NewEntry = Entry extends infer E ? (E extends Entry ? Omit<E, "id"> : never) : never;

function push(c: Conversation, ...entries: NewEntry[]): Conversation {
  let next = c;
  for (const e of entries) next = { ...next, serial: next.serial + 1, entries: [...next.entries, { ...e, id: `e${next.serial}` } as Entry] };
  return next;
}

export function nextStep(c: Conversation): Step {
  if (!c.money) return { kind: "money" };
  if (!c.goal) return { kind: "goal" };
  if (!c.loss) return { kind: "loss" };
  if (!c.symbols) return { kind: "symbols" };
  const model = MODELS.find((m) => m.id === c.strategy.model);
  if (!model) return { kind: "model" };
  const unset = model.params.find((p) => !c.strategy.params[p.key]);
  return unset ? { kind: "param", key: unset.key } : { kind: "ready" };
}

export function askedFor(step: Step): Asked {
  switch (step.kind) {
    case "money":
    case "goal":
    case "loss":
    case "symbols":
    case "model":
      return step.kind;
    case "param":
      return `param:${step.key}`;
    case "ready":
      return "review";
    default: {
      const unhandled: never = step;
      throw new Error(`unhandled step ${JSON.stringify(unhandled)}`);
    }
  }
}

export const INTRO =
  "Let's set up your agent. Tell me about it in your own words: how much money it can use, what it's for, and how much you could stand to lose. All at once is fine, or one at a time.";

export const READY = "That's everything. Here is your agent. Create it, or tell me what to change.";

/** The ready line under the independent-approval policy: the draft is whole, but nothing here can create it (V-047, interim). */
export const READY_NEEDS_SECOND_PERSON = "That's everything. Here is your agent. It can't be created here without a second person's approval.";

/** The one place the ready line is chosen: it invites a create only where a passkey alone may create. */
function readyLine(checks: Checks): string {
  return checks.independentApproval ? READY_NEEDS_SECOND_PERSON : READY;
}

/** The question for a step, with no example amounts or returns. */
export function question(step: Step, c: Conversation, checks: Checks): string {
  switch (step.kind) {
    case "money":
      return "How much money can it use, in dollars? It trades on paper, with simulated money.";
    case "goal":
      return "What's it for, in your own words?";
    case "loss":
      return `How much could you stand to lose, in total? In dollars, or as a share of the money.${c.money ? ` This workspace allows at most ${percent(LOSS_CEILING, 0)} of the money, ${usd(lossCeilingUsd(c.money.value))}.` : ""}`;
    case "symbols":
      return "Which stocks or ETFs can it trade? Write their symbols. Only you choose them.";
    case "model":
      return "How should it decide? Pick a model, or name it. They're in alphabetical order, and Owlhead recommends none.";
    case "param":
      return MODELS.flatMap((m) => m.params).find((p) => p.key === step.key)?.question ?? "";
    case "ready":
      return readyLine(checks);
    default: {
      const unhandled: never = step;
      throw new Error(`unhandled step ${JSON.stringify(unhandled)}`);
    }
  }
}

/**
 * What to say when a message changed nothing and the model said nothing either. A second miss in a
 * row at the same step says what shape of answer would be read, never a value, instead of the same
 * sentence again.
 */
function unreadFor(step: Step, again: boolean): string {
  switch (step.kind) {
    case "money":
      return again
        ? "I still couldn't find an amount. Write the figure on its own, in dollars, with nothing else in the message."
        : "I couldn't find an amount in that. How much money can it use, in dollars?";
    case "goal":
      return "What's it for, in your own words?";
    case "loss":
      return again
        ? "I still couldn't find a loss. Write one figure on its own: dollars, or a percentage of the money."
        : "I couldn't find a loss in that. Write it in dollars, or as a percentage of the money.";
    case "symbols":
      return again
        ? "I still couldn't find a symbol. Write the ticker symbols on their own, in capitals, with a space between them."
        : "I couldn't find a symbol in that. Write the symbols themselves, separated by commas or spaces.";
    case "model":
      return again ? "I still couldn't match that to a model. Pick one of the models below, or write its name as it appears there." : "Pick one of the models, or write its name.";
    case "param":
      return again ? "I still couldn't read that as a number. Write the number on its own, in figures, inside the range the question gives." : "Write it as a number, in figures.";
    case "ready":
      return again
        ? "I still couldn't find a change. Name the value and what it should become in one sentence: the money, the goal, the loss, the symbols, or the model."
        : "I couldn't find a change in that. You can change the money, the goal, the loss, the symbols, or the model by saying so.";
    default: {
      const unhandled: never = step;
      throw new Error(`unhandled step ${JSON.stringify(unhandled)}`);
    }
  }
}

/** Whether the reply before this one, at the same step, already answered a message that changed nothing. */
function missedBefore(c: Conversation, step: Step): boolean {
  for (let i = c.entries.length - 1; i >= 0; i--) {
    const e = c.entries[i];
    if (e.kind === "owner") continue;
    return e.kind === "said" && e.unread === true && sameStep(e.asks, step);
  }
  return false;
}

export function startConversation(): Conversation {
  const empty: Conversation = { entries: [], messages: [], money: null, loss: null, goal: null, notes: [], symbols: null, strategy: NO_STRATEGY, figuresOnly: false, revision: 0, serial: 0 };
  return push(empty, { kind: "said", lines: [INTRO], asks: { kind: "money" } });
}

/** Adds the owner's message to the log and returns what the compiler is asked to read. */
export function say(c: Conversation, text: string): [Conversation, CompilerInput] {
  const id = `e${c.serial}`;
  const next: Conversation = { ...c, serial: c.serial + 1, messages: [...c.messages, { id, text }], entries: [...c.entries, { id, kind: "owner", text }] };
  return [next, inputFor(next, id)];
}

/** The compiler's input for one of the owner's messages, again on a retry. */
export function inputFor(c: Conversation, messageId: string): CompilerInput {
  const upTo = c.messages.findIndex((m) => m.id === messageId);
  return { messages: c.messages.slice(0, upTo + 1), asked: askedFor(nextStep(c)), models: MODELS.map((m) => ({ id: m.id, name: m.name })) };
}

function moneyRoom(value: Dec, checks: Checks): string | null {
  return value > checks.room ? `Your paper account has ${usd(checks.room)} that no agent uses, less than the ${usd(value)} this agent would use. Write a smaller amount.` : null;
}

function claimed(symbols: readonly string[], checks: Checks): string | null {
  for (const s of symbols) {
    const holder = checks.claimedBy(s);
    if (holder) return `${s} is already traded by ${holder}. One agent trades an instrument on an account; choose another.`;
  }
  return null;
}

/** A stated loss in dollars, with its share of the money once the money is known. */
function lossShort(loss: Loss, money: Dec | null): string {
  if (!money) return loss.kind === "usd" ? usd(loss.value) : `${percent(loss.value)} of the money`;
  const [dollars, share] = loss.kind === "usd" ? [loss.value, div(loss.value, money)] : [mul(money, loss.value), loss.value];
  return `${usd(dollars)} (${percent(share)} of the money)`;
}

/** What one reading did: an acknowledgment, a sentence saying how words were read, or a refusal. */
type Outcome = { noted: string; readAs?: string } | { refused: string } | null;

const changed = (c: Conversation, patch: Partial<Conversation>): Conversation => ({ ...c, ...patch, revision: c.revision + 1 });

function apply(c: Conversation, r: Reading, messageId: string, checks: Checks): [Conversation, Outcome] {
  switch (r.field) {
    case "money": {
      const full = moneyRoom(r.value, checks);
      if (full) return [c, { refused: full }];
      return [changed(c, { money: { value: r.value, quote: r.quote, messageId } }), { noted: `${usd(r.value)} to use`, ...(r.fromWords ? { readAs: `I read “${r.quote}” as ${usd(r.value)}.` } : {}) }];
    }
    case "loss": {
      if (c.money) {
        const fits = checkLoss(r.loss, c.money.value);
        if (!fits.ok) return [c, { refused: fits.error }];
      }
      const words = lossShort(r.loss, c.money?.value ?? null);
      return [changed(c, { loss: { loss: r.loss, quote: r.quote, messageId } }), { noted: `a loss limit of ${words}`, ...(r.fromWords ? { readAs: `I read “${r.quote}” as ${words}.` } : {}) }];
    }
    case "goal":
      return [changed(c, { goal: { quote: r.quote, messageId } }), { noted: `the goal “${r.quote}”` }];
    case "symbols": {
      const read = readSymbols(r.symbols.join(" "));
      if (!read.ok) return [c, { refused: read.error }];
      const taken = claimed(read.value, checks);
      if (taken) return [c, { refused: taken }];
      return [changed(c, { symbols: read.value }), { noted: `${read.value.join(", ")} to trade` }];
    }
    case "model": {
      const model = MODELS.find((m) => m.id === r.model);
      if (!model || c.strategy.model === model.id) return [c, null];
      return [chosen(c, model.id), { noted: `the ${model.name} model` }];
    }
    case "param": {
      const param = MODELS.find((m) => m.id === c.strategy.model)?.params.find((p) => p.key === r.key);
      if (!param) return [c, null];
      const read = param.read(r.value);
      if (!read.ok) return [c, { refused: read.error }];
      return [changed(c, { strategy: { ...c.strategy, params: { ...c.strategy.params, [r.key]: read.value } } }), { noted: `${param.label.split(",")[0].toLowerCase()} of ${read.value}` }];
    }
    default: {
      const unhandled: never = r;
      throw new Error(`unhandled reading ${JSON.stringify(unhandled)}`);
    }
  }
}

const FIELD_ORDER: Record<Reading["field"], number> = { money: 0, loss: 1, goal: 2, symbols: 3, model: 4, param: 5 };

function list(items: string[]): string {
  if (items.length <= 2) return items.join(" and ");
  return `${items.slice(0, -1).join(", ")}, and ${items.at(-1)}`;
}

/**
 * Ends the platform's turn: what it has to say, then the next question when the conversation moved
 * on or there is nothing else to say. Once nothing is missing, a change shows the whole agent again.
 */
function reply(before: Conversation, after: Conversation, lines: string[], checks: Checks, unread = false): Conversation {
  const was = nextStep(before);
  const step = nextStep(after);
  const moved = !sameStep(was, step);
  const mark = unread ? { unread: true as const } : {};
  if (step.kind === "ready") {
    if (moved || after.revision !== before.revision) return push(after, { kind: "said", lines: [...lines, readyLine(checks)], asks: step }, { kind: "summary", revision: after.revision });
    return push(after, { kind: "said", lines, asks: step, ...mark });
  }
  return push(after, { kind: "said", lines: moved || lines.length === 0 ? [...lines, question(step, after, checks)] : lines, asks: step, ...mark });
}

/** The compiler's checked turn, applied: the model's reply, what was noted or refused, then what comes next. */
export function applyTurn(c: Conversation, messageId: string, turn: Turn, checks: Checks): Conversation {
  let next = c;
  const noted: string[] = [];
  const readAs: string[] = [];
  const refused: string[] = [];
  for (const r of [...turn.readings].sort((a, b) => FIELD_ORDER[a.field] - FIELD_ORDER[b.field])) {
    const [after, outcome] = apply(next, r, messageId, checks);
    next = after;
    if (outcome && "refused" in outcome) refused.push(outcome.refused);
    else if (outcome) {
      noted.push(outcome.noted);
      if (outcome.readAs) readAs.push(outcome.readAs);
    }
  }
  if (next.money && next.loss && next.money !== c.money) {
    const fits = checkLoss(next.loss.loss, next.money.value);
    if (!fits.ok) {
      refused.push(`With ${usd(next.money.value)}, the loss you gave no longer fits. ${fits.error}`);
      next = { ...next, loss: null };
    }
  }
  const notes = turn.notes.filter((n) => !next.notes.includes(n));
  if (notes.length > 0) next = changed(next, { notes: [...next.notes, ...notes] });

  const goals = turn.readings.flatMap((r) => (r.field === "goal" ? [r.quote] : []));
  const announced = notes.filter((n) => !goals.some((g) => n.includes(g) || g.includes(n)));

  const lines: string[] = [];
  if (turn.reply) lines.push(turn.reply);
  if (turn.withheld) lines.push("I left out a reply that read like advice. Owlhead doesn't give any.");
  if (noted.length > 0) lines.push(`Got it: ${list(noted)}.`, ...readAs);
  if (announced.length > 0) lines.push(`No limit can check ${list(announced.map((n) => `“${n}”`))}, so it isn't enforced. The agent gets it as a note.`);
  lines.push(...refused);
  if (noted.length > 0 || refused.length > 0) return reply(c, next, lines, checks);
  const step = nextStep(next);
  if (!turn.reply && !turn.withheld) return reply(c, next, [...lines, unreadFor(step, missedBefore(c, step))], checks, true);
  return reply(c, next, step.kind === "ready" ? lines : [...lines, question(step, next, checks)], checks);
}

/** The compiler did not answer, or answered outside its schema: the owner's words stay, and nothing changed. */
export function failTurn(c: Conversation, messageId: string, reason: "unreachable" | "invalid"): Conversation {
  return push(c, { kind: "failed", reason, messageId });
}

export function readWithoutModel(c: Conversation): Conversation {
  return { ...c, figuresOnly: true };
}

function chosen(c: Conversation, model: Strategy["model"]): Conversation {
  const spec = MODELS.find((m) => m.id === model);
  const params = Object.fromEntries((spec?.params ?? []).filter((p) => c.strategy.params[p.key]).map((p) => [p.key, c.strategy.params[p.key]]));
  return changed(c, { strategy: { model, params } });
}

export function chooseModel(c: Conversation, model: Strategy["model"], checks: Checks): Conversation {
  const spec = MODELS.find((m) => m.id === model);
  if (!spec || c.strategy.model === model) return c;
  return reply(c, chosen(push(c, { kind: "owner", text: spec.name }), model), [`Got it: the ${spec.name} model.`], checks);
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

/** Why the agent cannot be created as it stands, or null: the account may have changed since a value was said. The runtime repeats V-002 and V-006 when it deploys. */
export function blocker(c: Conversation, checks: Checks): string | null {
  if (!c.money || !c.loss || !c.goal) return "Say how much money it can use, what it's for, and how much it could lose first.";
  const strategy = readStrategy(c.strategy);
  if (!strategy.ok) return strategy.error;
  if (!c.symbols || c.symbols.length === 0) return "Add at least one symbol. Only you choose what it may trade.";
  return moneyRoom(c.money.value, checks) ?? claimed(c.symbols, checks);
}

/** The summary the owner may create from: the last one shown, while nothing has changed since. */
export function currentSummary(c: Conversation): Extract<Entry, { kind: "summary" }> | null {
  const last = c.entries.findLast((e) => e.kind === "summary");
  return last?.kind === "summary" && last.revision === c.revision && nextStep(c).kind === "ready" ? last : null;
}
