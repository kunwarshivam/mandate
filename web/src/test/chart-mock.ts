import type { CreatePriceLineOptions, MouseEventParams, SeriesMarker, Time } from "lightweight-charts";

/**
 * jsdom has no canvas, so tests swap Lightweight Charts' drawing for this recorder (vitest.setup.ts).
 * The library's enums and series definitions stay real; every chart, series, price line and marker
 * the app creates is kept here with the exact options it passed.
 */
export interface MockSeries {
  type: string;
  options: Record<string, unknown>;
  data: unknown[];
  priceLines: CreatePriceLineOptions[];
  markers: SeriesMarker<Time>[];
  /** What `addSeries` returned; crosshair events key their data by it. */
  api: unknown;
}

export interface MockChart {
  el: HTMLElement;
  options: Record<string, unknown>;
  series: MockSeries[];
  removed: boolean;
  fitted: boolean;
  crosshair: Array<(param: MouseEventParams<Time>) => void>;
  /** Where the app last pinned the crosshair (touch scrubbing), or `null` once it cleared it. */
  pinned: { price: number; time: Time } | null;
}

/** Stands in for the time scale's pixel mapping: tests set the time under a pointer's x. */
export const pointerTime: { at: (x: number) => number | null } = { at: () => null };

export const charts: MockChart[] = [];

/** Set `failNext` to make the next chart throw on creation, as a browser without canvas would. */
export const chartControl = { failNext: false };

export function resetCharts() {
  charts.length = 0;
  chartControl.failNext = false;
  pointerTime.at = () => null;
}

/** The chart drawn into a canvas container, the latest one if it was redrawn. */
export function chartIn(el: Element): MockChart | undefined {
  return charts.filter((c) => c.el === el && !c.removed).at(-1);
}

export function liveCharts(): MockChart[] {
  return charts.filter((c) => !c.removed);
}

function seriesApi(record: MockSeries) {
  return {
    setData(data: unknown[]) {
      record.data = data;
    },
    applyOptions(options: Record<string, unknown>) {
      Object.assign(record.options, options);
    },
    createPriceLine(options: CreatePriceLineOptions) {
      record.priceLines.push(options);
      return { options: () => options, applyOptions: (next: Partial<CreatePriceLineOptions>) => Object.assign(options, next) };
    },
    __record: record,
  };
}

export const mockChartModule = {
  createChart(el: HTMLElement, options: Record<string, unknown> = {}) {
    if (chartControl.failNext) {
      chartControl.failNext = false;
      throw new Error("canvas is not available");
    }
    const chart: MockChart = { el, options, series: [], removed: false, fitted: false, crosshair: [], pinned: null };
    charts.push(chart);
    return {
      addSeries(definition: { type: string }, seriesOptions: Record<string, unknown> = {}) {
        const record: MockSeries = { type: definition.type, options: { ...seriesOptions }, data: [], priceLines: [], markers: [], api: null };
        record.api = seriesApi(record);
        chart.series.push(record);
        return record.api;
      },
      applyOptions(next: Record<string, unknown>) {
        Object.assign(chart.options, next);
      },
      timeScale() {
        return {
          fitContent() {
            chart.fitted = true;
          },
          coordinateToTime(x: number) {
            return pointerTime.at(x);
          },
        };
      },
      subscribeCrosshairMove(handler: (param: MouseEventParams<Time>) => void) {
        chart.crosshair.push(handler);
      },
      setCrosshairPosition(price: number, time: Time) {
        chart.pinned = { price, time };
      },
      clearCrosshairPosition() {
        chart.pinned = null;
      },
      remove() {
        chart.removed = true;
      },
    };
  },
  createSeriesMarkers(series: { __record: MockSeries }, markers: SeriesMarker<Time>[]) {
    series.__record.markers = markers;
    return {
      setMarkers(next: SeriesMarker<Time>[]) {
        series.__record.markers = next;
      },
      markers: () => series.__record.markers,
      detach() {
        series.__record.markers = [];
      },
    };
  },
};
