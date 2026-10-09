/**
 * What the fixture deployment does after the owner acts, as pure functions over a workspace: a
 * confirmed mandate becomes an agent, the agent's first check proposes one buy that asks the owner,
 * an approved buy is submitted and then filled at the fixture market, never above its limit, with
 * protection placed, and a skip is noted. The runtime applies each step only once it is
 * "recorded", the way the journal would; nothing here is market data or a broker, and every price
 * is a fixture price.
 */
import type {
  Agent,
  Approval,
  ContentRef,
  FieldProvenance,
  Fill,
  GateDecision,
  InstrumentRef,
  Iso,
  Mandate,
  PastOrder,
  Position,
  TimelineEvent,
  TimelineKind,
  WorkingOrder,
  Workspace,
} from "@/fixtures/types";
import { lastPrice } from "@/fixtures/market";
import { type Dec, ONE, ZERO, add, dec, div, min, mul, sub, toDecimalString, toFixed } from "./decimal";
import { fixtureUlid } from "./fixture-ids";
import { price, quantity, usd } from "./format";
import { sha256Hex } from "./sha256";

/** An ISO time moved by `ms`, written in the same UTC offset it came in. */
export function addMs(iso: Iso, ms: number): Iso {
  const offset = iso.slice(-6);
  const sign = offset.startsWith("-") ? -1 : 1;
  const [h, m] = offset.slice(1).split(":").map(Number);
  const local = new Date(Date.parse(iso) + ms + sign * (h * 60 + m) * 60_000);
  return `${local.toISOString().slice(0, 19)}${offset}`;
}

/** JSON with every object's keys sorted, so equal documents always hash the same. */
export function canonicalJson(value: unknown): string {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  const entries = Object.entries(value as Record<string, unknown>)
    .filter(([, v]) => v !== undefined)
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
  return `{${entries.map(([k, v]) => `${JSON.stringify(k)}:${canonicalJson(v)}`).join(",")}}`;
}

export function mandateVersion(mandate: Mandate): ContentRef {
  return `sha256:${sha256Hex(canonicalJson(mandate))}`;
}

/** Agents still running on the account: a stopped agent's allocation is free again. */
function activeAgents(ws: Workspace): Agent[] {
  return ws.agents.filter((a) => a.mode !== "stopped");
}

/** V-002's headroom: account equity less every active agent's allocation. */
export function unallocatedUsd(ws: Workspace): Dec {
  return sub(dec(ws.connection.account_equity), add(ZERO, ...activeAgents(ws).map((a) => dec(a.mandate.capital.allocation_usd))));
}

/** V-006: the active agent that already trades `symbol` on this account, if one does. */
export function claimedBy(ws: Workspace, symbol: string): Agent | undefined {
  return activeAgents(ws).find((a) => a.mandate.universe.pinned_instruments.some((i) => i.symbol === symbol));
}

/** The rule id a refusal for independent approval carries, kept in the record, never in the sentence (C-6). */
export const INDEPENDENT_APPROVAL_RULE = "V-047";

/** §4.3's effective policy. Only a stated `false` turns it off: absent or not known counts as required (rule 3). */
export function independentApprovalRequired(ws: Workspace): boolean {
  return ws.independent_approval_required !== false;
}

/** Why a deployment is refused, with the rule it falls under; `rule` is set only for a rule the record names apart from the sentence. */
export interface DeployRefusal {
  rule?: string;
  reason: string;
}

/**
 * §4.3 and V-047: under `independent_approval_required` every deployment needs a user other than the
 * requester, a first version having no previous version for DEC-444's risk-reducing exception.
 * Nothing here can ask a second user yet, so a deployment under the policy is always refused, however
 * many people could approve, rather than confirmed with a passkey alone (interim, E11-12).
 */
export function independentDeployRefusal(ws: Workspace): DeployRefusal | null {
  if (!independentApprovalRequired(ws)) return null;
  return {
    rule: INDEPENDENT_APPROVAL_RULE,
    reason: "This workspace needs a second person to approve a new agent, and that approval can't be asked for here yet, so a passkey alone can't create it.",
  };
}

export type DeployCheck = { ok: true } | ({ ok: false } & DeployRefusal);

