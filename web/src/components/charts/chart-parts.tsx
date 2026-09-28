"use client";

import { useId, useMemo, useSyncExternalStore } from "react";
import { motion } from "motion/react";
import { type Market, buildMarket } from "@/fixtures/market";
import { useRuntime } from "@/lib/mock-runtime";
import type { ThemeMode } from "@/lib/theme";
import { cn } from "@/lib/utils";
import { type ChartLevel, usdLabel } from "./options";

export function useMarket(): Market {
  const { ws } = useRuntime();
  return useMemo(() => buildMarket(ws), [ws]);
}

function watchRoot(attribute: string): (onChange: () => void) => () => void {
  return (onChange) => {
    const observer = new MutationObserver(onChange);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: [attribute] });
    return () => observer.disconnect();
  };
}

const watchCvd = watchRoot("data-cvd");
const watchMode = watchRoot("data-mode");

/** Canvas cannot follow the CSS remap, so charts read the colour-blind friendly flag from `<html data-cvd>`. */
export function useColourBlind(): boolean {
  return useSyncExternalStore(
    watchCvd,
    () => document.documentElement.dataset.cvd === "on",
    () => false,
  );
}

/** Charts redraw from the palette when `<html data-mode>` changes, for the same reason. */
export function useChartMode(): ThemeMode {
  return useSyncExternalStore(
    watchMode,
    () => (document.documentElement.dataset.mode === "dark" ? "dark" : "light"),
    () => "light",
  );
}

/**
 * A quiet segmented control. The current range sits on a pale gold pill that glides to the next one
 * (a 300 ms spring; with reduced motion it jumps). Each option is a pressed-state button, so
 * the group reads as one control with one choice.
 */
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
  const pill = useId();
  return (
    <div role="group" aria-label={label} data-slot="range-picker" className="flex w-full max-w-md items-center justify-between gap-1 sm:w-auto sm:justify-start">
      {options.map((o) => {
        const on = o.id === value;
        return (
          <button
            key={o.id}
            type="button"
            aria-pressed={on}
            onClick={() => onChange(o.id)}
            className={cn(
              "press relative h-11 min-w-11 flex-1 rounded-full px-3 font-mono text-sm font-medium tabular outline-none focus-visible:ring-3 focus-visible:ring-ring sm:h-9 sm:flex-none",
              on ? "text-lapis" : "text-muted-foreground hover:text-foreground",
            )}
          >
            {on ? (
              <motion.span
                layoutId={pill}
                aria-hidden
                className="absolute inset-0 rounded-full bg-lapis-soft ring-1 ring-inset ring-lapis-line"
                transition={{ type: "spring", duration: 0.3, bounce: 0.1 }}
              />
            ) : null}
            <span className="relative">{o.label}</span>
          </button>
        );
      })}
    </div>
  );
}

const SWATCH = {
  mandate: "bg-muted-foreground",
  account: "bg-lapis-line",
  proposal: "bg-ink",
} as const;

/** Every level drawn on a chart, in words, with the ones outside its range said to be so. */
export function LevelLegend({ levels, offChart = [], format = usdLabel, className }: { levels: ChartLevel[]; offChart?: ChartLevel[]; format?: (n: number) => string; className?: string }) {
  const all = [...levels.map((l) => ({ l, drawn: true })), ...offChart.map((l) => ({ l, drawn: false }))].sort((a, b) => b.l.price - a.l.price);
  if (all.length === 0) return null;
  return (
    <ul data-slot="level-legend" className={cn("grid gap-x-8 gap-y-1.5 text-caption @lg:grid-cols-2", className)}>
      {all.map(({ l, drawn }) => (
        <li key={l.key} data-level={l.key} data-drawn={drawn} className={cn("grid grid-cols-[0.75rem_minmax(0,1fr)_auto] items-baseline gap-2", !drawn && "text-muted-foreground")}>
          <span aria-hidden className={cn("h-0.5 w-3 self-center rounded-full", SWATCH[l.tone])} />
          <span>
            <span className="font-medium">{l.label}</span>
            {l.meaning ? <span className="text-muted-foreground">: {l.meaning}</span> : null}
            {drawn ? null : <span> (outside the range shown)</span>}
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
      <div className="w-full rounded-lg bg-background" style={{ height }} role="img" aria-label={label} />
    </div>
  );
}

/** Lightweight Charts asks for a link to TradingView where the charts appear (NOTICE). */
export function ChartCredit({ className }: { className?: string }) {
  return (
    <p data-slot="chart-credit" className={cn("text-caption text-muted-foreground", className)}>
      Charts:{" "}
      <a href="https://www.tradingview.com/" className="underline decoration-border hover:text-foreground hover:decoration-current" rel="noreferrer" target="_blank">
        TradingView Lightweight Charts™
      </a>
    </p>
  );
}
