import { type Market, etParts } from "@/fixtures/market";
import type { Workspace } from "@/fixtures/types";
import { dec, div, sign, sub } from "./decimal";
import { type Direction, MINUS, signedPercent } from "./format";

export interface Quote {
  symbol: string;
  /** The last price, at the instrument's own precision. */
  last: string;
  /** The change since the previous session's close, "+1.07%", or null with no earlier close. */
  change: string | null;
  direction: Direction;
}

/** The instruments any agent holds, once each, in the order the agents list them. */
export function heldSymbols(ws: Workspace): Array<{ symbol: string; mark: string }> {
  const seen = new Map<string, string>();
  for (const agent of ws.agents) {
    for (const p of agent.positions) if (sign(dec(p.qty)) !== 0 && !seen.has(p.instrument.symbol)) seen.set(p.instrument.symbol, p.mark);
  }
  return [...seen].map(([symbol, mark]) => ({ symbol, mark }));
}

function previousClose(market: Market, symbol: string): number | null {
  const today = etParts(market.end).date;
  const daily = market.symbols[symbol]?.daily ?? [];
  for (let i = daily.length - 1; i >= 0; i--) if (daily[i].day < today) return daily[i].close;
  return null;
}

/** Last price and day change for each held instrument. Prices only: never an amount won or lost. */
export function quotes(ws: Workspace, market: Market): Quote[] {
  return heldSymbols(ws).map(({ symbol, mark }) => {
    const close = previousClose(market, symbol);
    if (close === null) return { symbol, last: mark, change: null, direction: "flat" };
    const places = Math.max(2, (mark.split(".")[1] ?? "").length);
    const before = dec(close.toFixed(places));
    const change = signedPercent(div(sub(dec(mark), before), before), 2);
    return { symbol, last: mark, change, direction: change.startsWith("+") ? "gain" : change.startsWith(MINUS) ? "loss" : "flat" };
  });
}

export type FeedKey = keyof Workspace["health"];

/** The feed heard from longest ago, whose age is how fresh the whole screen is. */
export function oldestFeed(ws: Workspace): { key: FeedKey; as_of: string } {
  const feeds = Object.entries(ws.health) as Array<[FeedKey, { as_of: string }]>;
  const [key, feed] = feeds.reduce((a, b) => (Date.parse(b[1].as_of) < Date.parse(a[1].as_of) ? b : a));
  return { key, as_of: feed.as_of };
}

/** Every feed answering: the only time the frame may show quotes instead of the status strip. */
export function allFeedsOk(ws: Workspace): boolean {
  return ws.status === "ready" && Object.values(ws.health).every((f) => f.state === "ok");
}