/** The checks a deployment repeats when it is applied (mandate spec V-047, V-002 and V-006). */
export function checkDeploy(ws: Workspace, mandate: Mandate): DeployCheck {
  const independence = independentDeployRefusal(ws);
  if (independence) return { ok: false, ...independence };
  const room = unallocatedUsd(ws);
  if (dec(mandate.capital.allocation_usd) > room) {
    return { ok: false, reason: `Your paper account has ${usd(room)} that no agent uses, less than the ${usd(mandate.capital.allocation_usd)} this agent asks for.` };
  }
  for (const i of mandate.universe.pinned_instruments) {
    const holder = claimedBy(ws, i.symbol);
    if (holder) return { ok: false, reason: `${i.symbol} is already traded by ${holder.label}. One agent trades an instrument on an account.` };
  }
  return { ok: true };
}

/** Where `symbol` trades in the fixture now: the mark of the agent holding it, else its recorded last price. */
export function marketPrice(ws: Workspace, symbol: string): string {
  for (const a of ws.agents) {
    const held = a.positions.find((p) => p.instrument.symbol === symbol);
    if (held) return held.mark;
  }
  return lastPrice(symbol);
}

function nextLabel(ws: Workspace): string {
  const numbers = ws.agents.map((a) => Number(/^Agent (\d+)$/.exec(a.label)?.[1] ?? 0));
  return `Agent ${Math.max(0, ...numbers) + 1}`;
}

function event(seed: string, at: Iso, kind: TimelineKind, text: string): TimelineEvent {
  return { event_id: fixtureUlid(`event:${seed}`), at, kind, text };
}

function prepend(ws: Workspace, agentId: string, ...events: TimelineEvent[]) {
  ws.timeline[agentId] = [...events, ...(ws.timeline[agentId] ?? [])];
}

export interface NewAgent {
  mandate: Mandate;
  provenance: FieldProvenance[];
}

export function agentIdFor(mandate: Mandate, seed: string): string {
  return `agt_${fixtureUlid(`agent:${seed}:${mandateVersion(mandate)}`)}`;
}

/**
 * The confirmed mandate as a running paper agent with nothing held: version 1, its allocation as
 * its equity, and one timeline entry saying who confirmed it. The caller checks `checkDeploy` first,
 * which refuses every deployment the workspace's independent-approval policy covers (V-047).
 */
export function deployAgent(ws: Workspace, request: NewAgent, at: Iso, seed: string): { ws: Workspace; agentId: string } {
  const next = structuredClone(ws);
  const version = mandateVersion(request.mandate);
  const agentId = agentIdFor(request.mandate, seed);
  const allocation = request.mandate.capital.allocation_usd;
  const agent: Agent = {
    agent_id: agentId,
    label: nextLabel(next),
    mandate: request.mandate,
    mandate_version: version,
    provenance: request.provenance,
    mode: "normal",
    restrictions: [],
    startup: "ready",
    pnl_total: "0",
    pnl_today: "0",
    realized_pnl: "0",
    state: {
      equity: allocation,
      equity_day_start: allocation,
      high_water_mark: allocation,
      capital_base: allocation,
      inherited_loss: "0",
      orders_today: 0,
      size_factor: "1",
      pending: [],
    },
    positions: [],
    orders: [],
    past_orders: [],
    fills: [],
    goal_progress: null,
    deployed_at: at,
    versions: [
      {
        mandate_version: version,
        previous: null,
        confirmed_at: at,
        step_up: true,
        classification: null,
        changes: [],
        application: { result: "applied", at, approvals_canceled: 0 },
      },
    ],
  };
  next.agents = [...next.agents, agent];
  prepend(next, agentId, event(`${agentId}:deployed`, at, "version", `Mandate version 1 confirmed by you and deployed to paper, with ${usd(allocation)} of simulated money.`));
  return { ws: next, agentId };
}

/** A fixture model score between 0.52 and 0.79, fixed per symbol and model. */
function fixtureScore(symbol: string, model: string): string {
  const hex = sha256Hex(`score:${model}:${symbol}`);
  return `0.${52 + (parseInt(hex.slice(0, 4), 16) % 28)}`;
}

function modelLines(model: Mandate["behavior"]["signal_models"][number], score: string, symbol: string): string[] {
  const params = model.params.map((p) => `${p.key}: ${p.value}`).join(", ");
  const lookback = model.params.find((p) => p.key === "lookback_bars")?.value ?? "?";
  const reading =
    model.id === "quant.mean_reversion"
      ? `${symbol}'s last close sits below its ${lookback}-bar average by more than the entry z-score`
      : `${symbol} closed above its ${lookback}-bar average for 4 bars`;
  return [`score: ${score} (weight ${model.weight})`, params, reading];
}

