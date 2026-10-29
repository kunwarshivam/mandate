import { act, fireEvent, screen, within } from "@testing-library/react";
import { ColorType, LineStyle, LineType } from "lightweight-charts";
import { afterEach, describe, expect, it } from "vitest";
import { AGENT_IDS, APPROVAL_IDS, buildWorkspace } from "@/fixtures/workspace";
import { parseOklch, toHex } from "@/lib/color";
import { PALETTE, PALETTE_DARK } from "@/lib/palette";
import { type MockChart, chartIn, liveCharts } from "@/test/chart-mock";
import { renderWithRuntime } from "@/test/harness";
import { CHART_COLOR, CHART_TOKEN, type ChartLevel, HERO_FILL, type Tone, areaOptions, baseOptions, candleOptions, heroAreaOptions, lineOptions, priceLineFor, setChartMode, trendColor } from "./options";
import { ApprovalChart, PositionChart } from "./price-chart";

const WS = buildWorkspace("normal");
const agentOf = (id: string) => WS.agents.find((a) => a.agent_id === id)!;
const SWING = agentOf(AGENT_IDS.swing);
const BTC = agentOf(AGENT_IDS.btc);
const XYZ = SWING.positions.find((p) => p.instrument.symbol === "XYZ")!;
const TONES: Tone[] = ["account", "agent", "neutral"];

afterEach(() => {
  delete document.documentElement.dataset.cvd;
});

/** Every `type` field in an options tree, however deep. */
function typesIn(value: unknown): unknown[] {
  if (value === null || typeof value !== "object") return [];
  return Object.entries(value).flatMap(([k, v]) => (k === "type" ? [v, ...typesIn(v)] : typesIn(v)));
}

function onlyChart(container: HTMLElement): MockChart {
  const canvases = container.querySelectorAll("[data-slot=chart-canvas]");
  expect(canvases).toHaveLength(1);
  const chart = chartIn(canvases[0]);
  expect(chart).toBeDefined();
  return chart!;
}

