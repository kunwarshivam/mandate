import type { Provenance } from "@/fixtures/types";
import { type Dec, ONE, ZERO, add, dec, div, fromInt, min, mul, sub, toDecimalString } from "@/lib/decimal";
import { percent, usd } from "@/lib/format";

/**
 * The mandate-writing prototype's stand-in for the compiler (brief A0 to A5, mandate spec §7), on
 * fixture rules only: a fixed, deterministic reading of the owner's words, so the same answers always
 * draft the same mandate. Nothing here is a model call and nothing is stored or sent.
 *
 * What the owner states is `user_stated` with its quoted span. Every other envelope value is drafted
 * as `platform_proposed`, or `platform_default` where spec §7 allows a default. Never proposed: any
 * `auto`, a delegation, the instrument list, the environment or the connection (V-022, V-038).
 */

export interface Answers {
  money: string;
  goal: string;
  loss: string;
}

export type Source = { kind: "questions"; answers: Answers } | { kind: "description"; text: string };

export type SectionKey = "money" | "limits" | "autonomy" | "universe";

export const SECTION_KEYS: readonly SectionKey[] = ["money", "limits", "autonomy", "universe"];

export interface DraftField {
  /** The mandate field, as a JSON Pointer. */
  path: string;
  label: string;
  value: string;
  /** Null while the owner has not filled in a field only they may fill. */
  provenance: Provenance | null;
  /** The owner's own words, for a stated value. */
  quote?: string;
}

export interface DraftSection {
  key: SectionKey;
  title: string;
  lead: string;
  fields: DraftField[];
}

export interface NotEnforced {
  quote: string;
  why: string;
}

export type Goal = { type: "continuous" } | { type: "profit_stop"; level: Dec; quote: string };

/** The envelope's numbers, as decimals: money in dollars, limits as fractions of the allocation. */
export interface Terms {
  allocationUsd: Dec;
  maxLossUsd: Dec;
  maxLossFraction: Dec;
  goal: Goal;
  maxDailyLoss: Dec;
  maxDrawdown: Dec;
  stopDistance: Dec;
  maxPositionFraction: Dec;
  maxPositionUsd: Dec;
  maxOrderUsd: Dec;
  maxOrdersPerDay: number;
  maxGrossExposureUsd: Dec;
  approvalTimeoutS: number;
  autonomyDefault: "ask" | "deny" | "auto";
  autonomyRules: ReadonlyArray<{ id: string; then: "ask" | "deny" | "auto" }>;
}

export interface Figures {
  positionLossAtStop: Dec;
  dailyLossBudget: Dec;
  flattenLoss: Dec;
  floorLoss: Dec;
  /** What could trade without asking once confirmed (DEC-189). */
  unasked: Dec;
}

export interface Draft {
  terms: Terms;
  symbols: string[];
  answers: Array<{ label: string; quote: string }>;
  sections: DraftSection[];
  figures: Figures;
  notEnforced: NotEnforced[];
}

/**
 * The retail profile's lifetime-loss ceiling (mandate spec §4.3: DEC-61's placeholder until counsel
 * answers). Shown as a limit, never filled in as an answer.
 */
export const LOSS_CEILING = dec("0.2");

/** DEC-117: the platform proposes five instruments at most. */
const PROPOSED_MAX_INSTRUMENTS = 5;
const MAX_INSTRUMENTS = 20;
const STOP_DISTANCE = dec("0.08");
const POSITION_CAP = dec("0.25");
const ORDER_FRACTION = dec("0.1");
const ORDERS_PER_DAY = 10;
const APPROVAL_TIMEOUT_S = 300;

/** Truncates a non-negative decimal to `places`, so a drafted limit never rounds up past the rule it came from. */
export function floorTo(value: Dec, places: number): Dec {
  const unit = 10n ** BigInt(12 - places);
  return (value / unit) * unit;
}

const cents = (value: Dec) => floorTo(value, 2);

interface Match {
  value: Dec;
  span: string;
  index: number;
  /** Written as dollars ("$", "dollars", "k"), not a bare figure such as a year. */
  dollars: boolean;
}

/** Amounts in figures: "$5,000", "5000.50", "5k". A figure followed by "%" or "percent" is a percentage, not an amount. */
const AMOUNT = /(\$\s?)?(\d{1,3}(?:,\d{3})+|\d+)(?:\.(\d{1,2}))?(\s?(?:k|thousand)\b)?(\s?(?:dollars?|usd)\b)?(?!\d|[.,]\d|\s?(?:%|percent\b))/gi;
const PERCENT = /(\d+(?:\.\d+)?)\s?(?:%|percent\b)/gi;