/**
 * The agent's first check: its one model scores its first affordable instrument and, at or above the
 * entry threshold, the order builder sizes one limit buy at the fixture price, capped by the order
 * and position limits. A new mandate asks before every buy, so the gate's allow becomes a request to
 * the owner with the mandate's deadline. With nothing affordable or a score below the threshold, the
 * check says so and asks nothing.
 */
export function firstProposal(ws: Workspace, agentId: string, at: Iso, seed: string): Workspace {
  const next = structuredClone(ws);
  const agent = next.agents.find((a) => a.agent_id === agentId);
  if (!agent || agent.mode !== "normal") return ws;
  const m = agent.mandate;
  const model = m.behavior.signal_models[0];
  if (!model) return ws;
  const budget = min(dec(m.risk.max_order_usd), dec(m.risk.max_position_usd));
  const candidates = [...m.universe.pinned_instruments].sort((a, b) => (a.symbol < b.symbol ? -1 : 1));
  const pick = candidates.find((i) => dec(marketPrice(ws, i.symbol)) <= budget);
  if (!pick) {
    prepend(next, agentId, event(`${agentId}:no-proposal:${seed}`, at, "gate", `No buy proposed: one share of each symbol costs more than the ${usd(budget)} order limit.`));
    return next;
  }
  const score = fixtureScore(pick.symbol, model.id);
  if (dec(score) < dec(m.behavior.sizing.entry_threshold)) {
    prepend(next, agentId, event(`${agentId}:below-threshold:${seed}`, at, "gate", `No buy proposed: ${pick.symbol} scored ${score}, below the entry threshold of ${m.behavior.sizing.entry_threshold}.`));
    return next;
  }
  const limit = marketPrice(ws, pick.symbol);
  const shares = div(budget, dec(limit)) / ONE;
  const qty = String(shares);
  const cost = toFixed(mul(shares * ONE, dec(limit)), 2);
  const approvalId = `apr_${fixtureUlid(`approval:${agentId}:${seed}`)}`;
  const approval: Approval = {
    approval_id: approvalId,
    agent_id: agentId,
    status: "delivered",
    requested_at: at,
    deadline: addMs(at, m.autonomy.approval.timeout_s * 1000),
    bound: {
      symbol: pick.symbol,
      asset_class: pick.asset_class,
      side: "buy",
      qty,
      limit,
      purpose: "open",
      mandate_version: agent.mandate_version,
      decided_by: "default:ask",
      combined_score: score,
    },
    trigger: "Your mandate asks before every buy.",
    risk_impact: [
      { field: "order_usd", value: cost, cap: m.risk.max_order_usd },
      { field: "position_usd_after", value: cost, cap: m.risk.max_position_usd },
      { field: "gross_usd_after", value: cost, cap: m.risk.max_gross_exposure_usd },
      { field: "bought_today_usd", value: cost, cap: null },
    ],
    evidence: [{ author: "owner_selected", model_id: model.id, version: model.version, produced_at: at, lines: modelLines(model, score, pick.symbol) }],
    approvers_required: 1,
    approvals_so_far: [],
  };
  const decision: GateDecision = {
    event_id: fixtureUlid(`decision:${approvalId}`),
    at,
    agent_id: agentId,
    mandate_version: agent.mandate_version,
    verdict: "allow",
    reason_code: null,
    action: { side: "buy", qty, symbol: pick.symbol, limit_price: limit, purpose: "open" },
    then: "Asked you for approval (your mandate asks before every buy).",
    approval_id: approvalId,
  };
  next.approvals = [approval, ...next.approvals];
  next.decisions = [decision, ...next.decisions];
  prepend(next, agentId, event(`${approvalId}:asked`, at, "approval", `Asked you to approve buy ${quantity(qty)} ${pick.symbol} at ${price(limit)}.`));
  return next;
}

function instrumentOf(agent: Agent, symbol: string): InstrumentRef | undefined {
  return agent.mandate.universe.pinned_instruments.find((i) => i.symbol === symbol) ?? agent.positions.find((p) => p.instrument.symbol === symbol)?.instrument;
}

/** The re-run allow that sent an approved buy, which names its order. */
function sentBy(ws: Workspace, approvalId: string): GateDecision | undefined {
  return ws.decisions.find((d) => d.approval_id === approvalId && d.client_order_id);
}

/**
 * An approval the owner answered and the gate re-ran: the buy goes to the paper broker as a resting
 * limit order, and the record says so. Only an approval that reached `acted` sends anything.
 */
