import { type Dec, ONE, ZERO, dec, div, fromInt, mul, toDecimalString } from "@/lib/decimal";
import { GOAL_WORDS, LOSS_WORDS, type Loss, MODELS, type ModelId, clauses, constraintsIn, findAmounts, findPercents, firstLoss } from "./draft";

/**
 * The compiler's model turn (mandate spec §7, inference spec INF-5, DEC-473). Each owner message goes
 * to a model with what the conversation last asked; the model answers in JSON, and nothing it returns
 * is a mandate value until `validateTurn` has re-read it from the owner's own words.
 *
 * The model only points. A reading names a field and quotes the span of the owner's latest message it
 * read; the guard checks the quote is there word for word, reads the value again from the quote with
 * the same deterministic readers the draft uses, and drops the reading when the two disagree. A value
 * the quote states in words, not figures ("five grand"), is kept only as a question back to the owner.
 * The model can name no other field: autonomy, delegations, the environment and the connection are
 * not in its vocabulary (V-022), and a symbol counts only when the owner wrote it (V-038).
 *
 * Its reply is quoted model output: text only, never a button (PX-18), and withheld when it reads as
 * advice. Deterministic code renders every action.
 */

/** What the conversation last asked the owner, so the model knows what an answer answers. */
export type Asked = "money" | "goal" | "loss" | "symbols" | "model" | `param:${string}` | "check" | "review";

export interface OwnerMessage {
  id: string;
  text: string;
}

export interface CompilerInput {
  /** The owner's messages, oldest first. Only the last is read; the rest are context. */
  messages: readonly OwnerMessage[];
  asked: Asked;
  /** The models the owner may choose, by name, in the registry's order. */
  models: ReadonlyArray<{ id: ModelId; name: string }>;
}

/** A model call. Its answer is untrusted JSON, read only through `validateTurn`; a throw means the model did not answer. */
export type Compiler = (input: CompilerInput) => Promise<unknown>;

export type Reading =
  | { field: "money"; quote: string; value: Dec; check: boolean }
  | { field: "loss"; quote: string; loss: Loss; check: boolean }
  | { field: "goal"; quote: string }
  | { field: "symbols"; quote: string; symbols: string[] }
  | { field: "model"; quote: string; model: ModelId }
  | { field: "param"; quote: string; key: string; value: string };

export const INTENTS = ["answer", "explain", "advice", "other"] as const;
export type Intent = (typeof INTENTS)[number];

export interface Turn {
  readings: Reading[];
  /** Quotes of the owner's that no field can express. */
  notes: string[];
  intent: Intent;
  /** The model's reply, quoted as it wrote it, or null. */
  reply: string | null;
  /** The model replied, and the guard withheld the reply because it read as advice. */
  withheld: boolean;
  /** Readings the guard refused, and why. Never shown as values. */
  dropped: Array<{ field: string; why: string }>;
}

export type TurnResult = { ok: true; turn: Turn } | { ok: false; error: string };

export const SCHEMA_ERROR = "The model's answer did not match the compiler's schema.";
const MAX_QUOTE = 500;
const MAX_REPLY = 1000;
const AMOUNT_TEXT = /^(0|[1-9]\d{0,11})(\.\d{1,2})?$/;
const FRACTION_TEXT = /^(0|1)(\.\d{1,6})?$/;
const NUMBER_TEXT = /^\d{1,6}(\.\d{1,2})?$/;
const SYMBOL_TEXT = /^[A-Za-z]{1,5}(\.[A-Za-z])?$/;

/**
 * A reply that recommends, predicts or promises: the platform gives no advice (compliance, DEC-52),
 * so such a reply is withheld whatever the prompt said.
 */
