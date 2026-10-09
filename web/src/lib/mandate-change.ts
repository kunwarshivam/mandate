/**
 * A new version of a running agent's mandate, as the deployment would make one: the owner's edits
 * become a proposed document, diffed against the version in effect, each changed path classified
 * (mandate spec §9.2), validated (§4.1 and the retail profile of §4.3), and, once the owner confirms
 * it, applied the way §2.2 and §5.1 say. The Edit form and a message in a thread both end here, so
 * a change reads, checks and applies the same way wherever the owner asked for it. Nothing here is
 * the risk gate: the deployment repeats every check when it applies a version.
 */
import type {
  Agent,
  ChangeClass,
  ContentRef,
  FieldProvenance,
  Iso,
  Mandate,
  MandateChange,
  MandateVersionRecord,
  PastOrder,
  RestrictionCode,
  TimelineEvent,
  TimelineKind,
  WorkingOrder,
  Workspace,
} from "@/fixtures/types";
import { LOSS_CEILING } from "@/components/new-agent/draft";
import { type Dec, ONE, ZERO, abs, add, dec, div, fromInt, mul, sub, toDecimalString } from "./decimal";
import { mandateVersion, unallocatedUsd } from "./fixture-journey";
import { fixtureUlid } from "./fixture-ids";
import { percent, price, quantity, seconds, usd } from "./format";
import { changeLabel, changeValue } from "./mandate-paths";

export const EDIT_PATHS = [
  "/capital/allocation_usd",
  "/capital/max_loss_from_allocation",
  "/risk/max_daily_loss",
  "/risk/max_order_usd",
  "/risk/max_position_usd",
  "/risk/max_position_fraction",
  "/risk/max_gross_exposure_usd",
  "/risk/max_orders_per_day",
  "/protection/stop_distance",
  "/autonomy/approval/timeout_s",
  "/autonomy/approval/two_approver_above_usd",
  "/notifications/quiet_hours/start",
  "/notifications/quiet_hours/end",
] as const;

export type EditPath = (typeof EDIT_PATHS)[number];
export type FieldValue = string | number | null;
export type Edits = Partial<Record<EditPath, FieldValue>>;

/** How the owner writes a value: dollars, a percentage, a whole number, minutes, or a 24-hour time. */
export type Unit = "usd" | "percent" | "count" | "minutes" | "time";

export type FieldGroup = "capital" | "size" | "protection" | "approvals" | "quiet";

export interface EditableField {
  path: EditPath;
  unit: Unit;
  group: FieldGroup;
  /** Left empty, the field is not set: only the two-approver threshold may be. */
  optional: boolean;
}

const FIELD: Record<EditPath, Omit<EditableField, "path">> = {
  "/capital/allocation_usd": { unit: "usd", group: "capital", optional: false },
  "/capital/max_loss_from_allocation": { unit: "percent", group: "capital", optional: false },
  "/risk/max_daily_loss": { unit: "percent", group: "capital", optional: false },
  "/risk/max_order_usd": { unit: "usd", group: "size", optional: false },
  "/risk/max_position_usd": { unit: "usd", group: "size", optional: false },
  "/risk/max_position_fraction": { unit: "percent", group: "size", optional: false },
  "/risk/max_gross_exposure_usd": { unit: "usd", group: "size", optional: false },
  "/risk/max_orders_per_day": { unit: "count", group: "size", optional: false },
  "/protection/stop_distance": { unit: "percent", group: "protection", optional: false },
  "/autonomy/approval/timeout_s": { unit: "minutes", group: "approvals", optional: false },
  "/autonomy/approval/two_approver_above_usd": { unit: "usd", group: "approvals", optional: true },
  "/notifications/quiet_hours/start": { unit: "time", group: "quiet", optional: false },
  "/notifications/quiet_hours/end": { unit: "time", group: "quiet", optional: false },
};

export function fieldFor(path: EditPath): EditableField {
  return { path, ...FIELD[path] };
}

/**
 * The fields this mandate lets the owner edit here. Instruments, signal models, rules, the goal,
 * the ladder, the environment and the connection are not among them; quiet hours only when set.
 */
export function editableFields(m: Mandate): EditableField[] {
  return EDIT_PATHS.filter((p) => m.notifications.quiet_hours !== null || FIELD[p].group !== "quiet").map(fieldFor);
}

