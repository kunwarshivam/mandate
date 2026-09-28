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
import { colorTokens } from "@/lib/tokens";

/**
 * Placard for TradingView Lightweight Charts. Canvas cannot read CSS variables, so the tokens are
 * converted to hex once. Every fill is one flat colour: an area's top and bottom colours are the
 * same, the background is solid, and there is no animation.
 */
function token(name: string): string {
  const found = colorTokens.find((t) => t.name === name);
  if (!found) throw new Error(`no colour token ${name}`);
  return toHex(found.value);
}

export const CHART_COLOR = {
  card: token("card"),
  background: token("background"),
  foreground: token("foreground"),
  muted: token("muted"),
  mutedForeground: token("muted-foreground"),
  border: token("border"),
  lapis: token("lapis"),
  lapisSoft: token("lapis-soft"),
  lapisForeground: token("lapis-foreground"),
  marigold: token("marigold"),
  marigoldForeground: token("marigold-foreground"),
  gain: token("gain"),
  loss: token("loss"),
  ink: token("ink"),
  inkForeground: token("ink-foreground"),
} as const;

export const CHART_FONT = "'Atkinson Hyperlegible Next Variable', ui-sans-serif, system-ui, sans-serif";

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
}

export function baseOptions({ reducedMotion, compact = false, valueFormat = usdLabel }: BaseOptionsInput): DeepPartial<ChartOptions> {
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

export type Tone = "account" | "agent" | "neutral";

/** A flat area: the fill is one colour from the line down to the axis. */
export function areaOptions(tone: Tone): AreaSeriesPartialOptions {
  const line = tone === "account" ? CHART_COLOR.lapis : tone === "agent" ? CHART_COLOR.foreground : CHART_COLOR.mutedForeground;
  const fill = tone === "account" ? CHART_COLOR.lapisSoft : CHART_COLOR.muted;
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

export function candleOptions(): CandlestickSeriesPartialOptions {
  return {
    upColor: CHART_COLOR.gain,
    downColor: CHART_COLOR.loss,
    wickUpColor: CHART_COLOR.gain,
    wickDownColor: CHART_COLOR.loss,
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
 * Who a level belongs to decides its colour: marigold for the mandate (limits, protection), lapis
 * for the account (average cost), ink for a proposal the owner is asked about.
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

export function priceLineFor(level: ChartLevel): CreatePriceLineOptions {
  const color = level.tone === "mandate" ? CHART_COLOR.marigold : level.tone === "account" ? CHART_COLOR.lapis : CHART_COLOR.ink;
  const text = level.tone === "mandate" ? CHART_COLOR.marigoldForeground : level.tone === "account" ? CHART_COLOR.lapisForeground : CHART_COLOR.inkForeground;
  return {
    id: level.key,
    price: level.price,
    color,
    lineWidth: 2,
    lineStyle: level.tone === "proposal" ? LineStyle.Dashed : LineStyle.Solid,
    lineVisible: true,
    axisLabelVisible: true,
    axisLabelColor: color,
    axisLabelTextColor: text,
    title: level.label,
  };
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