const ADVICE =
  /\b(you should (?:buy|sell|hold|use|pick|choose|trade|invest)|i(?: would|'d)? (?:recommend|suggest)|my (?:pick|recommendation)s?|(?:best|top) (?:stocks?|picks?|models?|choices?)|will (?:make|earn|return|grow|double)|guaranteed|expected return|beat the market|can'?t lose|risk[- ]free)\b/i;

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const hasWord = (text: string, word: string) => new RegExp(`(^|[^A-Za-z.])${escape(word)}($|[^A-Za-z])`, "i").test(text);
const hasNumber = (text: string, value: string) => new RegExp(`(^|[^\\d.])${escape(value)}($|[^\\d])`).test(text);
const writtenInWords = (quote: string) => findAmounts(quote).length === 0 && findPercents(quote).length === 0;

type Checked = { ok: true; reading: Reading } | { ok: false; why: string };

function checkReading(raw: Record<string, unknown>, said: string): Checked {
  const quote = raw.quote;
  if (typeof quote !== "string" || quote.trim() === "" || quote.length > MAX_QUOTE) return { ok: false, why: "no quote" };
  if (!said.includes(quote)) return { ok: false, why: "the quote is not in the owner's message" };
  switch (raw.field) {
    case "money": {
      if (typeof raw.value !== "string" || !AMOUNT_TEXT.test(raw.value) || dec(raw.value) <= ZERO) return { ok: false, why: "not an amount" };
      const value = dec(raw.value);
      if (writtenInWords(quote)) return { ok: true, reading: { field: "money", quote, value, check: true } };
      if (!findAmounts(quote).some((m) => m.value === value)) return { ok: false, why: "the amount is not one the quote states" };
      return { ok: true, reading: { field: "money", quote, value, check: false } };
    }
    case "loss": {
      if (raw.unit !== "usd" && raw.unit !== "fraction") return { ok: false, why: "no unit" };
      const pattern = raw.unit === "usd" ? AMOUNT_TEXT : FRACTION_TEXT;
      if (typeof raw.value !== "string" || !pattern.test(raw.value)) return { ok: false, why: "not a loss" };
      const loss: Loss = { kind: raw.unit, value: dec(raw.value) };
      if (loss.value <= ZERO || (loss.kind === "fraction" && loss.value > ONE)) return { ok: false, why: "not a loss" };
      if (writtenInWords(quote)) return { ok: true, reading: { field: "loss", quote, loss, check: true } };
      const stated = loss.kind === "usd" ? findAmounts(quote) : findPercents(quote);
      if (!stated.some((m) => m.value === loss.value)) return { ok: false, why: "the loss is not one the quote states" };
      return { ok: true, reading: { field: "loss", quote, loss, check: false } };
    }
    case "goal":
      return { ok: true, reading: { field: "goal", quote } };
    case "symbols": {
      const symbols = raw.symbols;
      if (!Array.isArray(symbols) || symbols.length === 0) return { ok: false, why: "no symbols" };
      for (const s of symbols) {
        if (typeof s !== "string" || !SYMBOL_TEXT.test(s)) return { ok: false, why: "not a symbol" };
        if (!hasWord(quote, s)) return { ok: false, why: "a symbol the owner did not write" };
      }
      return { ok: true, reading: { field: "symbols", quote, symbols: (symbols as string[]).map((s) => s.toUpperCase()) } };
    }
    case "model": {
      const model = MODELS.find((m) => m.id === raw.model);
      if (!model) return { ok: false, why: "not a model in the registry" };
      if (!quote.toLowerCase().includes(model.name.toLowerCase()) && !quote.includes(model.id)) return { ok: false, why: "a model the owner did not name" };
      return { ok: true, reading: { field: "model", quote, model: model.id } };
    }
    case "param": {
      if (typeof raw.key !== "string" || !MODELS.some((m) => m.params.some((p) => p.key === raw.key))) return { ok: false, why: "not a model setting" };
      if (typeof raw.value !== "string" || !NUMBER_TEXT.test(raw.value) || !hasNumber(quote, raw.value)) return { ok: false, why: "a value the owner did not write" };
      return { ok: true, reading: { field: "param", quote, key: raw.key, value: raw.value } };
    }
    default:
      return { ok: false, why: "the compiler never sets this field (V-022, V-038)" };
  }
}

/**
 * The guard between a model's answer and the conversation. A malformed answer fails whole (brief A2's
 * error state); a reading that does not stand up is dropped and the rest are kept.
 */
export function validateTurn(raw: unknown, input: CompilerInput): TurnResult {
  const said = input.messages.at(-1)?.text ?? "";
  if (!isRecord(raw) || !Array.isArray(raw.readings) || !Array.isArray(raw.not_enforced)) return { ok: false, error: SCHEMA_ERROR };
  if (typeof raw.intent !== "string" || !(INTENTS as readonly string[]).includes(raw.intent)) return { ok: false, error: SCHEMA_ERROR };
  if (raw.reply !== null && (typeof raw.reply !== "string" || raw.reply.length > MAX_REPLY)) return { ok: false, error: SCHEMA_ERROR };

  const readings: Reading[] = [];
  const dropped: Turn["dropped"] = [];
  for (const r of raw.readings) {
    if (!isRecord(r)) return { ok: false, error: SCHEMA_ERROR };
    const checked = checkReading(r, said);
    if (checked.ok) readings.push(checked.reading);
    else dropped.push({ field: String(r.field), why: checked.why });
  }
  const notes = raw.not_enforced.filter((q): q is string => typeof q === "string" && q.trim() !== "" && q.length <= MAX_QUOTE && said.includes(q));
  const reply = typeof raw.reply === "string" && raw.reply.trim() !== "" ? raw.reply.trim() : null;
  const withheld = reply !== null && ADVICE.test(reply);
  return { ok: true, turn: { readings, notes, intent: raw.intent as Intent, reply: withheld ? null : reply, withheld, dropped } };
}

/** The JSON a compiler answers with, before the guard. */
interface RawTurn {
  readings: Array<Record<string, string | string[]>>;
  not_enforced: string[];
  intent: Intent;
  reply: string | null;
}

const ONES = ["one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
const SMALL = [...ONES, "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen"];
const TENS = ["twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
const SPOKEN = new RegExp(`\\b(a|${TENS.join("|")}(?:[\\s-](?:${ONES.join("|")}))?|${SMALL.join("|")})\\s+(hundred|thousand|grand|percent)\\b`, "i");

function spokenNumber(word: string): number {
  const [tens, ones] = word.toLowerCase().split(/[\s-]/);
  if (tens === "a") return 1;
  const t = TENS.indexOf(tens);
  if (t >= 0) return (t + 2) * 10 + (ones ? ONES.indexOf(ones) + 1 : 0);
  return SMALL.indexOf(tens) + 1;
}

/** A sum or share written in words, as a model would read it: "five grand", "ten percent". */
function spoken(text: string): Loss | null {
  const m = SPOKEN.exec(text);
  if (!m) return null;
  const n = fromInt(spokenNumber(m[1]));
  switch (m[2].toLowerCase()) {
    case "hundred":
      return { kind: "usd", value: mul(n, fromInt(100)) };
    case "percent":
      return { kind: "fraction", value: div(n, fromInt(100)) };
    default:
      return { kind: "usd", value: mul(n, fromInt(1000)) };
  }
}

const ASKS_FOR_ADVICE = /\b(should i|what should|which (?:one|model|stocks?|symbols?) should|recommend|suggest|what (?:stocks?|symbols?) (?:to|should))\b/i;
const QUESTION = /\?\s*$|^(what|how|why|explain|tell me)\b/i;
const NOT_TICKERS = new Set(["USD", "ETF", "ETFS", "ESG", "CEO", "AI", "US", "OK", "IPO", "PM", "AM", "ET"]);
const NOT_SYMBOLS = new Set(["A", "AN", "AND", "I", "IT", "ITS", "ONLY", "THE", "TO", "TRADE", "WANT", "JUST", "PLUS", "ALSO", "OR", "OF", "ON", "IN", "MAY", "CAN", "LET", "USE", "BUY", "ME", "MY", "SOME", "WITH", "FOR", "AT", "BE", "IS", "THEM", "THESE", "THOSE", "AS", "WELL", "LIKE", "OK", "YES", "NO"]);
const TRADE_WORDS = /\b(trade|trades|trading|symbols?|tickers?|stocks?|shares?)\b/i;

export const ADVICE_REPLY =
  "I can't tell you what to trade or which model to use. Those are your choices, and the platform makes none of them for you. I can explain how either model works, or what a limit does.";

const MANDATE_REPLY =
  "A mandate is the set of limits your agent must stay inside: how much money it may use, how much it may lose, what it may trade, and when it must ask you. The risk gate enforces every limit, whatever the agent proposes.";
const PAPER_REPLY = "Paper means simulated funds on your broker's paper account. No real money moves, and no real order is placed.";

/** An answer from the registry's own methodology text, or null when the question is about none of it. */
function explanation(text: string, models: CompilerInput["models"]): string | null {
  if (!QUESTION.test(text.trim())) return null;
  const lower = text.toLowerCase();
  const model = MODELS.find((m) => models.some((n) => n.id === m.id) && lower.includes(m.name.toLowerCase()));
  if (model) return `${model.name}: ${model.what} You set ${model.params.map((p) => p.label.toLowerCase()).join(" and ")}. ${model.params.map((p) => p.hint).join(" ")}`;
  const param = MODELS.flatMap((m) => m.params).find((p) => lower.includes(p.label.split(",")[0].toLowerCase()));
  if (param) return param.hint;
  if (/\bmandate\b/.test(lower)) return MANDATE_REPLY;
  if (/\bpaper\b/.test(lower)) return PAPER_REPLY;
  return null;
}

function tokens(text: string): string[] {
  return text
    .split(/[\s,;/&+]+/)
    .map((t) => t.replace(/^[("'“]+|[)"'”.!?:]+$/g, ""))
    .filter((t) => SYMBOL_TEXT.test(t));
}

function symbolsIn(text: string, asked: Asked): string[] {
  const words = tokens(text);
  const caps = words.filter((t) => t.length >= 2 && t === t.toUpperCase() && !NOT_TICKERS.has(t));
  if (asked === "symbols") return caps.length > 0 ? caps : words.filter((t) => !NOT_SYMBOLS.has(t.toUpperCase()));
  return TRADE_WORDS.test(text) ? caps : [];
}

const amountText = (v: Dec) => toDecimalString(v);

function lossReading(quote: string, loss: Loss): Record<string, string> {
  return { field: "loss", quote, value: amountText(loss.value), unit: loss.kind };
}

/**
 * How the fixture workspace reads a message, standing in for the model (DEC-473): fixed rules over the
 * owner's words, so the same words always read the same way. With `asModel` it also does what only a
 * model could, reading amounts written in words and answering questions from the registry's text;
 * without it, it reads figures only, as the conversation does when no model answers.
 */
function reading(input: CompilerInput, asModel: boolean): RawTurn {
  const text = input.messages.at(-1)?.text.trim() ?? "";
  const { asked } = input;
  const none = (intent: Intent, reply: string | null): RawTurn => ({ readings: [], not_enforced: [], intent, reply });
  if (asModel && ASKS_FOR_ADVICE.test(text)) return none("advice", ADVICE_REPLY);
  const explained = asModel ? explanation(text, input.models) : null;
  if (explained) return none("explain", explained);

  const readings: RawTurn["readings"] = [];
  const parts = clauses(text);
  const inWords = (c: string) => (asModel ? spoken(c) : null);
  const figures = (c: string) => findAmounts(c).length > 0 || findPercents(c).length > 0;
  const used = new Set<string>();

  if (asked.startsWith("param:")) {
    const value = /\d+(?:\.\d+)?/.exec(text)?.[0];
    if (value) readings.push({ field: "param", quote: text, key: asked.slice("param:".length), value });
  } else if (asked === "goal") {
    readings.push({ field: "goal", quote: text });
    used.add(text);
  } else {
    const lossClause = parts.find((c) => LOSS_WORDS.test(c) && (figures(c) || inWords(c))) ?? (asked === "loss" ? parts.find((c) => figures(c) || inWords(c)) : undefined);
    if (lossClause) {
      const stated = firstLoss(lossClause) ?? inWords(lossClause);
      if (stated) {
        readings.push(lossReading(lossClause, stated));
        used.add(lossClause);
      }
    }
    const dollars = (c: string) => c !== lossClause && findAmounts(c).some((m) => m.dollars);
    const moneyClause =
      parts.find((c) => dollars(c) && !GOAL_WORDS.test(c)) ??
      parts.find(dollars) ??
      (asked === "money" ? parts.find((c) => c !== lossClause && (findAmounts(c).length > 0 || inWords(c)?.kind === "usd")) : undefined);
    if (moneyClause) {
      const figure = findAmounts(moneyClause).find((m) => m.dollars) ?? findAmounts(moneyClause)[0];
      const value = figure ? figure.value : inWords(moneyClause)?.value;
      if (value) {
        readings.push({ field: "money", quote: moneyClause, value: amountText(value) });
        used.add(moneyClause);
      }
    }
    const goalClause =
      parts.find((c) => !used.has(c) && GOAL_WORDS.test(c) && figures(c)) ??
      parts.find((c) => c === moneyClause && GOAL_WORDS.test(c) && findAmounts(c).length + findPercents(c).length > 1) ??
      (asked === "money" ? parts.find((c) => !used.has(c) && GOAL_WORDS.test(c)) : undefined);
    if (goalClause) {
      readings.push({ field: "goal", quote: goalClause });
      used.add(goalClause);
    }
  }

  const symbols = asked.startsWith("param:") ? [] : symbolsIn(text, asked);
  if (symbols.length > 0) readings.push({ field: "symbols", quote: text, symbols });
  for (const m of input.models) {
    const at = text.toLowerCase().indexOf(m.name.toLowerCase());
    if (at >= 0) readings.push({ field: "model", quote: text.slice(at, at + m.name.length), model: m.id });
  }
  return { readings, not_enforced: constraintsIn(text), intent: readings.length > 0 ? "answer" : "other", reply: null };
}

/** How long the fixture model takes to answer, so the conversation shows it reading. */
export const FIXTURE_LATENCY_MS = 650;

/** The fixture workspace's model: no network, no key, the same answer for the same words. */
export function fixtureCompiler({ latencyMs = FIXTURE_LATENCY_MS }: { latencyMs?: number } = {}): Compiler {
  return (input) =>
    new Promise((resolve) => {
      const answer = reading(input, true);
      if (latencyMs <= 0) resolve(answer);
      else setTimeout(() => resolve(answer), latencyMs);
    });
}

/** Reading without the model (brief A2's degraded state): figures only, through the same guard. */
export const figuresOnly: Compiler = async (input) => reading(input, false);
