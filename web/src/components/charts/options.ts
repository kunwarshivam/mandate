import {
  type AreaSeriesPartialOptions,
  type AutoscaleInfoProvider,
  type CandlestickData,
  type CandlestickSeriesPartialOptions,
  type ChartOptions,
  ColorType,
  CrosshairMode,
  type CreatePriceLineOptions,
  type DeepPartial,
  LastPriceAnimationMode,
  type LineSeriesPartialOptions,
  LineStyle,
  type SeriesMarker,
  type Time,
  TickMarkType,
  type UTCTimestamp,
} from "lightweight-charts";
import type { Bar, DailyBar, Point } from "@/fixtures/market";
import { toHex } from "@/lib/color";
import { PALETTE, PALETTES, type Palette, type TokenName } from "@/lib/palette";
import type { ThemeMode } from "@/lib/theme";

/**
 * TradingView Lightweight Charts in the calm system. Canvas cannot read CSS variables, so the tokens
 * are converted to hex once. Every fill is one flat colour: an area's top and bottom colours are the
 * same and the background is solid. The library animates nothing; the one motion, the line drawing
 * in on first load, is a CSS clip on the canvas's container (`draw-in`).
 */
export const CHART_TOKEN = {
  card: "card",
  background: "background",
  foreground: "foreground",
  muted: "muted",
  mutedForeground: "muted-foreground",
  border: "border",
  lapis: "lapis-line",
  lapisSoft: "lapis-soft",
  lapisForeground: "lapis-foreground",
  mandate: "mandate",
  mandateMarker: "mandate-marker",
  mandateStrong: "mandate-strong",
  gain: "gain",
  loss: "loss",
  gainCvd: "gain-cvd",
  lossCvd: "loss-cvd",
  ink: "ink",
  inkForeground: "ink-foreground",
} as const satisfies Record<string, TokenName>;

type ChartColors = Record<keyof typeof CHART_TOKEN, string>;

function chartColors(palette: Palette): ChartColors {
  return Object.fromEntries(Object.entries(CHART_TOKEN).map(([key, name]) => [key, toHex(palette.tokens[name].value)])) as ChartColors;
}

export const CHART_COLORS: Record<ThemeMode, ChartColors> = { light: chartColors(PALETTE), dark: chartColors(PALETTES.dark) };

/** The colours every option builder reads; a chart calls `setChartMode` before it draws. */
export const CHART_COLOR: ChartColors = { ...CHART_COLORS.light };

export function setChartMode(mode: ThemeMode): void {
  Object.assign(CHART_COLOR, CHART_COLORS[mode]);
}

export const CHART_FONT = "'Public Sans Variable', ui-sans-serif, system-ui, sans-serif";

