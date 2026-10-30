"use client";

import { type ReactNode, useCallback, useMemo, useState } from "react";
import type { UTCTimestamp } from "lightweight-charts";
import { AsOf } from "@/components/domain/as-of";
import { HeroFigure, SignedMoney } from "@/components/domain/money";
import { Placeholder } from "@/components/domain/placeholders";
import type { Market, Point } from "@/fixtures/market";
import type { Workspace } from "@/fixtures/types";
import { add, dec, sub, toFixed } from "@/lib/decimal";
import { type Direction, direction, usd } from "@/lib/format";
import { EQUITY_RANGES, type EquityRange, equityWindow, rangeStart } from "@/lib/chart-data";
import { useRuntime } from "@/lib/mock-runtime";
import { cn } from "@/lib/utils";
import { ChartCredit, RangePicker, useMarket } from "./chart-parts";
import { formatTime, usdLabel } from "./options";
import { type ChartSeries, type ScrubPoint, TimeChart } from "./time-chart";

/**
 * The account equity chart, restored from before #3003 for the Portfolio page (DEC-2413): one hero
 * figure over one scrubbable line, as the dashboard drew it.
 */

const RANGE_WORDS: Record<EquityRange, string> = {
  "1D": "today",
  "1W": "past week",
  "1M": "past month",
  "3M": "past 3 months",
  "1Y": "past year",
  All: "since the first agent deployed",
};

/**
 * The ranges the Portfolio page offers: the fixture history is weeks long, so a year adds nothing
 * "All" does not show. A range longer than the history says "since" its first day.
 */
export const ACCOUNT_RANGES = EQUITY_RANGES.filter((r) => r.id !== "1Y");

/** Short enough on a phone that the range picker and the figures under it start on the first screen (DEC-207). */
const ACCOUNT_PHONE_HEIGHT = 200;

/** The account leads the Portfolio page's main column (DEC-2413). */
const ACCOUNT_HEIGHT = 280;

/** The page opens on today, the question an owner checks first (DEC-468). */
export const ACCOUNT_RANGE: EquityRange = "1D";

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
  return `${pct > 0 ? "+" : "−"}${text}%`;
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
 * The line's values are drawing values; the page's money figures come from the decimals.
 */
function EquityHero({
  points,
  words,
  range,
  onRange,
  footer,
  pulse,
}: {
  points: Point[];
  words: string;
  range: EquityRange;
  onRange: (range: EquityRange) => void;
  footer: ReactNode;
  pulse: boolean;
}) {
  const [scrub, setScrub] = useState<ScrubPoint>(null);
  const onScrub = useCallback((p: ScrubPoint) => setScrub(p), []);
  const trend = direction(((points.at(-1)?.value ?? 0) - (points[0]?.value ?? 0)).toFixed(2));
  const series = useMemo<ChartSeries>(() => ({ kind: "area", tone: "account", points, trend }), [points, trend]);
  const now = points[points.length - 1];
  const shown = scrub ?? now ?? null;
  const base = points[0]?.value ?? 0;
  const change = shown ? Math.round((shown.value - base) * 100) / 100 : 0;
  const scrubbing = scrub !== null;
  const pct = shown ? signedPercent(change, base) : null;
  const changeText = change.toFixed(2);
  const changeTone = direction(changeText);

  return (
    <section aria-labelledby="account-equity-title" data-slot="account-equity" data-scrubbing={scrubbing ? "" : undefined} className="@container grid content-start gap-5">
      <div className="grid gap-1.5">
        <h2 id="account-equity-title" className="text-sm font-medium text-muted-foreground">
          Account value
        </h2>
        {/* Proportional figures: tabular digits leave visible gaps at the display size, and the pill sits on its own line, so nothing moves with the width. */}
        <p className="text-display proportional-nums lining-nums" data-slot="account-equity-value">
          {shown ? <HeroFigure value={usdLabel(shown.value)} instant={scrubbing} /> : "—"}
        </p>
        {points.length >= 2 && shown ? (
          <p className="flex items-baseline gap-2 pt-1 text-sm">
            <span data-slot="hero-change" data-tone={changeTone} className={cn("inline-flex w-fit min-w-0 max-w-full flex-wrap items-baseline gap-x-2 rounded-full px-3 py-1 font-medium", PILL[changeTone])}>
              <SignedMoney value={changeText} instant={scrubbing} />
              {pct ? <span className="font-mono tabular">({pct})</span> : null}
              <span className="text-muted-foreground tabular" data-slot="hero-when">
                {scrubbing ? formatTime(shown.time as UTCTimestamp) : words}
              </span>
            </span>
            <Placeholder name="performance" className="shrink-0" />
          </p>
        ) : (
          <Placeholder name="performance" className="w-fit" />
        )}
      </div>
      <TimeChart
        label={`Account value, ${words}`}
        summary={describe(points, "Account value", words)}
        series={series}
        axis={false}
        onScrub={onScrub}
        pulse={pulse}
        height={ACCOUNT_HEIGHT}
        phoneHeight={ACCOUNT_PHONE_HEIGHT}
        className="-mx-1"
      />
      <RangePicker label="Account value range" value={range} options={ACCOUNT_RANGES} onChange={onRange} />
      <div className="text-caption leading-6 text-muted-foreground *:mr-1.5 *:align-middle">{footer}</div>
    </section>
  );
}

/**
 * The account as one line, on paper: every agent's equity together, plus what no agent manages. The
 * fixture holds that part flat, so the chart ends at the broker's figure and moves with the agents.
 * Over the workspace API there is no equity history to draw (`useMarket` is `null`), so it renders
 * nothing and the page says the history is not reported.
 */
export function AccountEquityChart() {
  const market = useMarket();
  return market ? <AccountEquityChartDrawn market={market} /> : null;
}

function AccountEquityChartDrawn({ market }: { market: Market }) {
  const { ws, now } = useRuntime();
  const stale = ws.health.market_data.state !== "ok";
  const [range, setRange] = useState<EquityRange>(ACCOUNT_RANGE);
  const unmanaged = unmanagedEquity(ws);
  const all = useMemo(() => {
    const offset = Number(unmanaged);
    return market.account.map((p) => ({ time: p.time, value: Math.round((p.value + offset) * 100) / 100 }));
  }, [market, unmanaged]);
  const points = useMemo(() => equityWindow(all, range, market.end), [all, range, market.end]);
  const words = rangeWord(points, all, range, market.end);
  return (
    <EquityHero
      points={points}
      words={words}
      range={range}
      onRange={setRange}
      pulse={!stale}
      footer={
        <>
          <span>Simulated funds on paper.</span>
          <span data-slot="unmanaged" className="max-lg:hidden">
            Includes <span className="font-mono tabular">{usd(unmanaged)}</span> no agent manages.
          </span>
          <AsOf at={ws.health.market_data.as_of} now={now} stale={stale} />
          <ChartCredit className="mt-1" />
        </>
      }
    />
  );
}
