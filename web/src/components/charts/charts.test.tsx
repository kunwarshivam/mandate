import { act, fireEvent, screen, within } from "@testing-library/react";
import { ColorType, LineStyle } from "lightweight-charts";
import { afterEach, describe, expect, it } from "vitest";
import { AGENT_IDS, APPROVAL_IDS, SCENARIOS, buildWorkspace } from "@/fixtures/workspace";
import { parseOklch, toHex } from "@/lib/color";
import { toFixed } from "@/lib/decimal";
import { usd } from "@/lib/format";
import { agentLimits } from "@/lib/limits";
import { PALETTE } from "@/lib/palette";
import { type MockChart, chartControl, chartIn, liveCharts } from "@/test/chart-mock";
import { renderWithRuntime } from "@/test/harness";
import { AccountEquityChart, AgentEquityChart, unmanagedEquity } from "./equity-chart";
import { CHART_COLOR, CHART_TOKEN, type ChartLevel, type Tone, areaOptions, baseOptions, candleOptions, lineOptions, priceLineFor } from "./options";
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

  it("draws the account in navy, the mandate in brass, and the grid in tinted slate", () => {
    expect(PALETTE.tokens.lapis.ref).toBe("navy-800");
    expect(PALETTE.tokens["mandate-marker"].ref).toBe("brass-500");
    expect(PALETTE.tokens["mandate-strong"].ref).toBe("brass-700");
    const grid = baseOptions({ reducedMotion: false }).grid;
    expect(grid?.horzLines?.color).toBe(CHART_COLOR.muted);
    expect(grid?.vertLines?.color).toBe(CHART_COLOR.muted);
    const slate = parseOklch(PALETTE.tokens.muted.value);
    expect(slate.h).toBe(255);
    expect(slate.c).toBeGreaterThan(0);
  });

  it("a mandate level is a brass line with a dark brass label, the account navy, a proposal ink", () => {
    const level = (tone: ChartLevel["tone"]): ChartLevel => ({ key: tone, label: tone, price: 1, tone });
    expect(priceLineFor(level("mandate"))).toMatchObject({ color: CHART_COLOR.mandateMarker, axisLabelColor: CHART_COLOR.mandateStrong, axisLabelTextColor: CHART_COLOR.card });
    expect(priceLineFor(level("account"))).toMatchObject({ color: CHART_COLOR.lapis, axisLabelColor: CHART_COLOR.lapis, axisLabelTextColor: CHART_COLOR.lapisForeground });
    expect(priceLineFor(level("proposal"))).toMatchObject({ color: CHART_COLOR.ink, axisLabelColor: CHART_COLOR.ink, axisLabelTextColor: CHART_COLOR.inkForeground, lineStyle: LineStyle.Dashed });
  });

  it("candles turn blue and orange when colour-blind friendly is on", () => {
    expect(candleOptions(true)).toMatchObject({ upColor: CHART_COLOR.gainCvd, downColor: CHART_COLOR.lossCvd, wickUpColor: CHART_COLOR.gainCvd, wickDownColor: CHART_COLOR.lossCvd });
    expect(CHART_COLOR.gainCvd).not.toBe(CHART_COLOR.gain);
    expect(CHART_COLOR.lossCvd).not.toBe(CHART_COLOR.loss);
  });
});

