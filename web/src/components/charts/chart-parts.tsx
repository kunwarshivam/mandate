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

const colourBlindNow = () => document.documentElement.dataset.cvd === "on";
const chartModeNow = (): ThemeMode => (document.documentElement.dataset.mode === "dark" ? "dark" : "light");

/**
 * What a chart reads while the server renders and while the client hydrates. The server has no
 * document and draws no canvas, so it takes the default. Hydrating, the client reads `<html>`
 * itself: the head script set it before React ran, and the canvas is not in the server's markup,
 * so nothing can mismatch. Taking the default there drew a light chart into a dark page, then tore
 * it down for the dark one, and the plot stayed blank while the page loaded (plan B, the account
 * chart's dark-mode paint).
 */
function hydrating<T>(now: () => T, fallback: T): () => T {
  return () => (typeof document === "undefined" ? fallback : now());
}

const colourBlindHydrating = hydrating(colourBlindNow, false);
const chartModeHydrating = hydrating<ThemeMode>(chartModeNow, "light");

/** Canvas cannot follow the CSS remap, so charts read the colour-blind friendly flag from `<html data-cvd>`. */
export function useColourBlind(): boolean {
  return useSyncExternalStore(watchCvd, colourBlindNow, colourBlindHydrating);
}

/** Charts redraw from the palette when `<html data-mode>` changes, for the same reason. */
export function useChartMode(): ThemeMode {
  return useSyncExternalStore(watchMode, chartModeNow, chartModeHydrating);
}

/**
 * A segmented control. The current range sits on the sun highlight, in ink type, and the pill
 * springs to the next one (350 ms with a little bounce; with reduced motion it jumps). The focus ring
 * stands off the button, so it never reads as a second selection. Each option is a pressed-state
 * button, so the group reads as one control with one choice.
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
              "press relative h-11 min-w-11 flex-1 rounded-lg px-3 font-mono text-sm font-medium tabular outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-card sm:h-9 sm:flex-none",
              on ? "text-highlight-foreground" : "text-muted-foreground hover:text-foreground",
            )}
          >
            {on ? (
              <motion.span
                layoutId={pill}
                aria-hidden
                data-slot="range-pill"
                className="absolute inset-0 rounded-lg bg-highlight"
                transition={{ type: "spring", duration: 0.35, bounce: 0.25 }}
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
          <span aria-hidden className={cn("h-0.5 w-3 self-center rounded-xs", SWATCH[l.tone])} />
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