describe("chart builders draw flat, solid colour", () => {
  it.each([
    [false, false],
    [true, false],
    [false, true],
  ])("reducedMotion=%s compact=%s: solid background, no logo, no kinetic touch scroll when it should not move", (reducedMotion, compact) => {
    const options = baseOptions({ reducedMotion, compact });
    expect(options.layout?.background).toEqual({ type: ColorType.Solid, color: CHART_COLOR.card });
    expect(options.layout?.attributionLogo).toBe(false);
    expect(options.kineticScroll?.touch).toBe(!reducedMotion && !compact);
    expect(options.kineticScroll?.mouse).toBe(false);
    for (const t of typesIn(options)) expect(t).toBe(ColorType.Solid);
  });

  it.each(TONES)("the %s area is one colour from the line to the axis", (tone) => {
    const options = areaOptions(tone);
    expect(options.topColor).toBe(options.bottomColor);
    expect(typesIn(options)).toEqual([]);
  });

  it("no builder sets a fill that varies", () => {
    for (const options of [...TONES.map(areaOptions), ...TONES.map(lineOptions), candleOptions(), candleOptions(true)]) {
      for (const t of typesIn(options)) expect(t).toBe(ColorType.Solid);
      if ("topColor" in options) expect(options.topColor).toBe(options.bottomColor);
    }
  });

  it("draws a hero line in the colour of its change, smooth, with nothing under it", () => {
    for (const [trend, plain, cvd] of [
      ["gain", CHART_COLOR.gain, CHART_COLOR.gainCvd],
      ["loss", CHART_COLOR.loss, CHART_COLOR.lossCvd],
      ["flat", CHART_COLOR.foreground, CHART_COLOR.foreground],
    ] as const) {
      expect(heroAreaOptions(trend)).toMatchObject({ lineColor: plain, lineWidth: 3, lineType: LineType.Curved, topColor: HERO_FILL, bottomColor: HERO_FILL, lastValueVisible: false });
      expect(heroAreaOptions(trend, true).lineColor).toBe(cvd);
      expect(trendColor(trend)).toBe(plain);
    }
    expect(baseOptions({ reducedMotion: false, hero: { axis: false } }).timeScale?.visible).toBe(false);
  });

  it("colours are plain hex from the tokens, gains and losses signed", () => {
    for (const hex of Object.values(CHART_COLOR)) expect(hex).toMatch(/^#[0-9a-f]{6}$/i);
    expect(candleOptions()).toMatchObject({ upColor: CHART_COLOR.gain, downColor: CHART_COLOR.loss });
    expect(CHART_COLOR.gain).not.toBe(CHART_COLOR.loss);
    expect(areaOptions("account").lineColor).toBe(CHART_COLOR.lapis);
  });

  it("every chart colour is its palette token, converted to hex", () => {
    for (const [key, name] of Object.entries(CHART_TOKEN)) {
      expect(CHART_COLOR[key as keyof typeof CHART_TOKEN], `${key} is ${name}`).toBe(toHex(PALETTE.tokens[name].value));
    }
  });

  it("draws the account's levels in azure, the mandate's marks in azure, and the grid in a cool paper tint", () => {
    expect(CHART_TOKEN.lapis).toBe("lapis-line");
    expect(PALETTE.tokens["lapis-line"].ref).toBe("azure-600");
    expect(PALETTE.tokens["mandate-marker"].ref).toBe("azure-600");
    expect(PALETTE.tokens["mandate-strong"].ref).toBe("azure-800");
    const grid = baseOptions({ reducedMotion: false }).grid;
    expect(grid?.horzLines?.color).toBe(CHART_COLOR.muted);
    expect(grid?.vertLines?.color).toBe(CHART_COLOR.muted);
    const paper = parseOklch(PALETTE.tokens.muted.value);
    expect(paper.h).toBe(255);
    expect(paper.c).toBeGreaterThan(0);
  });

  it("a mandate level is a dashed grey line with an azure label, the account a solid azure line, a proposal dashed ink", () => {
    const level = (tone: ChartLevel["tone"]): ChartLevel => ({ key: tone, label: tone, price: 1, tone });
    expect(priceLineFor(level("mandate"))).toMatchObject({ color: CHART_COLOR.mutedForeground, axisLabelColor: CHART_COLOR.mandate, axisLabelTextColor: CHART_COLOR.mandateStrong, lineStyle: LineStyle.Dashed });
    expect(priceLineFor(level("account"))).toMatchObject({ color: CHART_COLOR.lapis, axisLabelColor: CHART_COLOR.ink, axisLabelTextColor: CHART_COLOR.inkForeground, lineStyle: LineStyle.Solid });
    expect(priceLineFor(level("proposal"))).toMatchObject({ color: CHART_COLOR.ink, axisLabelColor: CHART_COLOR.ink, axisLabelTextColor: CHART_COLOR.inkForeground, lineStyle: LineStyle.Dashed });
  });

  it("reads the dark palette after setChartMode, and back", () => {
    try {
      setChartMode("dark");
      for (const [key, name] of Object.entries(CHART_TOKEN)) {
        expect(CHART_COLOR[key as keyof typeof CHART_TOKEN], `${key} is ${name}`).toBe(toHex(PALETTE_DARK.tokens[name].value));
      }
      expect(baseOptions({ reducedMotion: false }).layout?.background).toEqual({ type: ColorType.Solid, color: toHex(PALETTE_DARK.tokens.card.value) });
    } finally {
      setChartMode("light");
    }
    expect(CHART_COLOR.card).toBe(toHex(PALETTE.tokens.card.value));
  });

  it("candles turn to the colour-blind alternates when colour-blind friendly is on", () => {
    expect(candleOptions(true)).toMatchObject({ upColor: CHART_COLOR.gainCvd, downColor: CHART_COLOR.lossCvd, wickUpColor: CHART_COLOR.gainCvd, wickDownColor: CHART_COLOR.lossCvd });
    expect(CHART_COLOR.gainCvd).not.toBe(CHART_COLOR.gain);
    expect(CHART_COLOR.lossCvd).not.toBe(CHART_COLOR.loss);
  });
});

describe("PositionChart", () => {
  it("draws candles with the average cost as the account's azure line and the bracket as the mandate's dashed lines, at the position's prices", () => {
    const { container } = renderWithRuntime(<PositionChart agent={SWING} position={XYZ} />);
    const [series] = onlyChart(container).series;
    expect(series.type).toBe("Candlestick");
    const byId = new Map(series.priceLines.map((l) => [l.id, l]));
    const expected = {
      "avg-cost": { price: Number(XYZ.avg_cost), color: CHART_COLOR.lapis, title: "Average cost" },
      stop: { price: Number(XYZ.protection.stop_price), color: CHART_COLOR.mutedForeground, title: "Stop" },
      "take-profit": { price: Number(XYZ.protection.take_profit_price), color: CHART_COLOR.mutedForeground, title: "Take-profit" },
    };
    const legend = [...container.querySelectorAll("[data-slot=level-legend] [data-level]")].map((li) => li.getAttribute("data-level"));
    expect(legend.sort()).toEqual(Object.keys(expected).sort());
    for (const [key, want] of Object.entries(expected)) {
      const line = byId.get(key);
      const drawn = container.querySelector(`[data-level=${key}]`)?.getAttribute("data-drawn") === "true";
      if (drawn) expect(line).toMatchObject(want);
      else expect(line).toBeUndefined();
    }
    expect(byId.size).toBeGreaterThan(0);
  });

  it("marks this agent's fills on the 5D candles", () => {
    const { container } = renderWithRuntime(<PositionChart agent={SWING} position={XYZ} />);
    fireEvent.click(within(screen.getByRole("group", { name: "Price range" })).getByRole("button", { name: "5D" }));
    const [series] = onlyChart(container).series;
    const bars = new Set(series.data.map((b) => (b as { time: unknown }).time));
    expect(series.markers.length).toBeGreaterThan(0);
    for (const m of series.markers) {
      expect(bars.has(m.time)).toBe(true);
      expect(m.text).toMatch(/^(Buy|Sell) /);
      expect(m.color).toBe(CHART_COLOR.ink);
    }
  });

  it("redraws its candles in the colour-blind friendly pair when <html data-cvd> turns on", async () => {
    const { container } = renderWithRuntime(<PositionChart agent={SWING} position={XYZ} />);
    const before = onlyChart(container);
    expect(before.series[0].options).toMatchObject({ upColor: CHART_COLOR.gain, downColor: CHART_COLOR.loss });
    await act(async () => {
      document.documentElement.dataset.cvd = "on";
      await Promise.resolve();
    });
    expect(before.removed).toBe(true);
    expect(onlyChart(container).series[0].options).toMatchObject({ upColor: CHART_COLOR.gainCvd, downColor: CHART_COLOR.lossCvd });
    expect(liveCharts()).toHaveLength(1);
  });

  it("tells a crypto holder there is no session to wait for", () => {
    renderWithRuntime(<PositionChart agent={BTC} position={BTC.positions[0]} />);
    expect(screen.getByText(/no session to wait for/)).toBeInTheDocument();
    expect(screen.queryByText(/regular session is 09:30/)).toBeNull();
  });

  it("names the stop-limit and its floor for crypto", () => {
    const { container } = renderWithRuntime(<PositionChart agent={BTC} position={BTC.positions[0]} />);
    const stop = container.querySelector("[data-level=stop]");
    expect(stop).toHaveTextContent("Stop-limit");
    expect(stop).toHaveTextContent("$50,855.55");
  });
});

describe("ApprovalChart", () => {
  it("is a small neutral line with one dashed proposed limit and no axes", () => {
    const approval = WS.approvals.find((a) => a.approval_id === APPROVAL_IDS.swingXyz)!;
    const { container } = renderWithRuntime(<ApprovalChart approval={approval} />);
    const chart = onlyChart(container);
    const [series] = chart.series;
    expect(series.type).toBe("Line");
    expect(series.options.color).toBe(CHART_COLOR.mutedForeground);
    expect(series.priceLines).toHaveLength(1);
    expect(series.priceLines[0]).toMatchObject({ title: "Proposed limit", price: Number(approval.bound.limit), lineStyle: LineStyle.Dashed, color: CHART_COLOR.ink });
    expect(chart.options).toMatchObject({ timeScale: { visible: false }, rightPriceScale: { visible: false }, handleScroll: false, handleScale: false });
    const requested = Date.parse(approval.requested_at) / 1000;
    for (const p of series.data) expect((p as { time: number }).time).toBeLessThanOrEqual(requested);
  });
});
