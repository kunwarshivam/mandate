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
}

export interface MockChart {
  el: HTMLElement;
  options: Record<string, unknown>;
  series: MockSeries[];
  removed: boolean;
  fitted: boolean;
  crosshair: Array<(param: MouseEventParams<Time>) => void>;
}

export const charts: MockChart[] = [];

export function resetCharts() {
  charts.length = 0;
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
    const chart: MockChart = { el, options, series: [], removed: false, fitted: false, crosshair: [] };
    charts.push(chart);
    return {
      addSeries(definition: { type: string }, seriesOptions: Record<string, unknown> = {}) {
        const record: MockSeries = { type: definition.type, options: { ...seriesOptions }, data: [], priceLines: [], markers: [] };
        chart.series.push(record);
        return seriesApi(record);
      },
      applyOptions(next: Record<string, unknown>) {
        Object.assign(chart.options, next);
      },
      timeScale() {
        return {
          fitContent() {
            chart.fitted = true;
          },
        };
      },
      subscribeCrosshairMove(handler: (param: MouseEventParams<Time>) => void) {
        chart.crosshair.push(handler);
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