export function valueAt(m: Mandate, path: EditPath): FieldValue {
  switch (path) {
    case "/capital/allocation_usd":
      return m.capital.allocation_usd;
    case "/capital/max_loss_from_allocation":
      return m.capital.max_loss_from_allocation;
    case "/risk/max_daily_loss":
      return m.risk.max_daily_loss;
    case "/risk/max_order_usd":
      return m.risk.max_order_usd;
    case "/risk/max_position_usd":
      return m.risk.max_position_usd;
    case "/risk/max_position_fraction":
      return m.risk.max_position_fraction;
    case "/risk/max_gross_exposure_usd":
      return m.risk.max_gross_exposure_usd;
    case "/risk/max_orders_per_day":
      return m.risk.max_orders_per_day;
    case "/protection/stop_distance":
      return m.protection.stop_distance;
    case "/autonomy/approval/timeout_s":
      return m.autonomy.approval.timeout_s;
    case "/autonomy/approval/two_approver_above_usd":
      return m.autonomy.approval.two_approver_above_usd;
    case "/notifications/quiet_hours/start":
      return m.notifications.quiet_hours?.start ?? null;
    case "/notifications/quiet_hours/end":
      return m.notifications.quiet_hours?.end ?? null;
    default: {
      const unhandled: never = path;
      throw new Error(`unhandled path ${String(unhandled)}`);
    }
  }
}

const text = (v: FieldValue): string => {
  if (typeof v !== "string") throw new Error(`expected a string, got ${String(v)}`);
  return v;
};
const whole = (v: FieldValue): number => {
  if (typeof v !== "number" || !Number.isInteger(v)) throw new Error(`expected a whole number, got ${String(v)}`);
  return v;
};

function put(m: Mandate, path: EditPath, v: FieldValue): void {
  switch (path) {
    case "/capital/allocation_usd":
      m.capital.allocation_usd = text(v);
      return;
    case "/capital/max_loss_from_allocation":
      m.capital.max_loss_from_allocation = text(v);
      return;
    case "/risk/max_daily_loss":
      m.risk.max_daily_loss = text(v);
      return;
    case "/risk/max_order_usd":
      m.risk.max_order_usd = text(v);
      return;
    case "/risk/max_position_usd":
      m.risk.max_position_usd = text(v);
      return;
    case "/risk/max_position_fraction":
      m.risk.max_position_fraction = text(v);
      return;
    case "/risk/max_gross_exposure_usd":
      m.risk.max_gross_exposure_usd = text(v);
      return;
    case "/risk/max_orders_per_day":
      m.risk.max_orders_per_day = whole(v);
      return;
    case "/protection/stop_distance":
      m.protection.stop_distance = text(v);
      return;
    case "/autonomy/approval/timeout_s":
      m.autonomy.approval.timeout_s = whole(v);
      return;
    case "/autonomy/approval/two_approver_above_usd":
      m.autonomy.approval.two_approver_above_usd = v === null ? null : text(v);
      return;
    case "/notifications/quiet_hours/start":
    case "/notifications/quiet_hours/end": {
      const hours = m.notifications.quiet_hours;
      if (!hours) throw new Error("quiet hours are not set, so they cannot be edited");
      if (path.endsWith("start")) hours.start = text(v);
      else hours.end = text(v);
      return;
    }
    default: {
      const unhandled: never = path;
      throw new Error(`unhandled path ${String(unhandled)}`);
    }
  }
}

export type Parsed = { ok: true; value: FieldValue } | { ok: false; error: string };

const MONEY = /^\$?\s?(\d{1,3}(?:,\d{3})+|\d+)(?:\.(\d{1,2}))?$/;
const SHARE = /^(\d+)(?:\.(\d{1,2}))?\s?%?$/;
const TIME = /^([01]?\d|2[0-3]):([0-5]\d)$/;