describe("AgentEquityChart", () => {
  it.each(WS.agents.map((a) => [a.label, a] as const))("%s: every price line is a mandate level at the mandate's value, labelled", (_label, agent) => {
    const { container } = renderWithRuntime(<AgentEquityChart agent={agent} />);
    const chart = onlyChart(container);
    const [series] = chart.series;
    expect(series.type).toBe("Area");
    expect(series.options.topColor).toBe(series.options.bottomColor);

    const levels = agentLimits(agent).levels;
    for (const line of series.priceLines) {
      const level = levels.find((l) => l.key === line.id);
      expect(level, String(line.id)).toBeDefined();
      expect(line.price).toBe(Number(toFixed(level!.at, 2)));
      expect(line.title).toBe(level!.label);
      expect(line.color).toBe(CHART_COLOR.mandateMarker);
      expect(line.lineStyle).toBe(LineStyle.Solid);
      expect(line.axisLabelVisible).toBe(true);
    }

    const legend = container.querySelector("[data-slot=level-legend]");
    const listed = [...(legend?.querySelectorAll("[data-level]") ?? [])];
    expect(listed.map((li) => li.getAttribute("data-level")).sort()).toEqual(levels.map((l) => l.key).sort());
    const drawnKeys = listed.filter((li) => li.getAttribute("data-drawn") === "true").map((li) => li.getAttribute("data-level"));
    expect(drawnKeys.sort()).toEqual(series.priceLines.map((l) => String(l.id)).sort());
  });

  it("draws at least one mandate level near an agent's equity", () => {
    const drawn = WS.agents.map((agent) => {
      const view = renderWithRuntime(<AgentEquityChart agent={agent} />);
      const count = onlyChart(view.container).series[0].priceLines.length;
      view.unmount();
      return count;
    });
    expect(Math.max(...drawn)).toBeGreaterThan(0);
  });

  it("redraws for a new range and removes the chart it replaces", () => {
    const { container } = renderWithRuntime(<AgentEquityChart agent={SWING} />);
    const first = onlyChart(container);
    const day = first.series[0].data.length;
    fireEvent.click(within(screen.getByRole("group", { name: "Equity range" })).getByRole("button", { name: "1W" }));
    expect(first.removed).toBe(true);
    const week = onlyChart(container);
    expect(week).not.toBe(first);
    expect(week.series[0].data.length).not.toBe(day);
    expect(liveCharts()).toHaveLength(1);
  });

  it("says so when the canvas cannot be drawn, and keeps the levels in words", () => {
    chartControl.failNext = true;
    const { container } = renderWithRuntime(<AgentEquityChart agent={SWING} />);
    expect(screen.getByRole("status")).toHaveTextContent("could not be drawn");
    expect(container.querySelector("[data-slot=chart-failed]")).not.toBeNull();
    expect(container.querySelector("[data-slot=level-legend]")).not.toBeNull();
    expect(liveCharts()).toHaveLength(0);
  });
});

describe("AccountEquityChart", () => {
  it("ends at the broker's equity, in navy, with the TradingView credit", () => {
    const { container } = renderWithRuntime(<AccountEquityChart />);
    const [series] = onlyChart(container).series;
    expect(series.options.lineColor).toBe(CHART_COLOR.lapis);
    expect(series.options.topColor).toBe(series.options.bottomColor);
    const last = series.data.at(-1) as { value: number };
    expect(last.value).toBe(Number(WS.connection.account_equity));
    expect(container.querySelector("[data-slot=account-equity-value]")).toHaveTextContent(usd(WS.connection.account_equity));
    expect(container.querySelector("[data-slot=unmanaged]")).toHaveTextContent(usd(unmanagedEquity(WS)));
    expect(screen.getByRole("link", { name: /TradingView Lightweight Charts/ })).toHaveAttribute("href", "https://www.tradingview.com/");
    expect(container.textContent).toContain("[[DISCLOSURE-PERFORMANCE]]");
  });

  it.each(SCENARIOS.map((s) => s.id))("%s: the broker's equity is the agents' plus a part no agent manages, never less", (scenario) => {
    const ws = buildWorkspace(scenario);
    if (ws.agents.length === 0) return;
    expect(unmanagedEquity(ws)).toBe(unmanagedEquity(WS));
    expect(Number(unmanagedEquity(ws))).toBeGreaterThan(0);
  });

  it("reads the crosshair out in Eastern time", () => {
    const { container } = renderWithRuntime(<AccountEquityChart />);
    const chart = onlyChart(container);
    const time = Date.parse("2026-09-28T14:03:00-04:00") / 1000;
    act(() => chart.crosshair[0]({ time, seriesData: new Map([[chart.series[0].api, { time, value: 24987.5 }]]) } as never));
    const readout = container.querySelector("[data-slot=time-chart] > p[aria-hidden]");
    expect(readout).toHaveTextContent("Sep 28, 14:03 ET");
    expect(readout).toHaveTextContent("$24,987.50");
    act(() => chart.crosshair[0]({ seriesData: new Map() } as never));
    expect(readout).not.toHaveTextContent("$24,987.50");
  });
});

describe("PositionChart", () => {
  it("draws candles with average cost in navy and the bracket in brass, at the position's prices", () => {
    const { container } = renderWithRuntime(<PositionChart agent={SWING} position={XYZ} />);
    const [series] = onlyChart(container).series;
    expect(series.type).toBe("Candlestick");
    const byId = new Map(series.priceLines.map((l) => [l.id, l]));
    const expected = {
      "avg-cost": { price: Number(XYZ.avg_cost), color: CHART_COLOR.lapis, title: "Average cost" },
      stop: { price: Number(XYZ.protection.stop_price), color: CHART_COLOR.mandateMarker, title: "Stop" },
      "take-profit": { price: Number(XYZ.protection.take_profit_price), color: CHART_COLOR.mandateMarker, title: "Take-profit" },
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
