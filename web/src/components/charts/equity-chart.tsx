"use client";

import { type ReactNode, useCallback, useMemo, useState } from "react";
import type { UTCTimestamp } from "lightweight-charts";
import { AsOf } from "@/components/domain/as-of";
import { HeroFigure, SignedMoney } from "@/components/domain/money";
import { FixtureTag, Placeholder } from "@/components/domain/placeholders";
import type { Point } from "@/fixtures/market";
import type { Agent, Workspace } from "@/fixtures/types";
import { add, dec, sub, toFixed } from "@/lib/decimal";
import { type Direction, direction, usd } from "@/lib/format";
import { EQUITY_RANGES, type EquityRange, drawable, equityWindow, mandateLevels, rangeStart } from "@/lib/chart-data";
import { useRuntime } from "@/lib/mock-runtime";
import { cn } from "@/lib/utils";
import type { ChartLevel } from "./options";
import { ChartCredit, ChartSkeleton, LevelLegend, RangePicker, useMarket } from "./chart-parts";
import { formatTime, usdLabel } from "./options";
import { type ChartSeries, type ScrubPoint, TimeChart } from "./time-chart";

const RANGE_WORDS: Record<EquityRange, string> = {
  "1D": "today",
  "1W": "past week",
  "1M": "past month",
  "3M": "past 3 months",
  "1Y": "past year",
  All: "since the first agent deployed",
};

/** The change's soft pill: its tint by sign, and the type in the gain or loss colour. */
const PILL: Record<Direction, string> = {
  gain: "bg-gain-soft text-gain",
  loss: "bg-loss-soft text-loss",
  flat: "bg-muted text-foreground",
};

const DAY_WORD = new Intl.DateTimeFormat("en-US", { timeZone: "America/New_York", month: "short", day: "numeric" });

/** What the change is measured over: the range's own words, or "since" the day history starts. */
function rangeWord(points: Point[], all: Point[], range: EquityRange, end: number): string {
  const start = rangeStart(all, range, end);
  if (start?.clipped && points[0]) return `since ${DAY_WORD.format(new Date(points[0].time * 1000))}`;
  return RANGE_WORDS[range];
}

function describe(points: Point[], what: string, words: string): string {
  if (points.length === 0) return `No ${what} recorded ${words}.`;
  const first = points[0].value;
  const last = points[points.length - 1].value;
  const values = points.map((p) => p.value);
  const change = last - first;
  const word = change > 0 ? "a gain" : change < 0 ? "a loss" : "no change";
  return `${what} ${words}: from ${usdLabel(first)} to ${usdLabel(last)}, ${word}${change === 0 ? "" : ` of ${usdLabel(Math.abs(change))}`}. Lowest ${usdLabel(Math.min(...values))}, highest ${usdLabel(Math.max(...values))}. Simulated funds.`;
}