/** The owner's words for one field, in the field's unit, as the document's value. */
export function readInput(field: EditableField, raw: string): Parsed {
  const input = raw.trim();
  if (input === "") return field.optional ? { ok: true, value: null } : { ok: false, error: "Enter a value." };
  switch (field.unit) {
    case "usd": {
      const m = MONEY.exec(input);
      if (!m) return { ok: false, error: "Write an amount in dollars, in figures, like 1,200." };
      const value = dec(`${m[1].replaceAll(",", "")}${m[2] ? `.${m[2]}` : ""}`);
      if (value <= ZERO) return { ok: false, error: "The amount must be more than zero." };
      return { ok: true, value: toDecimalString(value) };
    }
    case "percent": {
      const m = SHARE.exec(input);
      if (!m) return { ok: false, error: "Write a percentage in figures, like 2 or 2.5." };
      const value = div(dec(`${m[1]}${m[2] ? `.${m[2]}` : ""}`), fromInt(100));
      if (value <= ZERO) return { ok: false, error: "The percentage must be more than zero." };
      return { ok: true, value: toDecimalString(value) };
    }
    case "count": {
      if (!/^\d+$/.test(input.replaceAll(",", ""))) return { ok: false, error: "Write a whole number, like 20." };
      return { ok: true, value: Number(input.replaceAll(",", "")) };
    }
    case "minutes": {
      if (!/^\d+$/.test(input)) return { ok: false, error: "Write whole minutes, like 10." };
      return { ok: true, value: Number(input) * 60 };
    }
    case "time": {
      const m = TIME.exec(input);
      if (!m) return { ok: false, error: "Write a 24-hour time, like 22:30." };
      return { ok: true, value: `${m[1].padStart(2, "0")}:${m[2]}` };
    }
    default: {
      const unhandled: never = field.unit;
      throw new Error(`unhandled unit ${String(unhandled)}`);
    }
  }
}

/** A document value as the owner would type it back: the inverse of `readInput`. */
export function inputText(field: EditableField, value: FieldValue): string {
  if (value === null) return "";
  switch (field.unit) {
    case "usd":
    case "time":
    case "count":
      return String(value);
    case "percent":
      return toDecimalString(mul(dec(String(value)), fromInt(100)));
    case "minutes":
      return toDecimalString(div(fromInt(Number(value)), fromInt(60)));
    default: {
      const unhandled: never = field.unit;
      throw new Error(`unhandled unit ${String(unhandled)}`);
    }
  }
}

/** §9.2 "Maximums": a larger value is increasing, anything else reducing. */
const MAXIMUMS: ReadonlySet<EditPath> = new Set([
  "/capital/allocation_usd",
  "/capital/max_loss_from_allocation",
  "/risk/max_daily_loss",
  "/risk/max_order_usd",
  "/risk/max_position_usd",
  "/risk/max_position_fraction",
  "/risk/max_gross_exposure_usd",
  "/risk/max_orders_per_day",
  "/protection/stop_distance",
]);

const num = (v: FieldValue): Dec => dec(String(v));

/** One changed path, classified by its §9.2 row. */
export function classifyPath(path: EditPath, from: FieldValue, to: FieldValue): ChangeClass {
  if (MAXIMUMS.has(path)) return num(to) > num(from) ? "risk_increasing" : "risk_reducing";
  switch (path) {
    case "/autonomy/approval/two_approver_above_usd":
      return to !== null && (from === null || num(to) < num(from)) ? "risk_reducing" : "risk_increasing";
    case "/autonomy/approval/timeout_s":
      return "risk_increasing";
    case "/notifications/quiet_hours/start":
    case "/notifications/quiet_hours/end":
      return "neutral";
    default:
      throw new Error(`no §9.2 row for ${path}`);
  }
}

/** A version is risk-increasing if any path is, else reducing if any is, else neutral. */
export function overallClass(changes: MandateChange[]): ChangeClass | null {
  if (changes.length === 0) return null;
  if (changes.some((c) => c.classification === "risk_increasing")) return "risk_increasing";
  if (changes.some((c) => c.classification === "risk_reducing")) return "risk_reducing";
  return "neutral";
}

export interface Diff {
  mandate: Mandate;
  version: ContentRef;
  changes: MandateChange[];
  classification: ChangeClass | null;
  /** §9.2: a risk-increasing version is confirmed with a passkey. */
  stepUp: boolean;
}

/** Equal as values, so `1000.0` against `1000` is no change and leaves the document, and its hash, as they were. */
function sameValue(path: EditPath, a: FieldValue, b: FieldValue): boolean {
  if (a === null || b === null) return a === b;
  switch (FIELD[path].unit) {
    case "usd":
    case "percent":
      return num(a) === num(b);
    case "count":
    case "minutes":
    case "time":
      return a === b;
    default: {
      const unhandled: never = FIELD[path].unit;
      throw new Error(`unhandled unit ${String(unhandled)}`);
    }
  }
}

