import type { Agent, Fill, OrderState, PastOrder, WorkingOrder } from "@/fixtures/types";
import { type Dec, ZERO, add, dec, div, mul, sub } from "./decimal";
import { price, quantity } from "./format";

export type AnyOrder = WorkingOrder | PastOrder;

export function isPast(order: AnyOrder): order is PastOrder {
  return "closed_at" in order;
}

/** Working orders first, newest first, then finished ones, newest first. */
export function allOrders(agent: Agent): AnyOrder[] {
  const by = (a: AnyOrder, b: AnyOrder) => Date.parse(b.submitted_at) - Date.parse(a.submitted_at);
  return [...[...agent.orders].sort(by), ...[...agent.past_orders].sort(by)];
}

export function findOrder(agent: Agent, clientOrderId: string): AnyOrder | undefined {
  return allOrders(agent).find((o) => o.client_order_id === clientOrderId);
}

export function fillsOf(agent: Agent, clientOrderId: string): Fill[] {
  return agent.fills.filter((f) => f.client_order_id === clientOrderId);
}

/** Realized P&L in one instrument from the agent's fills, at average cost, oldest fill first. */
export function realizedIn(agent: Agent, symbol: string): Dec {
  let qty = ZERO;
  let cost = ZERO;
  let realized = ZERO;
  const fills = agent.fills.filter((f) => f.instrument.symbol === symbol).sort((a, b) => Date.parse(a.at) - Date.parse(b.at));
  for (const f of fills) {
    const q = dec(f.qty);
    const p = dec(f.price);
    if (f.side === "buy") {
      qty = add(qty, q);
      cost = add(cost, mul(q, p));
      continue;
    }
    const avg = qty === ZERO ? ZERO : div(cost, qty);
    realized = add(realized, mul(sub(p, avg), q));
    cost = sub(cost, mul(avg, q));
    qty = sub(qty, q);
  }
  return realized;
}

export function orderSentence(o: AnyOrder): string {
  const side = o.side === "buy" ? "Buy" : "Sell";
  const at = o.stop_price && o.limit_price ? `, stop ${price(o.stop_price)}, limit ${price(o.limit_price)}` : o.stop_price ? `, stop ${price(o.stop_price)}` : o.limit_price ? ` at a limit of ${price(o.limit_price)}` : "";
  return `${side} ${quantity(o.qty)} ${o.instrument.symbol}${at}`;
}

/** An ISO time moved by whole seconds, written in the fixture's ET offset. */
function shift(iso: string, seconds: number): string {
  const ms = Date.parse(iso) + seconds * 1000 - 4 * 3600 * 1000;
  return `${new Date(ms).toISOString().slice(0, 19)}-04:00`;
}

export interface LifecycleStep {
  at: string;
  state: OrderState;
  text: string;
}

/**
 * The order's life as the journal records it: recorded before sending, sent, the broker's answer,
 * each fill, and the final state with its reason. An unknown answer stays unknown.
 */
export function lifecycle(order: AnyOrder, fills: Fill[]): LifecycleStep[] {
  const steps: LifecycleStep[] = [
    { at: shift(order.submitted_at, -1), state: "Intent", text: "Recorded in the journal before anything was sent." },
    { at: order.submitted_at, state: "Submitting", text: "Sent to the broker." },
  ];
  if (order.state === "Unknown") {
    steps.push({
      at: shift(order.submitted_at, 5),
      state: "Unknown",
      text: `No answer from the broker. Nothing else is sent in ${order.instrument.symbol}, exits included, until it answers; the kill switch still works.`,
    });
    return steps;
  }
  if (order.state === "Rejected" && isPast(order)) {
    steps.push({ at: order.closed_at, state: "Rejected", text: order.note });
    return steps;
  }
  steps.push({ at: shift(order.submitted_at, 1), state: "Accepted", text: "The broker accepted it." });
  let filled = 0;
  const total = Number(order.qty);
  for (const f of fills) {
    filled += Number(f.qty);
    const full = Math.abs(filled - total) < 1e-12;
    steps.push({ at: f.at, state: full ? "Filled" : "PartiallyFilled", text: `Filled ${quantity(f.qty)} at ${price(f.price)}.` });
  }
  if (isPast(order) && order.state !== "Filled") steps.push({ at: order.closed_at, state: order.state, text: order.note });
  else if (isPast(order) && order.note && order.note !== "Filled in full.") steps.push({ at: order.closed_at, state: "Filled", text: order.note });
  return steps;
}
