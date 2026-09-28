"use client";

import { useMemo, useState } from "react";
import { AsOf } from "@/components/domain/as-of";
import { SignedMoney } from "@/components/domain/money";
import { FixtureTag, Placeholder } from "@/components/domain/placeholders";
import type { Point } from "@/fixtures/market";
import type { Agent, Workspace } from "@/fixtures/types";
import { add, dec, sub, toFixed } from "@/lib/decimal";
import { usd } from "@/lib/format";
import { EQUITY_RANGES, type EquityRange, drawable, equityWindow, mandateLevels } from "@/lib/chart-data";
import { useRuntime } from "@/lib/mock-runtime";
import { ChartCredit, ChartSkeleton, LevelLegend, RangePicker, useMarket } from "./chart-parts";
import { usdLabel } from "./options";
import { type ChartSeries, TimeChart } from "./time-chart";

const RANGE_WORDS: Record<EquityRange, string> = { "1D": "today", "1W": "the last week", "1M": "the last month", All: "since the first agent deployed" };

function describe(points: Point[], what: string, range: EquityRange): string {
  if (points.length === 0) return `No ${what} recorded for ${RANGE_WORDS[range]}.`;
  const first = points[0].value;
  const last = points[points.length - 1].value;
  const values = points.map((p) => p.value);
  const change = last - first;
  const word = change > 0 ? "a gain" : change < 0 ? "a loss" : "no change";
  return `${what} ${RANGE_WORDS[range]}: from ${usdLabel(first)} to ${usdLabel(last)}, ${word}${change === 0 ? "" : ` of ${usdLabel(Math.abs(change))}`}. Lowest ${usdLabel(Math.min(...values))}, highest ${usdLabel(Math.max(...values))}. Simulated funds.`;
}

function Change({ points, range }: { points: Point[]; range: EquityRange }) {
  if (points.length < 2) return null;
  const change = points[points.length - 1].value - points[0].value;
  return (
    <span className="inline-flex flex-wrap items-baseline gap-1.5 text-sm">
      <SignedMoney value={change.toFixed(2)} className="font-semibold" />
      <span className="text-muted-foreground">{RANGE_WORDS[range]}</span>
    </span>
  );
}

function useStale() {
  const { ws, now } = useRuntime();
  return { stale: ws.health.market_data.state !== "ok", asOf: ws.health.market_data.as_of, now };
}

/** Cash and holdings on the account that no agent manages: the broker's equity less the agents'. */
export function unmanagedEquity(ws: Workspace): string {
  return toFixed(sub(dec(ws.connection.account_equity), add(...ws.agents.map((a) => dec(a.state.equity)))), 2);
}

/**
 * The account in navy, on paper: every agent's equity together, plus what no agent manages. The
 * fixture holds that part flat, so the chart ends at the broker's figure and moves with the agents.
 */
export function AccountEquityChart() {
  const { ws } = useRuntime();
  const market = useMarket();
  const { stale, asOf, now } = useStale();
  const [range, setRange] = useState<EquityRange>("1D");
  const unmanaged = unmanagedEquity(ws);
  const points = useMemo(() => {
    const offset = Number(unmanaged);
    return equityWindow(market.account, range, market.end).map((p) => ({ time: p.time, value: Math.round((p.value + offset) * 100) / 100 }));
  }, [market, range, unmanaged]);
  const series = useMemo<ChartSeries>(() => ({ kind: "area", tone: "account", points }), [points]);
  const last = points[points.length - 1]?.value ?? 0;
  return (
    <section aria-labelledby="account-equity-title" data-slot="account-equity" className="reveal grid content-start gap-3 bg-card px-4 py-4 sm:px-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="grid gap-1">
          <h2 id="account-equity-title" className="text-h2">
            Account equity
          </h2>
          <p className="text-[2.25rem] leading-none font-semibold tabular" data-slot="account-equity-value">
            {usdLabel(last)}
          </p>
          <Change points={points} range={range} />
        </div>
        <RangePicker label="Account equity range" value={range} options={EQUITY_RANGES} onChange={setRange} />
      </div>
      <TimeChart label={`Account equity, ${RANGE_WORDS[range]}`} summary={describe(points, "Account equity", range)} series={series} height={240} />
      <p className="text-caption text-muted-foreground" data-slot="unmanaged">
        Your agents&apos; equity plus <span className="font-mono tabular">{usd(unmanaged)}</span> in cash and holdings no agent manages.
      </p>
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-caption text-muted-foreground">
        <Placeholder name="performance" />
        <span>Simulated funds on paper.</span>
        <AsOf at={asOf} now={now} stale={stale} />
        <FixtureTag />
        <ChartCredit className="ml-auto" />
      </div>
    </section>
  );
}

/** One agent's equity with its mandate's levels drawn as labelled lines and listed in words. */
export function AgentEquityChart({ agent }: { agent: Agent }) {
  const market = useMarket();
  const { stale, asOf, now } = useStale();
  const [range, setRange] = useState<EquityRange>("1D");
  const points = useMemo(() => equityWindow(market.equity[agent.agent_id] ?? [], range, market.end), [market, agent.agent_id, range]);
  const levels = useMemo(() => mandateLevels(agent), [agent]);
  const { drawn, offChart } = useMemo(() => drawable(levels, points.map((p) => p.value)), [levels, points]);
  const series = useMemo<ChartSeries>(() => ({ kind: "area", tone: "agent", points }), [points]);
  return (
    <section aria-labelledby="agent-equity-title" data-slot="agent-equity" className="reveal grid content-start gap-3 bg-card px-3 py-3 sm:px-4 sm:py-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="grid gap-1">
          <h2 id="agent-equity-title" className="text-h2">
            Equity against your mandate
          </h2>
          <Change points={points} range={range} />
        </div>
        <RangePicker label="Equity range" value={range} options={EQUITY_RANGES} onChange={setRange} />
      </div>
      {points.length < 2 ? (
        <p className="bg-muted px-3 py-3 text-sm">No equity history yet. It starts when the agent deploys.</p>
      ) : (
        <TimeChart label={`${agent.label} equity, ${RANGE_WORDS[range]}, with mandate levels`} summary={describe(points, `${agent.label} equity`, range)} series={series} levels={drawn} height={280} />
      )}
      <LevelLegend levels={drawn} offChart={offChart} />
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-caption text-muted-foreground">
        <Placeholder name="performance" />
        <AsOf at={asOf} now={now} stale={stale} />
      </div>
    </section>
  );
}

export function EquityChartSkeleton() {
  return (
    <div className="grid gap-3 bg-card px-4 py-4">
      <ChartSkeleton height={240} label="Loading equity" />
    </div>
  );
}