export function submitApproved(ws: Workspace, approvalId: string, at: Iso): Workspace {
  const approval = ws.approvals.find((a) => a.approval_id === approvalId);
  if (!approval || approval.status !== "acted" || sentBy(ws, approvalId)) return ws;
  const next = structuredClone(ws);
  const agent = next.agents.find((a) => a.agent_id === approval.agent_id);
  if (!agent || agent.mode === "stopped") return ws;
  const instrument = instrumentOf(agent, approval.bound.symbol);
  if (!instrument) return ws;
  const { qty, limit, symbol, purpose } = approval.bound;
  const cid = `cid_${fixtureUlid(`order:${approvalId}`)}`;
  const order: WorkingOrder = {
    client_order_id: cid,
    instrument,
    side: "buy",
    qty,
    filled_qty: "0",
    limit_price: limit,
    stop_price: null,
    purpose,
    state: "Accepted",
    submitted_at: at,
    time_in_force: instrument.asset_class === "crypto" ? "gtc" : "day",
  };
  agent.orders = [order, ...agent.orders];
  agent.state.orders_today += 1;
  next.decisions = [
    {
      event_id: fixtureUlid(`decision:${approvalId}:rerun`),
      at,
      agent_id: agent.agent_id,
      mandate_version: agent.mandate_version,
      verdict: "allow",
      reason_code: null,
      action: { side: "buy", qty, symbol, limit_price: limit, purpose },
      then: "Approved by you; submitted to the paper broker.",
      client_order_id: cid,
      approval_id: approvalId,
    },
    ...next.decisions,
  ];
  prepend(
    next,
    agent.agent_id,
    event(`${cid}:submitted`, at, "order", `Buy ${quantity(qty)} ${symbol} at ${price(limit)} submitted; resting.`),
    event(`${approvalId}:approved`, at, "approval", `You approved buy ${quantity(qty)} ${symbol} at ${price(limit)}.`),
  );
  return next;
}

const cents = (value: Dec) => toFixed(value, 2);

/** The protective prices for a holding bought at `cost` (trading spec §5.4), rounded to the cent. */
function protectiveLevels(m: Mandate, cost: Dec): { stop: string; limit: string | null; takeProfit: string | null } {
  const stop = mul(cost, sub(ONE, dec(m.protection.stop_distance ?? "0")));
  const offset = m.protection.crypto_stop_limit_offset;
  return {
    stop: cents(stop),
    limit: offset ? cents(mul(dec(cents(stop)), sub(ONE, dec(offset)))) : null,
    takeProfit: m.protection.take_profit_distance ? cents(mul(cost, add(ONE, dec(m.protection.take_profit_distance)))) : null,
  };
}

/**
 * The submitted buy fills in full at the fixture market price, never above its limit, so the recorded
 * price history it lands in is unchanged. The position grows at average cost, the equity moves
 * only by the difference between the mark and the fill, and protection is placed: for an equity, a
 * new bracket tranche's stop for the quantity bought; for crypto, the one stop-limit re-placed for
 * the new quantity at the new average cost. The approval and the decision then say it filled.
 */
