"use client";

import { useMemo, useSyncExternalStore } from "react";
import { type Market, buildMarket } from "@/fixtures/market";
import { useRuntime } from "@/lib/mock-runtime";
import { cn } from "@/lib/utils";
import { type ChartLevel, usdLabel } from "./options";

export function useMarket(): Market {
  const { ws } = useRuntime();
  return useMemo(() => buildMarket(ws), [ws]);
}

function watchCvd(onChange: () => void): () => void {
  const observer = new MutationObserver(onChange);
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-cvd"] });
  return () => observer.disconnect();
}

/** Canvas cannot follow the CSS remap, so charts read the colour-blind friendly flag from `<html data-cvd>`. */
export function useColourBlind(): boolean {
  return useSyncExternalStore(
    watchCvd,
    () => document.documentElement.dataset.cvd === "on",
    () => false,
  );
}

export function RangePicker<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: ReadonlyArray<{ id: T; label: string }>;
  onChange: (value: T) => void;
}) {
  return (
    <div role="group" aria-label={label} data-slot="range-picker" className="inline-flex border border-foreground">
      {options.map((o) => {
        const on = o.id === value;
        return (
          <button
            key={o.id}
            type="button"
            aria-pressed={on}
            onClick={() => onChange(o.id)}
            className={cn(
              "h-9 min-w-11 px-2 text-sm font-semibold outline-none focus-visible:ring-3 focus-visible:ring-ring focus-visible:ring-inset max-sm:h-11",
              on ? "bg-foreground text-background" : "bg-card text-foreground hover:bg-muted",
            )}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

const SWATCH = {
  mandate: "bg-mandate-marker",
  account: "bg-lapis",
  proposal: "bg-ink",
} as const;

/** Every level drawn on a chart, in words, with the ones outside its range said to be so. */
export function LevelLegend({ levels, offChart = [], format = usdLabel, className }: { levels: ChartLevel[]; offChart?: ChartLevel[]; format?: (n: number) => string; className?: string }) {
  const all = [...levels.map((l) => ({ l, drawn: true })), ...offChart.map((l) => ({ l, drawn: false }))].sort((a, b) => b.l.price - a.l.price);
  if (all.length === 0) return null;
  return (
    <ul data-slot="level-legend" className={cn("grid gap-1 text-sm", className)}>
      {all.map(({ l, drawn }) => (
        <li key={l.key} data-level={l.key} data-drawn={drawn} className="grid grid-cols-[0.75rem_minmax(0,1fr)_auto] items-baseline gap-2">
          <span aria-hidden className={cn("size-3 self-center", SWATCH[l.tone])} />
          <span>
            <span className="font-semibold">{l.label}</span>
            {l.meaning ? <span className="text-muted-foreground">: {l.meaning}</span> : null}
            {drawn ? null : <span className="text-muted-foreground"> (outside the range shown)</span>}
          </span>
          <span className="font-mono tabular">{format(l.price)}</span>
        </li>
      ))}
    </ul>
  );
}

/** Loading draws the chart's shape and nothing else: no line until values arrive. */
export function ChartSkeleton({ height = 280, label = "Loading chart" }: { height?: number; label?: string }) {
  return (
    <div data-slot="chart-skeleton" aria-busy="true" className="grid gap-1.5">
      <span className="h-5" />
      <div className="w-full bg-muted" style={{ height }} role="img" aria-label={label} />
    </div>
  );
}

/** Lightweight Charts asks for a link to TradingView where the charts appear (NOTICE). */
export function ChartCredit({ className }: { className?: string }) {
  return (
    <p data-slot="chart-credit" className={cn("text-caption text-muted-foreground", className)}>
      Charts:{" "}
      <a href="https://www.tradingview.com/" className="underline underline-offset-2 hover:text-foreground" rel="noreferrer" target="_blank">
        TradingView Lightweight Charts™
      </a>
    </p>
  );
}