/** The edited document and its diff against `before`, paths sorted, unchanged edits dropped. */
export function diffMandate(before: Mandate, edits: Edits): Diff {
  const mandate = structuredClone(before);
  for (const path of EDIT_PATHS) if (path in edits && !sameValue(path, valueAt(before, path), edits[path] ?? null)) put(mandate, path, edits[path] ?? null);
  const changes = EDIT_PATHS.filter((p) => valueAt(before, p) !== valueAt(mandate, p))
    .sort()
    .map((path): MandateChange => {
      const from = valueAt(before, path);
      const to = valueAt(mandate, path);
      return { path, from, to, classification: classifyPath(path, from, to) };
    });
  const classification = overallClass(changes);
  return { mandate, version: mandateVersion(mandate), changes, classification, stepUp: classification === "risk_increasing" };
}

/** A rule the proposed version breaks, by its code in the spec, in the owner's words. */
export interface Refusal {
  rule: string;
  text: string;
}

/** The retail profile's approval-window minimum (§4.3), DEC-61's placeholder, beside its lifetime-loss ceiling. */
export const RETAIL_MIN_TIMEOUT_S = 120;
const MAX_TIMEOUT_S = 86_400;
const MAX_ORDERS_PER_DAY = 10_000;

export interface ValidationContext {
  /** Account equity less every other active agent's allocation (V-002). */
  roomUsd: Dec;
  approverUsers: number;
}

/** The V-rules and limits the editable fields can break, every one reported (§4). */
export function validate(m: Mandate, ctx: ValidationContext): Refusal[] {
  const out: Refusal[] = [];
  const r = m.risk;
  const share = (path: EditPath, value: string, closedAtOne: boolean) => {
    const v = dec(value);
    if (v >= ONE && !(closedAtOne && v === ONE)) out.push({ rule: "schema", text: `${changeLabel(path)} must be ${closedAtOne ? "at most" : "less than"} 100%.` });
  };
  share("/capital/max_loss_from_allocation", m.capital.max_loss_from_allocation, false);
  share("/risk/max_daily_loss", r.max_daily_loss, false);
  share("/protection/stop_distance", m.protection.stop_distance, false);
  share("/risk/max_position_fraction", r.max_position_fraction, true);
  if (dec(m.capital.max_loss_from_allocation) > LOSS_CEILING) {
    out.push({ rule: "§4.3", text: `This workspace allows a lifetime loss limit of at most ${percent(LOSS_CEILING, 0)} of the capital.` });
  }
  if (r.max_orders_per_day < 1 || r.max_orders_per_day > MAX_ORDERS_PER_DAY) out.push({ rule: "schema", text: `Orders a day must be from 1 to ${MAX_ORDERS_PER_DAY.toLocaleString("en-US")}.` });
  const timeout = m.autonomy.approval.timeout_s;
  if (timeout < RETAIL_MIN_TIMEOUT_S || timeout > MAX_TIMEOUT_S) {
    out.push({ rule: "§4.3", text: `The approval window must be from ${seconds(RETAIL_MIN_TIMEOUT_S)} to ${seconds(MAX_TIMEOUT_S)}.` });
  }
  const allocation = dec(m.capital.allocation_usd);
  if (allocation > ctx.roomUsd) {
    out.push({ rule: "V-002", text: `Your paper account has ${usd(ctx.roomUsd)} that no other agent uses, less than the ${usd(allocation)} of capital.` });
  }
  const chain: Array<[EditPath, string]> = [
    ["/risk/max_order_usd", r.max_order_usd],
    ["/risk/max_position_usd", r.max_position_usd],
    ["/risk/max_gross_exposure_usd", r.max_gross_exposure_usd],
    ["/capital/allocation_usd", m.capital.allocation_usd],
  ];
  for (let i = 0; i + 1 < chain.length; i++) {
    const [lowPath, low] = chain[i];
    const [highPath, high] = chain[i + 1];
    if (dec(low) > dec(high)) out.push({ rule: "V-013", text: `${changeLabel(lowPath)}, ${usd(low)}, is more than ${changeLabel(highPath).toLowerCase()}, ${usd(high)}.` });
  }
  if (dec(m.capital.max_loss_from_allocation) < dec(r.max_drawdown)) {
    out.push({
      rule: "V-014",
      text: `${changeLabel("/capital/max_loss_from_allocation")}, ${percent(m.capital.max_loss_from_allocation)}, is below the ${percent(r.max_drawdown)} drawdown at which the agent closes everything and pauses.`,
    });
  }
  const hours = m.notifications.quiet_hours;
  if (hours && hours.start === hours.end) out.push({ rule: "V-016", text: "Quiet hours must start and end at different times." });
  if (m.autonomy.approval.two_approver_above_usd !== null && ctx.approverUsers < 2) {
    out.push({ rule: "V-024", text: "Two approvers needs two people with the approver role, and this workspace has one." });
  }
  return out;
}

