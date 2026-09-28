"use client";

import { type PointerEvent, useCallback, useId, useLayoutEffect, useRef, useState } from "react";
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
  type UTCTimestamp,
  createChart,
  createSeriesMarkers,
} from "lightweight-charts";
import type { Bar, DailyBar, Point } from "@/fixtures/market";
import { nearestPoint } from "@/lib/chart-data";
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

/** A point the owner is holding the crosshair on; `null` is "now". */
export type ScrubPoint = Point | null;

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

function scrubFor(param: MouseEventParams<Time>, api: ISeriesApi<SeriesType>): ScrubPoint {
  if (typeof param.time !== "number") return null;
  const datum = param.seriesData.get(api);
  return datum && "value" in datum ? { time: param.time, value: datum.value } : null;
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

interface Live {
  chart: IChartApi;
  api: ISeriesApi<SeriesType>;
  points: Point[];
}

/**
 * One chart. The canvas is decoration for sighted readers; `summary` says the same in words and
 * `levels` are listed beside the chart by the caller. If the canvas cannot be drawn, the chart
 * says so and the figures around it stay.
 *
 * Without `onScrub`, a readout above the plot follows the crosshair and otherwise shows the latest
 * value, in ET. With `onScrub`, the chart is a hero: the caller's figure reads the point under the
 * pointer or finger, and letting go (or leaving the plot) hands back `null`, which is "now". A
 * mouse moves Lightweight Charts' own crosshair; a finger or pen drags it along the line, while a
 * vertical swipe still scrolls the page.
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
  onScrub,
  axis = true,
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
  onScrub?: (point: ScrubPoint) => void;
  /** A hero chart's price scale, which the mandate levels need for their labels. */
  axis?: boolean;
  className?: string;
}) {
  const reducedMotion = useReducedMotion() ?? false;
  const colourBlind = useColourBlind();
  const [readout, setReadout] = useState<Readout | null>(null);
  const [failedFor, setFailedFor] = useState<ChartSeries | null>(null);
  const summaryId = useId();
  const live = useRef<Live | null>(null);
  const scrubRef = useRef(onScrub);
  useLayoutEffect(() => {
    scrubRef.current = onScrub;
  });
  const hero = onScrub !== undefined;

  /** The chart lives as long as its container and its inputs; a new input draws a new chart. */
  const attach = useCallback(
    (el: HTMLDivElement | null) => {
      if (!el) return;
      let chart: IChartApi | null = null;
      try {
        chart = createChart(el, baseOptions({ reducedMotion, compact, valueFormat, hero: hero ? { axis } : undefined }));
        let api: ISeriesApi<SeriesType>;
        if (series.kind === "candles") {
          const candles = chart.addSeries(CandlestickSeries, candleOptions(colourBlind));
          candles.setData(candleData(series.bars));
          api = candles;
        } else if (series.kind === "area") {
          const area = chart.addSeries(AreaSeries, { ...areaOptions(series.tone), ...(hero ? { lastValueVisible: false, crosshairMarkerRadius: 5 } : {}) });
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
        live.current = { chart, api, points: series.kind === "candles" ? [] : series.points };
        chart.subscribeCrosshairMove((param) => {
          if (hero) scrubRef.current?.(scrubFor(param, api));
          else setReadout(readoutFor(param, api, valueFormat));
        });
        // The canvas draws with whatever face is ready; redraw once Mona Sans loads.
        void document.fonts?.load(`12px ${CHART_FONT}`, "0123456789").then(() => chart?.applyOptions({ layout: { fontFamily: CHART_FONT } }));
      } catch {
        chart?.remove();
        chart = null;
        setFailedFor(series);
      }
      return () => {
        chart?.remove();
        chart = null;
        live.current = null;
      };
    },
    [series, levels, markers, reducedMotion, compact, valueFormat, colourBlind, hero, axis],
  );

  /** Touch and pen: follow the finger along the line. A mouse already moves the crosshair. */
  const follow = (e: PointerEvent<HTMLDivElement>) => {
    const current = live.current;
    if (!hero || e.pointerType === "mouse" || !current) return;
    const x = e.clientX - e.currentTarget.getBoundingClientRect().left;
    const time = current.chart.timeScale().coordinateToTime(x);
    if (typeof time !== "number") return;
    const point = nearestPoint(current.points, time);
    if (!point) return;
    current.chart.setCrosshairPosition(point.value, point.time as UTCTimestamp, current.api);
    scrubRef.current?.(point);
  };
  const release = (e: PointerEvent<HTMLDivElement>) => {
    if (!hero || e.pointerType === "mouse") return;
    live.current?.chart.clearCrosshairPosition();
    scrubRef.current?.(null);
  };

  const failed = failedFor === series;
  const shown = readout ?? lastReadout(series, valueFormat);
  return (
    <div data-slot="time-chart" data-hero={hero ? "" : undefined} className={cn("grid gap-1.5", className)}>
      {hero ? null : (
        <p aria-hidden className="flex min-h-5 flex-wrap items-baseline gap-x-2 text-caption text-muted-foreground tabular">
          {shown ? (
            <>
              <span>{shown.time}</span>
              <span className="font-medium text-foreground">{shown.text}</span>
            </>
          ) : null}
        </p>
      )}
      <div
        ref={attach}
        role="img"
        aria-label={label}
        aria-describedby={summaryId}
        data-slot="chart-canvas"
        data-draw-in={hero && !reducedMotion ? "" : undefined}
        onPointerDown={follow}
        onPointerMove={follow}
        onPointerUp={release}
        onPointerCancel={release}
        onPointerLeave={release}
        className={cn("relative w-full bg-card", hero && "touch-pan-y select-none", hero && !reducedMotion && "draw-in")}
        style={{ height }}
      />
      <p id={summaryId} className="sr-only">
        {summary}
      </p>
      {failed ? (
        <p role="status" data-slot="chart-failed" className="rounded-lg bg-background px-3 py-2 text-sm">
          This chart could not be drawn in your browser. The figures on this page are current.
        </p>
      ) : null}
    </div>
  );
}
