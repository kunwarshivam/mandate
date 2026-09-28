import type { ChartLevel } from "@/components/charts/options";
import { type Bar, type DailyBar, type Market, type Point, aggregate, etParts, sample, unixOf } from "@/fixtures/market";
import type { Agent } from "@/fixtures/types";
import { toFixed } from "./decimal";
import { agentLimits } from "./limits";

export type EquityRange = "1D" | "1W" | "1M" | "All";
export const EQUITY_RANGES: ReadonlyArray<{ id: EquityRange; label: string }> = [
  { id: "1D", label: "1D" },
  { id: "1W", label: "1W" },
  { id: "1M", label: "1M" },
  { id: "All", label: "All" },
];

export type PriceRange = "1D" | "5D" | "1M" | "6M" | "1Y";
export const PRICE_RANGES: ReadonlyArray<{ id: PriceRange; label: string }> = [
  { id: "1D", label: "1D" },
  { id: "5D", label: "5D" },
  { id: "1M", label: "1M" },
  { id: "6M", label: "6M" },
  { id: "1Y", label: "1Y" },
];

const HOUR = 3600;
const DAY = 86_400;

function dayStart(end: number): number {
  return unixOf(`${etParts(end).date}T00:00:00-04:00`);
}

/** Equity for a range: every minute today, every 10 minutes for a week, hourly beyond. */
export function equityWindow(points: Point[], range: EquityRange, end: number): Point[] {
  if (points.length === 0) return [];
  switch (range) {
    case "1D":
      return sample(points, dayStart(end), 60);
    case "1W":
      return sample(points, end - 7 * DAY, 600);
    case "1M":
      return sample(points, end - 30 * DAY, HOUR);
    case "All":
      return sample(points, points[0].time, HOUR);
    default: {
      const unhandled: never = range;
      throw new Error(`unhandled range ${String(unhandled)}`);
    }
  }
}

/** Candles for a range: minutes for the last trading day, 5-minute bars for five, days beyond. */
export function priceWindow(minute: Bar[], daily: DailyBar[], range: PriceRange): Array<Bar | DailyBar> {
  const days = [...new Set(minute.map((b) => etParts(b.time).date))];
  switch (range) {
    case "1D": {
      const last = days[days.length - 1];
      return minute.filter((b) => etParts(b.time).date === last);
    }
    case "5D": {
      const keep = new Set(days.slice(-5));
      return aggregate(
        minute.filter((b) => keep.has(etParts(b.time).date)),
        5,
      );
    }
    case "1M":
      return daily.slice(-22);
    case "6M":
      return daily.slice(-126);
    case "1Y":
      return daily.slice(-252);
    default: {
      const unhandled: never = range;
      throw new Error(`unhandled range ${String(unhandled)}`);
    }
  }
}

export function equityPoints(market: Market, agentId: string): Point[] {
  return market.equity[agentId] ?? [];
}

/** The mandate's equity levels in dollars, as price lines; the same values the rails use. */
export function mandateLevels(agent: Agent): ChartLevel[] {
  return agentLimits(agent).levels.map((l) => ({
    key: l.key,
    label: l.label,
    price: Number(toFixed(l.at, 2)),
    tone: "mandate" as const,
    meaning: l.action,
  }));
}

/** Levels near enough to the data to draw without flattening it; the rest are listed, not drawn. */
export function drawable(levels: ChartLevel[], values: number[], margin = 0.04): { drawn: ChartLevel[]; offChart: ChartLevel[] } {
  if (values.length === 0) return { drawn: [], offChart: levels };
  const lo = Math.min(...values) * (1 - margin);
  const hi = Math.max(...values) * (1 + margin);
  const drawn = levels.filter((l) => l.price >= lo && l.price <= hi);
  return { drawn, offChart: levels.filter((l) => !drawn.includes(l)) };
}

export function closes(series: Array<Bar | DailyBar>): number[] {
  return series.flatMap((b) => [b.low, b.high]);
}