const ET_TIME = new Intl.DateTimeFormat("en-US", { timeZone: "America/New_York", hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
const ET_DAY = new Intl.DateTimeFormat("en-US", { timeZone: "America/New_York", month: "short", day: "numeric" });
const ET_MONTH = new Intl.DateTimeFormat("en-US", { timeZone: "UTC", month: "short" });
const ET_YEAR = new Intl.DateTimeFormat("en-US", { timeZone: "UTC", year: "numeric" });
const DAY_LABEL = new Intl.DateTimeFormat("en-US", { timeZone: "UTC", month: "short", day: "numeric", year: "numeric" });

function dayDate(time: Time): Date | null {
  if (typeof time === "string") return new Date(`${time}T12:00:00Z`);
  if (typeof time === "object") return new Date(Date.UTC(time.year, time.month - 1, time.day, 12));
  return null;
}

/** "Sep 28, 14:03 ET" for a minute, "Sep 28, 2026" for a day. */
export function formatTime(time: Time): string {
  const day = dayDate(time);
  if (day) return DAY_LABEL.format(day);
  const d = new Date((time as number) * 1000);
  return `${ET_DAY.format(d)}, ${ET_TIME.format(d)} ET`;
}

function tickMark(time: Time, type: TickMarkType): string {
  const day = dayDate(time);
  const d = day ?? new Date((time as number) * 1000);
  switch (type) {
    case TickMarkType.Year:
      return ET_YEAR.format(d);
    case TickMarkType.Month:
      return ET_MONTH.format(d);
    case TickMarkType.DayOfMonth:
      return day ? DAY_LABEL.format(d).replace(/, \d{4}$/, "") : ET_DAY.format(d);
    case TickMarkType.Time:
    case TickMarkType.TimeWithSeconds:
      return day ? DAY_LABEL.format(d) : ET_TIME.format(d);
    default: {
      const unhandled: never = type;
      throw new Error(`unhandled tick mark ${String(unhandled)}`);
    }
  }
}

export function usdLabel(value: number): string {
  const sign = value < 0 ? "\u2212" : "";
  return `${sign}$${Math.abs(value).toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
}

export interface BaseOptionsInput {
  reducedMotion: boolean;
  /** A small chart with no axes and no interaction (the approval screen on a phone). */
  compact?: boolean;
  valueFormat?: (value: number) => string;
  /**
   * A hero chart that the owner scrubs: no grid, no pan or zoom (the range tabs choose the window),
   * a hairline crosshair with no labels because the hero figure above reads it out. `axis` keeps
   * the price scale for the mandate levels' labels.
   */
  hero?: { axis: boolean };
}

export function baseOptions({ reducedMotion, compact = false, valueFormat = usdLabel, hero }: BaseOptionsInput): DeepPartial<ChartOptions> {
  if (hero) return heroOptions(valueFormat, hero.axis);
  return {
    autoSize: true,
    layout: {
      background: { type: ColorType.Solid, color: CHART_COLOR.card },
      textColor: CHART_COLOR.mutedForeground,
      fontFamily: CHART_FONT,
      fontSize: 12,
      attributionLogo: false,
    },
    grid: {
      vertLines: { color: CHART_COLOR.muted, style: LineStyle.Solid, visible: !compact },
      horzLines: { color: CHART_COLOR.muted, style: LineStyle.Solid, visible: true },
    },
    rightPriceScale: { borderColor: CHART_COLOR.border, visible: !compact, scaleMargins: { top: 0.12, bottom: 0.12 } },
    leftPriceScale: { visible: false },
    timeScale: {
      borderColor: CHART_COLOR.border,
      visible: !compact,
      timeVisible: true,
      secondsVisible: false,
      tickMarkFormatter: tickMark,
      fixLeftEdge: true,
      fixRightEdge: true,
      lockVisibleTimeRangeOnResize: true,
    },
    crosshair: {
      mode: CrosshairMode.Magnet,
      vertLine: { color: CHART_COLOR.foreground, width: 1, style: LineStyle.Solid, labelBackgroundColor: CHART_COLOR.ink },
      horzLine: { color: CHART_COLOR.foreground, width: 1, style: LineStyle.Solid, labelBackgroundColor: CHART_COLOR.ink },
    },
    localization: { locale: "en-US", timeFormatter: formatTime, priceFormatter: valueFormat },
    handleScroll: !compact,
    handleScale: !compact,
    kineticScroll: { mouse: false, touch: !reducedMotion && !compact },
  };
}

function heroOptions(valueFormat: (value: number) => string, axis: boolean): DeepPartial<ChartOptions> {
  return {
    autoSize: true,
    layout: {
      background: { type: ColorType.Solid, color: CHART_COLOR.card },
      textColor: CHART_COLOR.mutedForeground,
      fontFamily: CHART_FONT,
      fontSize: 12,
      attributionLogo: false,
    },
    grid: {
      vertLines: { color: CHART_COLOR.muted, style: LineStyle.Solid, visible: false },
      horzLines: { color: CHART_COLOR.muted, style: LineStyle.Solid, visible: false },
    },
    rightPriceScale: { visible: axis, borderVisible: false, scaleMargins: { top: 0.16, bottom: 0.12 } },
    leftPriceScale: { visible: false },
    timeScale: {
      visible: true,
      borderVisible: false,
      timeVisible: true,
      secondsVisible: false,
      tickMarkFormatter: tickMark,
      fixLeftEdge: true,
      fixRightEdge: true,
      lockVisibleTimeRangeOnResize: true,
    },
    crosshair: {
      mode: CrosshairMode.Magnet,
      vertLine: { color: CHART_COLOR.mutedForeground, width: 1, style: LineStyle.Solid, labelVisible: false },
      horzLine: { visible: false, labelVisible: false },
    },
    localization: { locale: "en-US", timeFormatter: formatTime, priceFormatter: valueFormat },
    handleScroll: false,
    handleScale: false,
    kineticScroll: { mouse: false, touch: false },
  };
}

export type Tone = "account" | "agent" | "neutral";

/** A flat area: the fill is one colour from the line down to the axis. */
export function areaOptions(tone: Tone): AreaSeriesPartialOptions {
  const line = tone === "account" ? CHART_COLOR.lapis : tone === "agent" ? CHART_COLOR.foreground : CHART_COLOR.mutedForeground;
  const fill = tone === "account" ? CHART_COLOR.lapisSoft : tone === "agent" ? CHART_COLOR.card : CHART_COLOR.muted;
  return {
    lineColor: line,
    lineWidth: 2,
    topColor: fill,
    bottomColor: fill,
    priceLineVisible: false,
    lastValueVisible: true,
    lastPriceAnimation: LastPriceAnimationMode.Disabled,
    crosshairMarkerBorderColor: CHART_COLOR.card,
    crosshairMarkerBackgroundColor: line,
  };
}

export function lineOptions(tone: Tone): LineSeriesPartialOptions {
  return {
    color: tone === "account" ? CHART_COLOR.lapis : tone === "agent" ? CHART_COLOR.foreground : CHART_COLOR.mutedForeground,
    lineWidth: 2,
    priceLineVisible: false,
    lastValueVisible: true,
    lastPriceAnimation: LastPriceAnimationMode.Disabled,
  };
}

/** Gain and loss candles; with colour-blind friendly on, blue for gains and raspberry (light) or orange (dark) for losses. */
export function candleOptions(colourBlind = false): CandlestickSeriesPartialOptions {
  const up = colourBlind ? CHART_COLOR.gainCvd : CHART_COLOR.gain;
  const down = colourBlind ? CHART_COLOR.lossCvd : CHART_COLOR.loss;
  return {
    upColor: up,
    downColor: down,
    wickUpColor: up,
    wickDownColor: down,
    borderVisible: false,
    priceLineVisible: true,
    priceLineColor: CHART_COLOR.foreground,
    priceLineStyle: LineStyle.Solid,
    lastValueVisible: true,
  };
}

export function areaData(points: Point[]): Array<{ time: UTCTimestamp; value: number }> {
  return points.map((p) => ({ time: p.time as UTCTimestamp, value: p.value }));
}

/** Extended-hours candles are drawn in the border colour so the regular session reads first. */
export function candleData(bars: Array<Bar | DailyBar>): CandlestickData<Time>[] {
  return bars.map((b) => {
    const time = "day" in b ? (b.day as Time) : (b.time as UTCTimestamp);
    const extended = "session" in b && (b.session === "pre" || b.session === "post");
    const base = { time, open: b.open, high: b.high, low: b.low, close: b.close };
    return extended ? { ...base, color: CHART_COLOR.border, wickColor: CHART_COLOR.border } : base;
  });
}

/**
 * Who a level belongs to decides its look: a dashed grey line with a gold label for the mandate
 * (limits, protection), a solid gold line for the account (average cost), a dashed ink line for a
 * proposal the owner is asked about. The mandate's line stays grey so it never reads as the account's.
 */
export type LevelTone = "mandate" | "account" | "proposal";

export interface ChartLevel {
  key: string;
  label: string;
  price: number;
  tone: LevelTone;
  /** What happens at this level, in words, for the legend. */
  meaning?: string;
}

/** The line and its axis label. A mandate level's label is a pale gold tag in dark gold type. */
function levelColours(tone: LevelTone): { line: string; label: string; text: string } {
  switch (tone) {
    case "mandate":
      return { line: CHART_COLOR.mutedForeground, label: CHART_COLOR.mandate, text: CHART_COLOR.mandateStrong };
    case "account":
      return { line: CHART_COLOR.lapis, label: CHART_COLOR.ink, text: CHART_COLOR.inkForeground };
    case "proposal":
      return { line: CHART_COLOR.ink, label: CHART_COLOR.ink, text: CHART_COLOR.inkForeground };
    default: {
      const unhandled: never = tone;
      throw new Error(`unhandled level tone ${String(unhandled)}`);
    }
  }
}

export function priceLineFor(level: ChartLevel, labelled = true): CreatePriceLineOptions {
  const { line, label, text } = levelColours(level.tone);
  return {
    id: level.key,
    price: level.price,
    color: line,
    lineWidth: 1,
    lineStyle: level.tone === "account" ? LineStyle.Solid : LineStyle.Dashed,
    lineVisible: true,
    axisLabelVisible: labelled,
    axisLabelColor: label,
    axisLabelTextColor: text,
    title: labelled ? level.label : "",
  };
}

/** A label is about 18 px tall on a plot of about 190 px, so labels closer than this share of the span collide. */
export const LABEL_GAP = 0.09;

/**
 * The levels whose labels would sit on top of a label above them, from the top of the scale down.
 * Their lines still draw; the legend under the chart names every level.
 */
export function crowdedLevels(levels: ChartLevel[], values: number[]): Set<string> {
  const prices = [...values, ...levels.map((l) => l.price)];
  const span = prices.length > 0 ? Math.max(...prices) - Math.min(...prices) : 0;
  const hidden = new Set<string>();
  let last: number | null = null;
  for (const level of [...levels].sort((a, b) => b.price - a.price)) {
    if (last !== null && span > 0 && last - level.price < span * LABEL_GAP) hidden.add(level.key);
    else last = level.price;
  }
  return hidden;
}

export interface ChartMarker {
  time: number | string;
  side: "buy" | "sell";
  text: string;
}

export function markersFor(markers: ChartMarker[]): SeriesMarker<Time>[] {
  return [...markers]
    .sort((a, b) => (a.time < b.time ? -1 : a.time > b.time ? 1 : 0))
    .map((m) => ({
      time: m.time as Time,
      position: m.side === "buy" ? "belowBar" : "aboveBar",
      shape: m.side === "buy" ? "arrowUp" : "arrowDown",
      color: CHART_COLOR.ink,
      text: m.text,
    }));
}

/** Widens the price scale to take in the drawn levels, so a level is never off the top or bottom. */
export function autoscaleWith(levels: ChartLevel[]): AutoscaleInfoProvider {
  return (original) => {
    const info = original();
    if (!info?.priceRange || levels.length === 0) return info;
    const prices = levels.map((l) => l.price);
    return {
      ...info,
      priceRange: { minValue: Math.min(info.priceRange.minValue, ...prices), maxValue: Math.max(info.priceRange.maxValue, ...prices) },
    };
  };
}