function signedPercent(change: number, base: number): string | null {
  if (base === 0) return null;
  const pct = (change / base) * 100;
  const text = Math.abs(pct).toFixed(2);
  if (text === "0.00") return "0.00%";
  return `${pct > 0 ? "+" : "\u2212"}${text}%`;
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
 * The signature of the calm system: one hero figure over one scrubbable line. Holding the pointer
 * or a finger on the line moves the figure, its change, and the date to that point, instantly;
 * letting go returns them to now, where a live change rolls in. The change is always measured from
 * the start of the range shown, with its sign, the word, and the performance disclosure beside it.
 */
function EquityHero({
  slot,
  titleId,
  title,
  valueSlot,
  points,
  words,
  chartLabel,
  summary,
  tone,
  levels,
  legend,
  range,
  onRange,
  rangeLabel,
  empty,
  footer,
}: {
  slot: string;
  titleId: string;
  title: string;
  valueSlot: string;
  points: Point[];
  words: string;
  chartLabel: string;
  summary: string;
  tone: "account" | "agent";
  levels?: ChartLevel[];
  legend?: ReactNode;
  range: EquityRange;
  onRange: (range: EquityRange) => void;
  rangeLabel: string;
  empty?: ReactNode;
  footer: ReactNode;
}) {
  const [scrub, setScrub] = useState<ScrubPoint>(null);
  const onScrub = useCallback((p: ScrubPoint) => setScrub(p), []);
  const series = useMemo<ChartSeries>(() => ({ kind: "area", tone, points }), [points, tone]);
  const now = points[points.length - 1];
  const shown = scrub ?? now ?? null;
  const base = points[0]?.value ?? 0;
  const change = shown ? Math.round((shown.value - base) * 100) / 100 : 0;
  const scrubbing = scrub !== null;
  const pct = shown ? signedPercent(change, base) : null;
  const changeText = change.toFixed(2);
  const changeTone = direction(changeText);

  return (
    <section aria-labelledby={titleId} data-slot={slot} data-scrubbing={scrubbing ? "" : undefined} className="@container grid content-start gap-5">
      <div className="grid gap-1.5">
        <h2 id={titleId} className="text-sm font-medium text-muted-foreground">
          {title}
        </h2>
        <p className="text-display tabular" data-slot={valueSlot}>
          {shown ? <HeroFigure value={usdLabel(shown.value)} instant={scrubbing} /> : "—"}
        </p>
        {points.length >= 2 && shown ? (
          // The disclosure keeps a line of its own until the widest scrubbed pill fits beside it, so it never hops lines mid-scrub.
          <p className="flex flex-col items-start gap-1.5 pt-1 @xl:flex-row @xl:items-center @xl:gap-2">
            <span data-slot="hero-change" data-tone={changeTone} className={cn("inline-flex w-fit max-w-full flex-wrap items-baseline gap-x-2 rounded-full px-3 py-1 text-sm font-medium", PILL[changeTone])}>
              <SignedMoney value={changeText} instant={scrubbing} />
              {pct ? <span className="font-mono tabular">({pct})</span> : null}
              <span className="text-muted-foreground tabular" data-slot="hero-when">
                {scrubbing ? formatTime(shown.time as UTCTimestamp) : words}
              </span>
            </span>
            <Placeholder name="performance" />
          </p>
        ) : (
          <Placeholder name="performance" className="w-fit" />
        )}
      </div>
      {empty ?? (
        <TimeChart
          label={chartLabel}
          summary={summary}
          series={series}
          levels={levels}
          axis={(levels?.length ?? 0) > 0}
          onScrub={onScrub}
          height={tone === "account" ? 260 : 280}
          className="-mx-1"
        />
      )}
      <RangePicker label={rangeLabel} value={range} options={EQUITY_RANGES} onChange={onRange} />
      {legend}
      <div className="text-caption leading-6 text-muted-foreground *:mr-1.5 *:align-middle">{footer}</div>
    </section>
  );
}

/**
 * The account as a gold line, on paper: every agent's equity together, plus what no agent manages. The
 * fixture holds that part flat, so the chart ends at the broker's figure and moves with the agents.
 */
export function AccountEquityChart() {
  const { ws } = useRuntime();
  const market = useMarket();
  const { stale, asOf, now } = useStale();
  const [range, setRange] = useState<EquityRange>("1D");
  const unmanaged = unmanagedEquity(ws);
  const all = useMemo(() => {
    const offset = Number(unmanaged);
    return market.account.map((p) => ({ time: p.time, value: Math.round((p.value + offset) * 100) / 100 }));
  }, [market, unmanaged]);
  const points = useMemo(() => equityWindow(all, range, market.end), [all, range, market.end]);
  const words = rangeWord(points, all, range, market.end);
  return (
    <EquityHero
      slot="account-equity"
      titleId="account-equity-title"
      title="Account equity"
      valueSlot="account-equity-value"
      points={points}
      words={words}
      chartLabel={`Account equity, ${words}`}
      summary={describe(points, "Account equity", words)}
      tone="account"
      range={range}
      onRange={setRange}
      rangeLabel="Account equity range"
      footer={
        <>
          <span>Simulated funds on paper.</span>
          <span data-slot="unmanaged">
            Includes <span className="font-mono tabular">{usd(unmanaged)}</span> no agent manages.
          </span>
          <AsOf at={asOf} now={now} stale={stale} />
          <FixtureTag />
          <ChartCredit className="mt-1" />
        </>
      }
    />
  );
}

/** One agent's equity with its mandate's levels drawn as labelled dashed lines and listed in words. */
export function AgentEquityChart({ agent }: { agent: Agent }) {
  const market = useMarket();
  const { stale, asOf, now } = useStale();
  const [range, setRange] = useState<EquityRange>("1D");
  const all = useMemo(() => market.equity[agent.agent_id] ?? [], [market.equity, agent.agent_id]);
  const points = useMemo(() => equityWindow(all, range, market.end), [all, range, market.end]);
  const levels = useMemo(() => mandateLevels(agent), [agent]);
  const { drawn, offChart } = useMemo(() => drawable(levels, points.map((p) => p.value)), [levels, points]);
  const words = rangeWord(points, all, range, market.end);
  return (
    <EquityHero
      slot="agent-equity"
      titleId="agent-equity-title"
      title="Equity against your mandate"
      valueSlot="agent-equity-value"
      points={points}
      words={words}
      chartLabel={`${agent.label} equity, ${words}, with mandate levels`}
      summary={describe(points, `${agent.label} equity`, words)}
      tone="agent"
      levels={drawn}
      range={range}
      onRange={setRange}
      rangeLabel="Equity range"
      empty={points.length < 2 ? <p className="rounded-lg bg-background px-4 py-6 text-sm text-muted-foreground">No equity history yet. It starts when the agent deploys.</p> : undefined}
      legend={<LevelLegend levels={drawn} offChart={offChart} />}
      footer={
        <>
          <span>Paper P&amp;L, simulated.</span>
          <AsOf at={asOf} now={now} stale={stale} />
        </>
      }
    />
  );
}

export function EquityChartSkeleton() {
  return (
    <div className="@container grid gap-5">
      <div className="grid gap-2">
        <span className="h-4 w-28 rounded-sm bg-muted" />
        <span className="h-[1lh] w-[5.5em] rounded-md bg-muted text-display" />
      </div>
      <ChartSkeleton height={260} label="Loading equity" />
    </div>
  );
}