/** Restrictions that latch a limit (§5.4 to §5.7): while one holds, the allocation cannot go up. */
const LATCHED: ReadonlySet<RestrictionCode> = new Set(["drawdown_scale_sizes", "drawdown_exits_only", "drawdown_flatten", "daily_loss", "hard_breach", "lifetime_floor"]);

export function latched(agent: Agent): RestrictionCode[] {
  return agent.restrictions.filter((r) => LATCHED.has(r.code)).map((r) => r.code);
}

const isOpening = (o: WorkingOrder) => o.side === "buy" && (o.purpose === "open" || o.purpose === "increase");
const orderValue = (o: WorkingOrder): Dec => mul(sub(dec(o.qty), dec(o.filled_qty)), dec(o.limit_price ?? "0"));

/** Σ |market value| plus working opening orders (§5.1). */
export function grossExposure(agent: Agent): Dec {
  return add(ZERO, ...agent.positions.map((p) => abs(dec(p.market_value))), ...agent.orders.filter(isOpening).map(orderValue));
}

/** a × b ÷ c rounded up at 12 places, so a scaled level never makes a loss fraction smaller (§5.1). */
function scaleUp(a: Dec, b: Dec, c: Dec): Dec {
  const numerator = a * b;
  const q = numerator / c;
  return numerator % c === ZERO ? q : q + 1n;
}

const HARD = dec("1.25");

interface Levels {
  e: Dec;
  h: Dec;
  e0: Dec;
  c: Dec;
  l: Dec;
}

/** Every limit condition, soft and hard, as §5.4, §5.5 and §5.7 state them. */
function conditions(m: Mandate, s: Levels): boolean[] {
  const out: boolean[] = [];
  for (const rung of m.risk.drawdown_ladder) {
    const at = dec(rung.at);
    out.push(sub(s.h, s.e) >= mul(at, s.h), sub(s.h, s.e) >= mul(mul(HARD, at), s.h));
  }
  const mdl = dec(m.risk.max_daily_loss);
  out.push(sub(s.e, s.e0) <= -mul(mdl, s.e0), sub(s.e, s.e0) <= -mul(mul(HARD, mdl), s.e0));
  const f = dec(m.capital.max_loss_from_allocation);
  out.push(s.e <= add(mul(s.c, sub(ONE, f)), s.l), s.e <= add(mul(s.c, sub(ONE, mul(HARD, f))), s.l));
  return out;
}

function levels(agent: Agent): Levels {
  const s = agent.state;
  return { e: dec(s.equity), h: dec(s.high_water_mark), e0: dec(s.equity_day_start), c: dec(s.capital_base), l: dec(s.inherited_loss) };
}

type Scaled = { ok: true; after: Levels } | { ok: false; reason: string };

/** §5.1: an allocation change of Δ, rejected or scaled, against the limits of the version it comes with. */
export function allocationChange(agent: Agent, next: Mandate): Scaled | null {
  const delta = sub(dec(next.capital.allocation_usd), dec(agent.mandate.capital.allocation_usd));
  if (delta === ZERO) return null;
  const before = levels(agent);
  const e1 = add(before.e, delta);
  if (delta > ZERO && latched(agent).length > 0) {
    return { ok: false, reason: "An allocation increase is refused while a limit is latched." };
  }
  if (e1 <= ZERO || e1 < grossExposure(agent)) {
    return { ok: false, reason: `The agent's equity after the change, ${usd(e1)}, would be less than what it holds and has on order, ${usd(grossExposure(agent))}.` };
  }
  const after: Levels = { e: e1, h: scaleUp(before.h, e1, before.e), e0: scaleUp(before.e0, e1, before.e), c: scaleUp(before.c, e1, before.e), l: scaleUp(before.l, e1, before.e) };
  const was = conditions(next, before);
  if (conditions(next, after).some((hit, i) => hit && !was[i])) {
    return { ok: false, reason: "After the change, one of the agent's limits would be reached that is not reached now." };
  }
  return { ok: true, after };
}

/** What a version does when it applies, beyond its own fields, for the review to state before it is confirmed. */
export interface Effects {
  approvalsWaiting: number;
  /** Working opening orders that break the new limits, canceled when the version applies (§2.2). */
  ordersCanceled: WorkingOrder[];
  /** Latched limits: a version never clears them (MI-3). */
  latched: RestrictionCode[];
  /** An order in an unknown state holds a risk-increasing version until it is resolved. */
  unknownOrder: boolean;
  /** A confirmed version still waiting for its safe point: it never applies if this one applies first. */
  pendingNumber: number | null;
}

