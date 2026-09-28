"use client";

import { useCallback, useId, useState } from "react";
import { useReducedMotion } from "motion/react";
import {
  AreaSeries,
  CandlestickSeries,
  type IChartApi,
  type ISeriesApi,
  LineSeries,
  type MouseEventParams,
  type SeriesType,
  type Time,
  createChart,
  createSeriesMarkers,
} from "lightweight-charts";
import type { Bar, DailyBar, Point } from "@/fixtures/market";
import { cn } from "@/lib/utils";
import { useColourBlind } from "./chart-parts";
import {
  CHART_FONT,
  type ChartLevel,
  type ChartMarker,
  type Tone,
  areaData,
  areaOptions,
  autoscaleWith,
  baseOptions,
  candleData,
  candleOptions,
  formatTime,
  lineOptions,
  markersFor,
  priceLineFor,
  usdLabel,
} from "./options";

export type ChartSeries =
  | { kind: "area"; tone: Tone; points: Point[] }
  | { kind: "line"; tone: Tone; points: Point[] }
  | { kind: "candles"; bars: Array<Bar | DailyBar> };

const NO_LEVELS: ChartLevel[] = [];
const NO_MARKERS: ChartMarker[] = [];

interface Readout {
  time: string;
  text: string;
}

function readoutFor(param: MouseEventParams<Time>, api: ISeriesApi<SeriesType>, format: (n: number) => string): Readout | null {
  if (!param.time) return null;
  const datum = param.seriesData.get(api);
  if (!datum) return null;
  if ("close" in datum) {
    return { time: formatTime(param.time), text: `Open ${format(datum.open)}, high ${format(datum.high)}, low ${format(datum.low)}, close ${format(datum.close)}` };
  }
  if ("value" in datum) return { time: formatTime(param.time), text: format(datum.value) };
  return null;
}

function lastReadout(series: ChartSeries, format: (n: number) => string): Readout | null {
  if (series.kind === "candles") {
    const b = series.bars[series.bars.length - 1];
    if (!b) return null;
    return { time: formatTime(("day" in b ? b.day : b.time) as Time), text: `Close ${format(b.close)}` };
  }
  const p = series.points[series.points.length - 1];
  return p ? { time: formatTime(p.time as Time), text: format(p.value) } : null;
}

/**
 * One chart. The canvas is decoration for sighted readers; `summary` says the same in words and
 * `levels` are listed beside the chart by the caller. The readout above the plot follows the
 * crosshair and otherwise shows the latest value, in ET. If the canvas cannot be drawn, the chart
 * says so and the figures around it stay.
 */
export function TimeChart({
  label,
  summary,
  series,
  levels = NO_LEVELS,
  markers = NO_MARKERS,
  height = 280,
  compact = false,
  valueFormat = usdLabel,
  className,
}: {
  label: string;
  summary: string;
  series: ChartSeries;
  levels?: ChartLevel[];
  markers?: ChartMarker[];
  height?: number;
  compact?: boolean;
  valueFormat?: (n: number) => string;
  className?: string;
}) {
  const reducedMotion = useReducedMotion() ?? false;
  const colourBlind = useColourBlind();
  const [readout, setReadout] = useState<Readout | null>(null);
  const [failedFor, setFailedFor] = useState<ChartSeries | null>(null);
  const summaryId = useId();

  /** The chart lives as long as its container and its inputs; a new input draws a new chart. */
  const attach = useCallback(
    (el: HTMLDivElement | null) => {
      if (!el) return;
      let chart: IChartApi | null = null;
      try {
        chart = createChart(el, baseOptions({ reducedMotion, compact, valueFormat }));
        let api: ISeriesApi<SeriesType>;
        if (series.kind === "candles") {
          const candles = chart.addSeries(CandlestickSeries, candleOptions(colourBlind));
          candles.setData(candleData(series.bars));
          api = candles;
        } else if (series.kind === "area") {
          const area = chart.addSeries(AreaSeries, areaOptions(series.tone));
          area.setData(areaData(series.points));
          api = area;
        } else {
          const line = chart.addSeries(LineSeries, lineOptions(series.tone));
          line.setData(areaData(series.points));
          api = line;
        }
        for (const level of levels) api.createPriceLine(priceLineFor(level));
        if (levels.length > 0) api.applyOptions({ autoscaleInfoProvider: autoscaleWith(levels) });
        if (markers.length > 0) createSeriesMarkers(api, markersFor(markers));
        chart.timeScale().fitContent();
        chart.subscribeCrosshairMove((param) => setReadout(readoutFor(param, api, valueFormat)));
        // The canvas draws with whatever face is ready; redraw once the plain-zero figures load.
        void document.fonts?.load(`12px ${CHART_FONT}`, "0123456789").then(() => chart?.applyOptions({ layout: { fontFamily: CHART_FONT } }));
      } catch {
        chart?.remove();
        chart = null;
        setFailedFor(series);
      }
      return () => {
        chart?.remove();
        chart = null;
      };
    },
    [series, levels, markers, reducedMotion, compact, valueFormat, colourBlind],
  );

  const failed = failedFor === series;
  const shown = readout ?? lastReadout(series, valueFormat);
  return (
    <div data-slot="time-chart" className={cn("grid gap-1.5", className)}>
      <p aria-hidden className="flex min-h-5 flex-wrap items-baseline gap-x-2 text-caption text-muted-foreground tabular">
        {shown ? (
          <>
            <span>{shown.time}</span>
            <span className="font-bold text-foreground">{shown.text}</span>
          </>
        ) : null}
      </p>
      <div
        ref={attach}
        role="img"
        aria-label={label}
        aria-describedby={summaryId}
        data-slot="chart-canvas"
        className="relative w-full bg-card"
        style={{ height }}
      />
      <p id={summaryId} className="sr-only">
        {summary}
      </p>
      {failed ? (
        <p role="status" data-slot="chart-failed" className="bg-muted px-3 py-2 text-sm">
          This chart could not be drawn in your browser. The figures on this page are current.
        </p>
      ) : null}
    </div>
  );
}