export function findAmounts(text: string): Match[] {
  return Array.from(text.matchAll(AMOUNT), (m) => {
    const whole = dec(`${m[2].replaceAll(",", "")}${m[3] ? `.${m[3]}` : ""}`);
    return { value: m[4] ? mul(whole, fromInt(1000)) : whole, span: m[0].trim(), index: m.index, dollars: Boolean(m[1] || m[4] || m[5]) };
  });
}

export function findPercents(text: string): Match[] {
  return Array.from(text.matchAll(PERCENT), (m) => ({ value: div(dec(m[1]), fromInt(100)), span: m[0].trim(), index: m.index, dollars: false }));
}

/** A loss in dollars or as a percentage of the money, whichever the owner wrote first. */
type Loss = { kind: "usd"; value: Dec } | { kind: "fraction"; value: Dec };

function firstLoss(text: string): Loss | null {
  const amount = findAmounts(text)[0];
  const share = findPercents(text)[0];
  if (share && (!amount || share.index <= amount.index)) return { kind: "fraction", value: share.value };
  if (amount) return { kind: "usd", value: amount.value };
  return null;
}

/** Words that state a constraint no mandate field can express (spec §7, "not enforced"). */
const CONSTRAINT =
  /\b(avoid|avoiding|never|don['’]?t|do not|except|ethical|sustainable|esg|news|earnings|announcements?|macro|fed|weekends?|overnight|dividends?|tax|taxes|steady|steadily|slowly|carefully|gently|retire|retirement|boring|calm)\b/i;

const LOSS_WORDS = /\b(lose|losing|loss|losses|drop|down)\b/i;
const GOAL_WORDS = /\b(stop|stops|reach|reaches|until|gain|gains|grow|grows|make|makes|up|target|double)\b/i;

function clauses(text: string): string[] {
  return text
    .split(/(?<=[.;!?])\s+|\n+|,\s+|\s+(?:and|but)\s+/i)
    .map((c) => c.trim())
    .filter((c) => c.length > 0);
}

function constraintsIn(text: string): NotEnforced[] {
  return clauses(text)
    .filter((c) => CONSTRAINT.test(c))
    .map((quote) => ({ quote, why: "No limit in the mandate can check this, so nothing enforces it. It reaches the agent's models only as description text." }));
}

export type CheckResult<T> = { ok: true; value: T } | { ok: false; error: string };

export function readMoney(text: string): CheckResult<Dec> {
  const amount = findAmounts(text)[0];
  if (!amount) return { ok: false, error: "Write the amount in figures, in dollars." };
  if (amount.value <= ZERO) return { ok: false, error: "The amount must be more than zero." };
  return { ok: true, value: amount.value };
}

export function readGoal(text: string): CheckResult<string> {
  if (text.trim() === "") return { ok: false, error: "Say what this agent is for, in your own words." };
  return { ok: true, value: text.trim() };
}

/** The ceiling in dollars for a given allocation, for the hint beside the loss question. */
export function lossCeilingUsd(allocation: Dec): Dec {
  return cents(mul(allocation, LOSS_CEILING));
}

export function readLoss(text: string, allocation: Dec): CheckResult<{ usd: Dec; fraction: Dec }> {
  const loss = firstLoss(text);
  if (!loss) return { ok: false, error: "Write it in figures: dollars, or a percentage of the money." };
  const usdValue = loss.kind === "usd" ? loss.value : cents(mul(allocation, loss.value));
  const fraction = loss.kind === "fraction" ? loss.value : div(loss.value, allocation);
  if (usdValue <= ZERO) return { ok: false, error: "The loss must be more than zero." };
  if (fraction > ONE) return { ok: false, error: `That is more than the ${usd(allocation)} this agent may use.` };
  if (fraction > LOSS_CEILING) {
    return {
      ok: false,
      error: `That is more than this workspace allows: at most ${percent(LOSS_CEILING, 0)} of the money, ${usd(lossCeilingUsd(allocation))}. Change your answer to continue.`,
    };
  }
  return { ok: true, value: { usd: usdValue, fraction } };
}

/** A level stated in the goal: a percentage gain, or dollars (above the allocation, an equity level; at or below it, a gain). */
function readLevel(text: string, allocation: Dec, skip?: Match): Goal | null {
  const share = findPercents(text)[0];
  const amount = findAmounts(text).find((m) => m.index !== skip?.index);
  if (share && (!amount || share.index <= amount.index)) return share.value > ZERO ? { type: "profit_stop", level: share.value, quote: share.span } : null;
  if (!amount || amount.value <= ZERO) return null;
  const gain = amount.value > allocation ? sub(amount.value, allocation) : amount.value;
  return { type: "profit_stop", level: div(gain, allocation), quote: amount.span };
}

export type Read = { allocation: Dec; loss: { usd: Dec; fraction: Dec }; goal: Goal; answers: Draft["answers"]; quotes: Record<"money" | "loss", string>; notEnforced: NotEnforced[] };

/** The three answers, from the questions or from a description; an error names what is missing. */
export function read(source: Source): CheckResult<Read> {
  switch (source.kind) {
    case "questions": {
      const { money, goal, loss } = source.answers;
      const allocation = readMoney(money);
      if (!allocation.ok) return allocation;
      const goalText = readGoal(goal);
      if (!goalText.ok) return goalText;
      const lossValue = readLoss(loss, allocation.value);
      if (!lossValue.ok) return lossValue;
      const level = readLevel(goal, allocation.value);
      const notEnforced = level
        ? constraintsIn(goal)
        : [{ quote: goal.trim(), why: "No goal type can express this, so it is kept as description text and nothing enforces it. The agent runs until you stop it." }];
      return {
        ok: true,
        value: {
          allocation: allocation.value,
          loss: lossValue.value,
          goal: level ?? { type: "continuous" },
          answers: [
            { label: "Money", quote: money.trim() },
            { label: "Goal", quote: goal.trim() },
            { label: "Loss you can stand", quote: loss.trim() },
          ],
          quotes: { money: money.trim(), loss: loss.trim() },
          notEnforced,
        },
      };
    }
    case "description": {
      const text = source.text.trim();
      if (text === "") return { ok: false, error: "Write a description first." };
      const parts = clauses(text);
      const lossClause = parts.find((c) => LOSS_WORDS.test(c) && (findAmounts(c).length > 0 || findPercents(c).length > 0));
      const moneyClause = parts.find((c) => c !== lossClause && findAmounts(c).some((m) => m.dollars)) ?? parts.find((c) => c !== lossClause && findAmounts(c).length > 0);
      const missing = [moneyClause ? null : "how much money it may use, in dollars", lossClause ? null : "how much it may lose, in dollars or as a percentage"].filter(Boolean);
      if (!moneyClause || !lossClause) return { ok: false, error: `We could not find ${missing.join(", or ")} in your words. Add it, or answer the three questions instead.` };
      const amounts = findAmounts(moneyClause);
      const allocation = amounts.find((m) => m.dollars) ?? amounts[0];
      const lossValue = readLoss(lossClause, allocation.value);
      if (!lossValue.ok) return lossValue;
      const goalClause = parts.find((c) => c !== lossClause && GOAL_WORDS.test(c) && readLevel(c, allocation.value, c === moneyClause ? allocation : undefined));
      const goal = goalClause ? readLevel(goalClause, allocation.value, goalClause === moneyClause ? allocation : undefined) : null;
      return {
        ok: true,
        value: {
          allocation: allocation.value,
          loss: lossValue.value,
          goal: goal ?? { type: "continuous" },
          answers: [{ label: "Your description", quote: text }],
          quotes: { money: moneyClause, loss: lossClause },
          notEnforced: constraintsIn(text),
        },
      };
    }
    default: {
      const unhandled: never = source;
      throw new Error(`unhandled source ${JSON.stringify(unhandled)}`);
    }
  }
}

const SYMBOL = /^[A-Z]{1,5}(\.[A-Z])?$/;

/** The owner's instrument list: upper case, sorted and unique (V-009). The platform never fills it in (V-038). */
export function readSymbols(text: string): CheckResult<string[]> {
  const words = text
    .split(/[\s,]+/)
    .map((w) => w.trim().toUpperCase())
    .filter((w) => w.length > 0);
  const bad = words.filter((w) => !SYMBOL.test(w));
  if (bad.length > 0) return { ok: false, error: `Not a stock symbol: ${bad.join(", ")}.` };
  const symbols = [...new Set(words)].sort();
  if (symbols.length > MAX_INSTRUMENTS) return { ok: false, error: `At most ${MAX_INSTRUMENTS} symbols.` };
  return { ok: true, value: symbols };
}

/**
 * The unasked dollars (DEC-189): the order value the agent could still submit today without an ask.
 * It is bounded by the `auto` paths; with one, by `max_orders_per_day` × `max_order_usd` and the
 * gross-exposure headroom (a new agent holds nothing, so the whole limit). A new mandate has no
 * delegations. With no `auto` path, nothing that adds risk runs unasked, so the figure is zero.
 */
export function unaskedUsd(terms: Terms): Dec {
  const autoPaths = terms.autonomyRules.filter((r) => r.then === "auto").length + (terms.autonomyDefault === "auto" ? 1 : 0);
  if (autoPaths === 0) return ZERO;
  return min(mul(fromInt(terms.maxOrdersPerDay), terms.maxOrderUsd), terms.maxGrossExposureUsd);
}

/**
 * Proposed limits, derived from the stated loss so they fit inside it: a day may lose a quarter of
 * the lifetime loss, the agent flattens at half of it, and one position's loss at its stop never
 * exceeds the daily budget (W-002 cannot fire).
 */
function termsFor(r: Read): Terms {
  const a = r.allocation;
  const maxDailyLoss = floorTo(div(r.loss.fraction, fromInt(4)), 4);
  const maxDrawdown = floorTo(div(r.loss.fraction, fromInt(2)), 4);
  const maxPositionFraction = min(POSITION_CAP, floorTo(div(maxDailyLoss, STOP_DISTANCE), 4));
  const maxPositionUsd = cents(mul(a, maxPositionFraction));
  return {
    allocationUsd: a,
    maxLossUsd: r.loss.usd,
    maxLossFraction: r.loss.fraction,
    goal: r.goal,
    maxDailyLoss,
    maxDrawdown,
    stopDistance: STOP_DISTANCE,
    maxPositionFraction,
    maxPositionUsd,
    maxOrderUsd: min(cents(mul(a, ORDER_FRACTION)), maxPositionUsd),
    maxOrdersPerDay: ORDERS_PER_DAY,
    maxGrossExposureUsd: a,
    approvalTimeoutS: APPROVAL_TIMEOUT_S,
    autonomyDefault: "ask",
    autonomyRules: [],
  };
}

export function figuresFor(t: Terms): Figures {
  return {
    positionLossAtStop: mul(t.maxPositionUsd, t.stopDistance),
    dailyLossBudget: mul(t.allocationUsd, t.maxDailyLoss),
    flattenLoss: mul(t.allocationUsd, t.maxDrawdown),
    floorLoss: t.maxLossUsd,
    unasked: unaskedUsd(t),
  };
}

export function goalWords(t: Terms): string {
  switch (t.goal.type) {
    case "continuous":
      return "Runs until you stop it.";
    case "profit_stop":
      return `Stops when its equity reaches ${usd(mul(t.allocationUsd, add(ONE, t.goal.level)))}, ${percent(t.goal.level)} above the money it may use. Then it sells what it holds and retires.`;
    default: {
      const unhandled: never = t.goal;
      throw new Error(`unhandled goal ${JSON.stringify(unhandled)}`);
    }
  }
}

function sectionsFor(t: Terms, r: Read, symbols: string[], figures: Figures): DraftSection[] {
  const goal: DraftField =
    t.goal.type === "profit_stop"
      ? { path: "/goal/profit_level", label: "Goal", value: goalWords(t), provenance: "user_stated", quote: t.goal.quote }
      : { path: "/goal/end_date", label: "Goal", value: goalWords(t), provenance: "platform_proposed" };
  return [
    {
      key: "money",
      title: "Money and goal",
      lead: "Your three answers, as the mandate reads them.",
      fields: [
        { path: "/capital/allocation_usd", label: "Money it may use", value: usd(t.allocationUsd), provenance: "user_stated", quote: r.quotes.money },
        {
          path: "/capital/max_loss_from_allocation",
          label: "Most it may lose, in total",
          value: `${usd(t.maxLossUsd)}, ${percent(t.maxLossFraction)} of the money. At ${usd(sub(t.allocationUsd, t.maxLossUsd))} it closes everything and pauses until you change the mandate.`,
          provenance: "user_stated",
          quote: r.quotes.loss,
        },
        goal,
        { path: "/environment", label: "Where it runs", value: "Paper: simulated funds, no real money.", provenance: "platform_default" },
      ],
    },
    {
      key: "limits",
      title: "Limits",
      lead: "Drafted to fit inside the loss you stated. The risk gate enforces each one, whatever the agent proposes.",
      fields: [
        {
          path: "/risk/max_daily_loss",
          label: "Loss in one day",
          value: `${usd(figures.dailyLossBudget)}, ${percent(t.maxDailyLoss, 2)} of the day's starting equity. Past it, exits only until the next day.`,
          provenance: "platform_proposed",
        },
        {
          path: "/risk/max_drawdown",
          label: "Fall from its highest point",
          value: `${usd(figures.flattenLoss)}, ${percent(t.maxDrawdown, 2)}. Past it, it closes everything and pauses.`,
          provenance: "platform_proposed",
        },
        { path: "/risk/max_position_usd", label: "Largest position", value: `${usd(t.maxPositionUsd)}, ${percent(t.maxPositionFraction, 2)} of the money.`, provenance: "platform_proposed" },
        { path: "/risk/max_order_usd", label: "Largest order", value: usd(t.maxOrderUsd), provenance: "platform_proposed" },
        { path: "/risk/max_orders_per_day", label: "Orders in a day", value: `At most ${t.maxOrdersPerDay}.`, provenance: "platform_proposed" },
        { path: "/risk/max_gross_exposure_usd", label: "Total holdings", value: `At most ${usd(t.maxGrossExposureUsd)}.`, provenance: "platform_proposed" },
        {
          path: "/protection/stop_distance",
          label: "Protective stop",
          value: `A resting stop ${percent(t.stopDistance, 0)} below what it paid, on every position. One full position stopped out loses about ${usd(figures.positionLossAtStop)}.`,
          provenance: "platform_proposed",
        },
        { path: "/risk/scale_action", label: "When sizes are scaled down", value: "Limits new buys only. It does not sell to shrink a position.", provenance: "platform_proposed" },
      ],
    },
    {
      key: "autonomy",
      title: "When it asks you",
      lead: "Selling to cut risk never asks: protective stops, the loss limits and the kill switch act at once.",
      fields: [
        { path: "/autonomy/default", label: "Buying", value: "Asks you before every buy.", provenance: "platform_default" },
        { path: "/autonomy/approval/timeout_s", label: "Time to answer", value: `${t.approvalTimeoutS / 60} minutes.`, provenance: "platform_proposed" },
        { path: "/autonomy/approval/on_timeout", label: "If you do not answer", value: "The buy is skipped. Silence never buys.", provenance: "platform_default" },
        { path: "/autonomy/approval/approvers", label: "Who answers", value: "You.", provenance: "platform_default" },
      ],
    },
    {
      key: "universe",
      title: "What it may trade",
      lead: "You choose the instruments. The platform never fills in your list.",
      fields: [
        symbols.length > 0
          ? { path: "/universe/pinned_instruments", label: "Instruments", value: symbols.join(", "), provenance: "user_entered" }
          : { path: "/universe/pinned_instruments", label: "Instruments", value: "None yet.", provenance: null },
        { path: "/universe/asset_classes", label: "Kinds of instrument", value: "US stocks and ETFs.", provenance: "platform_proposed" },
        { path: "/universe/max_instruments", label: "Most instruments at once", value: String(Math.max(PROPOSED_MAX_INSTRUMENTS, symbols.length)), provenance: "platform_proposed" },
        { path: "/universe/leveraged_etps_enabled", label: "Leveraged and inverse ETFs", value: "Off.", provenance: "platform_default" },
      ],
    },
  ];
}

export function compile(r: Read, symbols: string[]): Draft {
  const terms = termsFor(r);
  const figures = figuresFor(terms);
  return { terms, symbols, answers: r.answers, sections: sectionsFor(terms, r, symbols, figures), figures, notEnforced: r.notEnforced };
}

/**
 * A stand-in for the version hash: FNV-1a (64-bit) over the drafted values. It names this draft on
 * the prototype's record screen; no version exists anywhere.
 */
export function draftDigest(draft: Draft): string {
  const t = draft.terms;
  const canonical = JSON.stringify([
    toDecimalString(t.allocationUsd),
    toDecimalString(t.maxLossUsd),
    t.goal.type === "profit_stop" ? toDecimalString(t.goal.level) : null,
    toDecimalString(t.maxDailyLoss),
    toDecimalString(t.maxDrawdown),
    toDecimalString(t.maxPositionUsd),
    toDecimalString(t.maxOrderUsd),
    draft.symbols,
  ]);
  let hash = 0xcbf29ce484222325n;
  for (const byte of new TextEncoder().encode(canonical)) {
    hash ^= BigInt(byte);
    hash = (hash * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return hash.toString(16).padStart(16, "0");
}