function breaks(agent: Agent, m: Mandate): WorkingOrder[] {
  const held = (symbol: string) => add(ZERO, ...agent.positions.filter((p) => p.instrument.symbol === symbol).map((p) => abs(dec(p.market_value))));
  const holdings = add(ZERO, ...agent.positions.map((p) => abs(dec(p.market_value))));
  return agent.orders.filter((o) => {
    if (!isOpening(o) || o.state === "Unknown") return false;
    const value = orderValue(o);
    return value > dec(m.risk.max_order_usd) || add(held(o.instrument.symbol), value) > dec(m.risk.max_position_usd) || add(holdings, value) > dec(m.risk.max_gross_exposure_usd);
  });
}

function waiting(ws: Workspace, agentId: string, at: Iso) {
  return ws.approvals.filter((a) => a.agent_id === agentId && a.status === "delivered" && Date.parse(at) < Date.parse(a.deadline));
}

function pendingIndex(agent: Agent): number {
  return agent.versions.findIndex((v) => v.application.result === "pending");
}

export function effectsOf(ws: Workspace, agent: Agent, next: Mandate, at: Iso): Effects {
  const pending = pendingIndex(agent);
  return {
    approvalsWaiting: waiting(ws, agent.agent_id, at).length,
    ordersCanceled: breaks(agent, next),
    latched: latched(agent),
    unknownOrder: agent.orders.some((o) => o.state === "Unknown"),
    pendingNumber: pending === -1 ? null : pending + 1,
  };
}

export interface Proposal extends Diff {
  agentId: string;
  /** The version in effect when it was proposed, which it is diffed against. */
  previous: ContentRef;
  /** Version number it would take. */
  number: number;
  refusals: Refusal[];
}

/** The rule id a refusal for independent approval carries, kept in the record, never in the sentence (C-6). */
export const INDEPENDENT_APPROVAL_RULE = "V-047";

/** §4.3's effective policy. Only a stated `false` turns it off: absent or not known counts as required (rule 3). */
export function independentApprovalRequired(ws: Workspace): boolean {
  return ws.independent_approval_required !== false;
}

/**
 * §4.3 and V-047: under `independent_approval_required` a version that is not risk-reducing needs a
 * user other than the requester, and nothing here can ask one yet, so it is refused rather than
 * confirmed with a passkey alone. With fewer than two people who can approve, V-047 refuses a
 * neutral version too and lets only a risk-reducing one through (DEC-444). The approver count is a
 * lower bound on the workspace's users; with two or more, a neutral version needs no second user.
 */
function independentApproval(ws: Workspace, classification: ChangeClass | null): Refusal | null {
  if (!independentApprovalRequired(ws) || classification === null || classification === "risk_reducing") return null;
  if (ws.approver_users < 2) {
    return {
      rule: INDEPENDENT_APPROVAL_RULE,
      text: "This workspace needs a second person to approve any change that doesn't lower risk, and no second person can approve in it, so only a change that lowers risk can be confirmed.",
    };
  }
  if (classification === "risk_increasing") {
    return {
      rule: INDEPENDENT_APPROVAL_RULE,
      text: "This workspace needs a second person to approve a change that raises risk, and that approval can't be asked for here yet, so a passkey alone can't confirm it.",
    };
  }
  return null;
}

/** Everything that stops a version from applying now, from validation, §4.3's independent approval and §5.1. */
function refusalsFor(ws: Workspace, agent: Agent, d: Diff): Refusal[] {
  if (agent.mode === "stopped") return [{ rule: "stopped", text: `${agent.label} is stopped, and a stopped agent's mandate does not change.` }];
  if (d.changes.length === 0) return [];
  const room = add(unallocatedUsd(ws), dec(agent.mandate.capital.allocation_usd));
  const out = validate(d.mandate, { roomUsd: room, approverUsers: ws.approver_users });
  const independence = independentApproval(ws, d.classification);
  if (independence) out.push(independence);
  const scaled = allocationChange(agent, d.mandate);
  if (scaled && !scaled.ok) out.push({ rule: "§5.1", text: scaled.reason });
  return out;
}