export function fillApproved(ws: Workspace, approvalId: string, at: Iso): Workspace {
  const sent = sentBy(ws, approvalId);
  if (!sent?.client_order_id) return ws;
  const next = structuredClone(ws);
  const agent = next.agents.find((a) => a.agent_id === sent.agent_id);
  const order = agent?.orders.find((o) => o.client_order_id === sent.client_order_id);
  if (!agent || !order || agent.mode === "stopped") return ws;
  const m = agent.mandate;
  const q = dec(order.qty);
  const symbol = order.instrument.symbol;
  const mark = dec(marketPrice(ws, symbol));
  const fillPrice = min(dec(order.limit_price ?? "0"), mark);
  const fillText = cents(fillPrice);

  const fill: Fill = { fill_id: `fil_${fixtureUlid(`fill:${order.client_order_id}`)}`, client_order_id: order.client_order_id, instrument: order.instrument, side: "buy", qty: order.qty, price: fillText, at };
  agent.fills = [...agent.fills, fill];
  agent.orders = agent.orders.filter((o) => o !== order);
  const filled: PastOrder = { ...order, state: "Filled", filled_qty: order.qty, closed_at: at, note: "Approved by you; filled at the market, at or below its limit (fixture fill)." };
  agent.past_orders = [filled, ...agent.past_orders];

  const held = agent.positions.find((p) => p.instrument.symbol === symbol);
  const qtyAfter = add(held ? dec(held.qty) : ZERO, q);
  const avgAfter = held ? div(add(mul(dec(held.qty), dec(held.avg_cost)), mul(q, fillPrice)), qtyAfter) : fillPrice;
  const value = mul(qtyAfter, mark);
  const avgText = toDecimalString(dec(toFixed(avgAfter, 4)));
  const equityMove = mul(q, sub(mark, fillPrice));

  const crypto = order.instrument.asset_class === "crypto";
  const levels = protectiveLevels(m, crypto ? dec(avgText) : fillPrice);
  const events: TimelineEvent[] = [event(`${fill.fill_id}:filled`, at, "fill", `Filled buy ${quantity(order.qty)} ${symbol} at ${price(fill.price)}.`)];
  if (m.protection.enabled) {
    if (crypto) {
      const replaced = agent.orders.filter((o) => o.instrument.symbol === symbol && o.purpose === "protective");
      agent.orders = agent.orders.filter((o) => !replaced.includes(o));
      agent.past_orders = [...replaced.map((o): PastOrder => ({ ...o, state: "Canceled", closed_at: at, note: "Canceled to re-place protection for the new quantity." })), ...agent.past_orders];
    }
    const protectQty = crypto ? toDecimalString(qtyAfter) : order.qty;
    agent.orders = [
      ...agent.orders,
      {
        client_order_id: `cid_${fixtureUlid(`protect:${order.client_order_id}`)}`,
        instrument: order.instrument,
        side: "sell",
        qty: protectQty,
        filled_qty: "0",
        limit_price: levels.limit,
        stop_price: levels.stop,
        purpose: "protective",
        state: "Accepted",
        submitted_at: at,
        time_in_force: "gtc",
      },
    ];
    events.unshift(
      event(
        `${order.client_order_id}:protected`,
        at,
        "protection",
        crypto
          ? `Stop-limit placed: ${quantity(protectQty)} ${symbol}, stop ${price(levels.stop)}, limit ${price(levels.limit ?? "0")}.`
          : `Bracket placed for ${quantity(protectQty)} ${symbol}: stop ${price(levels.stop)}${levels.takeProfit ? `, take-profit ${price(levels.takeProfit)}` : ""}.`,
      ),
    );
  }

  const position: Position = {
    instrument: order.instrument,
    qty: toDecimalString(qtyAfter),
    broker_qty: toDecimalString(qtyAfter),
    avg_cost: avgText,
    mark: toDecimalString(mark),
    mark_as_of: held?.mark_as_of ?? at,
    market_value: cents(value),
    unrealized_pnl: cents(sub(value, mul(qtyAfter, dec(avgText)))),
    protection:
      !m.protection.enabled
        ? { kind: "none", stop_price: null, limit_price: null, take_profit_price: null, unprotected_fraction: null }
        : crypto
          ? { kind: "crypto_stop_limit", stop_price: levels.stop, limit_price: levels.limit, take_profit_price: null, unprotected_fraction: null }
          : (held?.protection ?? { kind: "bracket", stop_price: levels.stop, limit_price: null, take_profit_price: levels.takeProfit, unprotected_fraction: null }),
  };
  agent.positions = held ? agent.positions.map((p) => (p === held ? position : p)) : [...agent.positions, position];
  agent.state.equity = cents(add(dec(agent.state.equity), equityMove));
  agent.pnl_total = cents(add(dec(agent.pnl_total), equityMove));
  agent.pnl_today = cents(add(dec(agent.pnl_today), equityMove));
  next.connection.account_equity = cents(add(dec(next.connection.account_equity), equityMove));

  const approval = next.approvals.find((a) => a.approval_id === approvalId);
  if (approval?.resolution) approval.resolution = { ...approval.resolution, text: `Approved by you. The gate re-ran and allowed it; the order was submitted and filled ${quantity(order.qty)} at ${price(fill.price)}.` };
  next.decisions = next.decisions.map((d) => (d.event_id === sent.event_id ? { ...d, then: "Approved by you; submitted and filled." } : d));
  prepend(next, agent.agent_id, ...events);
  return next;
}

/** A skip sends nothing; the agent's timeline says the owner skipped it. */
export function noteSkip(ws: Workspace, approvalId: string, at: Iso): Workspace {
  const approval = ws.approvals.find((a) => a.approval_id === approvalId);
  if (approval?.status !== "rejected") return ws;
  const next = structuredClone(ws);
  prepend(next, approval.agent_id, event(`${approvalId}:skipped`, at, "approval", `You skipped buy ${quantity(approval.bound.qty)} ${approval.bound.symbol}.`));
  return next;
}
