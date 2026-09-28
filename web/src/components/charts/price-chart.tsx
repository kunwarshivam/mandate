"use client";

import { useMemo, useState } from "react";
import { type Bar, type DailyBar, etParts, unixOf } from "@/fixtures/market";
import type { Agent, Approval, Position } from "@/fixtures/types";
import { PRICE_RANGES, type PriceRange, closes, drawable, priceWindow } from "@/lib/chart-data";
import { price } from "@/lib/format";
import { cn } from "@/lib/utils";
import { LevelLegend, RangePicker, useMarket } from "./chart-parts";
import type { ChartLevel, ChartMarker } from "./options";
import { type ChartSeries, TimeChart } from "./time-chart";

const RANGE_WORDS: Record<PriceRange, string> = { "1D": "the last trading day, by minute", "5D": "five trading days, 5-minute bars", "1M": "one month, daily", "6M": "six months, daily", "1Y": "one year, daily" };

function priceLabel(n: number): string {
  return `$${n.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
}

/** The position's own levels: protection from the mandate as dashed lines with gold labels, the average cost in the account's gold. */
export function positionLevels(position: Position): ChartLevel[] {
  const pr = position.protection;
  const levels: ChartLevel[] = [{ key: "avg-cost", label: "Average cost", price: Number(position.avg_cost), tone: "account", meaning: "What you paid per unit" }];
  if (pr.stop_price) {
    levels.push({
      key: "stop",
      label: pr.kind === "crypto_stop_limit" ? "Stop-limit" : "Stop",
      price: Number(pr.stop_price),
      tone: "mandate",
      meaning: pr.kind === "crypto_stop_limit" && pr.limit_price ? `Protection sells here, at no less than ${price(pr.limit_price)}` : "Protection sells here",
    });
  }
  if (pr.take_profit_price) levels.push({ key: "take-profit", label: "Take-profit", price: Number(pr.take_profit_price), tone: "mandate", meaning: "Protection takes the gain here" });
  return levels;
}

function markerTime(fillAt: string, range: PriceRange, bars: Array<Bar | DailyBar>): number | string | null {
  const t = unixOf(fillAt);
  if (range === "1M" || range === "6M" || range === "1Y") {
    const day = etParts(t).date;
    return bars.some((b) => "day" in b && b.day === day) ? day : null;
  }
  const size = range === "5D" ? 300 : 60;
  const bucket = t - (t % size);
  return bars.some((b) => "time" in b && b.time === bucket) ? bucket : null;
}

export function PositionChart({ agent, position }: { agent: Agent; position: Position }) {
  const market = useMarket();
  const [range, setRange] = useState<PriceRange>("1D");
  const symbol = market.symbols[position.instrument.symbol];
  const bars = useMemo(() => (symbol ? priceWindow(symbol.minute, symbol.daily, range) : []), [symbol, range]);
  const levels = useMemo(() => positionLevels(position), [position]);
  const { drawn, offChart } = useMemo(() => drawable(levels, closes(bars), 0.06), [levels, bars]);
  const markers = useMemo<ChartMarker[]>(
    () =>
      agent.fills
        .filter((f) => f.instrument.symbol === position.instrument.symbol)
        .flatMap((f) => {
          const time = markerTime(f.at, range, bars);
          return time === null ? [] : [{ time, side: f.side, text: `${f.side === "buy" ? "Buy" : "Sell"} ${f.qty}` }];
        }),
    [agent.fills, position.instrument.symbol, range, bars],
  );
  const series = useMemo<ChartSeries>(() => ({ kind: "candles", bars }), [bars]);
  const last = bars[bars.length - 1];
  const summary = last
    ? `${position.instrument.symbol} over ${RANGE_WORDS[range]}: last close ${priceLabel(last.close)}, low ${priceLabel(Math.min(...bars.map((b) => b.low)))}, high ${priceLabel(Math.max(...bars.map((b) => b.high)))}. ${markers.length} of this agent's fills are marked.`
    : `No bars for ${position.instrument.symbol}.`;
  return (
    <div data-slot="position-chart" className="grid gap-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-sm text-muted-foreground">
          {position.instrument.asset_class === "crypto"
            ? "Crypto trades around the clock; there is no session to wait for."
            : "Pre-market and after-hours candles are grey; the regular session is 09:30 to 16:00 ET."}
        </p>
        <RangePicker label="Price range" value={range} options={PRICE_RANGES} onChange={setRange} />
      </div>
      <TimeChart label={`${position.instrument.symbol} price, ${RANGE_WORDS[range]}`} summary={summary} series={series} levels={drawn} markers={markers} height={320} valueFormat={priceLabel} />
      <LevelLegend levels={drawn} offChart={offChart} format={priceLabel} />
    </div>
  );
}

/** Small and neutral: the instrument today and the limit the agent proposes, nothing that steers. */
export function ApprovalChart({ approval, className }: { approval: Pick<Approval, "bound" | "requested_at">; className?: string }) {
  const market = useMarket();
  const symbol = market.symbols[approval.bound.symbol];
  const points = useMemo(() => {
    if (!symbol) return [];
    const day = priceWindow(symbol.minute, symbol.daily, "1D") as Bar[];
    const requested = unixOf(approval.requested_at);
    return day.filter((b) => b.time <= requested).map((b) => ({ time: b.time, value: b.close }));
  }, [symbol, approval.requested_at]);
  const levels = useMemo<ChartLevel[]>(() => [{ key: "proposed-limit", label: "Proposed limit", price: Number(approval.bound.limit), tone: "proposal" }], [approval.bound.limit]);
  const series = useMemo<ChartSeries>(() => ({ kind: "line", tone: "neutral", points }), [points]);
  if (points.length < 2) return null;
  const last = points[points.length - 1].value;
  return (
    <figure data-slot="approval-chart" className={cn("grid gap-1.5", className)}>
      <figcaption className="text-caption text-muted-foreground">{approval.bound.symbol} today, up to the request. The dashed line is the proposed limit.</figcaption>
      <TimeChart
        label={`${approval.bound.symbol} today with the proposed limit`}
        summary={`${approval.bound.symbol} last traded at ${priceLabel(last)} when the request was made; the proposed limit is ${priceLabel(Number(approval.bound.limit))}.`}
        series={series}
        levels={levels}
        height={140}
        compact
        valueFormat={priceLabel}
      />
    </figure>
  );
}