export function propose(ws: Workspace, agent: Agent, edits: Edits): Proposal {
  const d = diffMandate(agent.mandate, edits);
  return { ...d, agentId: agent.agent_id, previous: agent.mandate_version, number: agent.versions.length + 1, refusals: refusalsFor(ws, agent, d) };
}

/** Where the owner asked: the Edit form records values as entered, a message as stated, with the words quoted (§2.1). */
export type Origin = { kind: "form" } | { kind: "message"; quote: string };

export type Recorded = { result: "applied" } | { result: "pending" } | { result: "rejected"; reason: string };

function event(seed: string, at: Iso, text: string, kind: TimelineKind = "version"): TimelineEvent {
  return { event_id: fixtureUlid(`event:${seed}`), at, kind, text };
}

function prepend(ws: Workspace, agentId: string, ...events: TimelineEvent[]) {
  ws.timeline[agentId] = [...events, ...(ws.timeline[agentId] ?? [])];
}

/** "Largest order from $1,000 to $800", for the record and the timeline. */
export function changeWords(c: MandateChange): string {
  return `${changeLabel(c.path)} from ${changeValue(c.path, c.from)} to ${changeValue(c.path, c.to)}`;
}

function provenanceAfter(agent: Agent, changes: MandateChange[], origin: Origin): FieldProvenance[] {
  const changed = new Set(changes.map((c) => c.path));
  const kept = agent.provenance.filter((p) => !changed.has(p.path));
  const fresh = changes.map((c): FieldProvenance => (origin.kind === "form" ? { path: c.path, provenance: "user_entered" } : { path: c.path, provenance: "user_stated", quote: origin.quote }));
  return [...kept, ...fresh];
}

function reject(ws: Workspace, agent: Agent, index: number, at: Iso, reason: string) {
  const record = agent.versions[index];
  agent.versions[index] = { ...record, application: { result: "rejected", at, reason } };
  prepend(ws, agent.agent_id, event(`${record.mandate_version}:rejected`, at, `Mandate version ${index + 1} rejected: ${reason}`));
}

/**
 * Applies the version at `index` (§2.2): the new document and its provenance, the risk state scaled
 * for an allocation change (§5.1), requests waiting for the owner canceled, opening orders that break
 * the new limits canceled, and any other version still waiting rejected, since it was diffed
 * against a version that is no longer in effect.
 */
function apply(ws: Workspace, agent: Agent, index: number, mandate: Mandate, origin: Origin, at: Iso) {
  const record = agent.versions[index];
  const number = index + 1;
  const scaled = allocationChange(agent, mandate);
  if (scaled?.ok) {
    agent.state = {
      ...agent.state,
      equity: toDecimalString(scaled.after.e),
      high_water_mark: toDecimalString(scaled.after.h),
      equity_day_start: toDecimalString(scaled.after.e0),
      capital_base: toDecimalString(scaled.after.c),
      inherited_loss: toDecimalString(scaled.after.l),
    };
  }
  const canceledOrders = breaks(agent, mandate);
  agent.orders = agent.orders.filter((o) => !canceledOrders.includes(o));
  agent.past_orders = [
    ...canceledOrders.map((o): PastOrder => ({ ...o, state: "Canceled", closed_at: at, note: `Canceled: larger than the limits of mandate version ${number}.` })),
    ...agent.past_orders,
  ];
  const canceled = waiting(ws, agent.agent_id, at);
  for (const a of canceled) {
    a.status = "superseded";
    a.resolution = { at, text: `Canceled: mandate version ${number} applied. Anything still wanted is proposed again under it.`, cancel_reason: "version_applied" };
  }
  const inEffect = agent.versions.findIndex((v) => v.mandate_version === agent.mandate_version) + 1;
  agent.versions.forEach((v, i) => {
    if (i !== index && v.application.result === "pending") {
      reject(ws, agent, i, at, `Version ${number} applied first, and this version was confirmed against version ${inEffect}, which is no longer in effect.`);
    }
  });
  agent.mandate = mandate;
  agent.mandate_version = record.mandate_version;
  agent.provenance = provenanceAfter(agent, record.changes, origin);
  agent.versions[index] = { ...record, application: { result: "applied", at, approvals_canceled: canceled.length } };
  prepend(
    ws,
    agent.agent_id,
    ...canceledOrders.map((o) =>
      event(`${o.client_order_id}:version-canceled`, at, `Buy ${quantity(o.qty)} ${o.instrument.symbol} at ${price(o.limit_price ?? "0")} canceled: larger than the limits of mandate version ${number}.`, "order"),
    ),
    event(`${record.mandate_version}:applied`, at, `Mandate version ${number} applied: ${record.changes.map(changeWords).join("; ")}.`),
  );
}

/**
 * The owner confirmed `proposal` and the deployment recorded it at `at`. A version proposed against
 * one that is no longer in effect is refused before it becomes a version. Otherwise every check is
 * repeated: a failure is a version rejected at application; a risk-increasing version waits for its
 * safe point; any other applies now.
 */
export function recordChange(ws: Workspace, proposal: Proposal, origin: Origin, at: Iso): { ws: Workspace; recorded: Recorded } {
  const next = structuredClone(ws);
  const agent = next.agents.find((a) => a.agent_id === proposal.agentId);
  if (!agent) return { ws, recorded: { result: "rejected", reason: "The agent no longer exists." } };
  if (agent.mandate_version !== proposal.previous) {
    return { ws, recorded: { result: "rejected", reason: "Another version applied after this change was shown, so nothing was confirmed. Look at the mandate as it is now and make the change again." } };
  }
  const d = diffMandate(agent.mandate, editsOf(proposal));
  if (d.version !== proposal.version || d.changes.length === 0) {
    return { ws, recorded: { result: "rejected", reason: "This change no longer matches the mandate in effect, so nothing was confirmed." } };
  }
  const refusals = refusalsFor(next, agent, d);
  const record: MandateVersionRecord = {
    mandate_version: d.version,
    previous: proposal.previous,
    confirmed_at: at,
    step_up: d.stepUp,
    classification: d.classification,
    changes: d.changes,
    application: { result: "pending" },
  };
  agent.versions = [...agent.versions, record];
  const index = agent.versions.length - 1;
  if (refusals.length > 0) {
    const reason = refusals.map((r) => r.text).join(" ");
    reject(next, agent, index, at, reason);
    return { ws: next, recorded: { result: "rejected", reason } };
  }
  if (d.classification === "risk_increasing") {
    agent.versions.forEach((v, i) => {
      if (i !== index && v.application.result === "pending") reject(next, agent, i, at, `Version ${index + 1}, confirmed after it, replaces it.`);
    });
    return { ws: next, recorded: { result: "pending" } };
  }
  apply(next, agent, index, d.mandate, origin, at);
  return { ws: next, recorded: { result: "applied" } };
}

/**
 * The agent's next evaluation (§2.2): a waiting risk-increasing version applies once no order is in
 * an unknown state, after its checks are repeated. `null` while it still waits.
 */
export function reachSafePoint(ws: Workspace, agentId: string, version: ContentRef, origin: Origin, at: Iso): { ws: Workspace; recorded: Recorded } | null {
  const current = ws.agents.find((a) => a.agent_id === agentId);
  const pendingAt = current?.versions.findIndex((v) => v.mandate_version === version && v.application.result === "pending") ?? -1;
  if (!current || pendingAt === -1) {
    const settled = current?.versions.find((v) => v.mandate_version === version)?.application;
    if (settled?.result === "applied") return { ws, recorded: { result: "applied" } };
    return { ws, recorded: { result: "rejected", reason: settled?.result === "rejected" ? settled.reason : "This version is no longer waiting to apply." } };
  }
  if (current.orders.some((o) => o.state === "Unknown")) return null;
  const next = structuredClone(ws);
  const agent = next.agents.find((a) => a.agent_id === agentId);
  if (!agent) return null;
  const record = agent.versions[pendingAt];
  const d = diffMandate(agent.mandate, editsOf(record));
  if (record.previous !== agent.mandate_version || d.version !== record.mandate_version) {
    const reason = "It was confirmed against a version that is no longer in effect.";
    reject(next, agent, pendingAt, at, reason);
    return { ws: next, recorded: { result: "rejected", reason } };
  }
  const refusals = refusalsFor(next, agent, d);
  if (refusals.length > 0) {
    const reason = refusals.map((r) => r.text).join(" ");
    reject(next, agent, pendingAt, at, reason);
    return { ws: next, recorded: { result: "rejected", reason } };
  }
  apply(next, agent, pendingAt, d.mandate, origin, at);
  return { ws: next, recorded: { result: "applied" } };
}

/** A proposal's own edits, so the deployment rebuilds the document rather than trusting the one sent. */
export function editsOf(p: Pick<Proposal, "changes">): Edits {
  return Object.fromEntries(p.changes.map((c) => [c.path, c.to])) as Edits;
}
