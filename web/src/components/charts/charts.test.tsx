import { act, fireEvent, screen, within } from "@testing-library/react";
import { ColorType, LineStyle, LineType } from "lightweight-charts";
import { afterEach, describe, expect, it } from "vitest";
import { AGENT_IDS, APPROVAL_IDS, SCENARIOS, buildWorkspace } from "@/fixtures/workspace";
import { parseOklch, toHex } from "@/lib/color";
import { toFixed } from "@/lib/decimal";
import { direction, usd } from "@/lib/format";
import { agentLimits } from "@/lib/limits";
import { PALETTE, PALETTE_DARK } from "@/lib/palette";
import { type MockChart, chartControl, chartIn, liveCharts, pointerTime } from "@/test/chart-mock";
import { renderWithRuntime } from "@/test/harness";
import { drawn, spoken } from "@/test/spoken";
import { ACCOUNT_RANGE, AccountEquityChart, AgentEquityChart, unmanagedEquity } from "./equity-chart";
import { CHART_COLOR, CHART_TOKEN, type ChartLevel, HERO_FILL, LABEL_GAP, type Tone, areaOptions, baseOptions, candleOptions, crowdedLevels, heroAreaOptions, lineOptions, priceLineFor, setChartMode, trendColor, usdLabel } from "./options";
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

describe("AgentEquityChart", () => {
  it.each(WS.agents.map((a) => [a.label, a] as const))("%s: every price line is a mandate level at the mandate's value, labelled", (_label, agent) => {
    const { container } = renderWithRuntime(<AgentEquityChart agent={agent} />);
    const chart = onlyChart(container);
    const [series] = chart.series;
    expect(series.type).toBe("Area");
    expect(series.options.topColor).toBe(series.options.bottomColor);

    const levels = agentLimits(agent).levels;
    const [open] = series.priceLines.filter((l) => l.id === "open");
    expect(open).toMatchObject({ price: (series.data[0] as { value: number }).value, lineStyle: LineStyle.Dotted, axisLabelVisible: false, title: "" });
    for (const line of series.priceLines.filter((l) => l.id !== "open")) {
      const level = levels.find((l) => l.key === line.id);
      expect(level, String(line.id)).toBeDefined();
      expect(line.price).toBe(Number(toFixed(level!.at, 2)));
      expect(line.color).toBe(CHART_COLOR.mutedForeground);
      expect(line.lineStyle).toBe(LineStyle.Dashed);
      expect(line.title === "" ? !line.axisLabelVisible : line.title === level!.label && line.axisLabelVisible, String(line.id)).toBe(true);
    }
    expect(series.priceLines.some((l) => l.axisLabelVisible)).toBe(series.priceLines.length > 0);

    const legend = container.querySelector("[data-slot=level-legend]");
    const listed = [...(legend?.querySelectorAll("[data-level]") ?? [])];
    expect(listed.map((li) => li.getAttribute("data-level")).sort()).toEqual(levels.map((l) => l.key).sort());
    const drawnKeys = listed.filter((li) => li.getAttribute("data-drawn") === "true").map((li) => li.getAttribute("data-level"));
    expect(drawnKeys.sort()).toEqual(series.priceLines.filter((l) => l.id !== "open").map((l) => String(l.id)).sort());
  });

  it("hides the label of a level that would sit on the label above it, and keeps its line", () => {
    const level = (key: string, price: number): ChartLevel => ({ key, label: key, price, tone: "mandate" });
    const levels = [level("high", 110), level("daily", 90), level("drawdown", 90 - 100 * LABEL_GAP * 0.4), level("floor", 50)];
    expect([...crowdedLevels(levels, [100, 105])]).toEqual(["drawdown"]);
    expect(priceLineFor(levels[2], false)).toMatchObject({ title: "", axisLabelVisible: false, lineVisible: true, price: levels[2].price });
    expect(crowdedLevels([level("only", 10)], [])).toEqual(new Set());
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
  it("ends at the broker's equity, as the account's line, with the TradingView credit", () => {
    const { container } = renderWithRuntime(<AccountEquityChart />);
    const [series] = onlyChart(container).series;
    const data = series.data as Array<{ value: number }>;
    expect(series.options.lineColor).toBe(trendColor(direction((data.at(-1)!.value - data[0].value).toFixed(2))));
    expect(series.options.topColor).toBe(HERO_FILL);
    expect(series.options.bottomColor).toBe(HERO_FILL);
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

});

describe("the hero chart scrubs", () => {
  type Datum = { time: number; value: number };
  const hover = (chart: MockChart, p: Datum) =>
    act(() => chart.crosshair[0]({ time: p.time, point: { x: 10, y: 10 }, seriesData: new Map([[chart.series[0].api, p]]) } as never));
  const leave = (chart: MockChart) => act(() => chart.crosshair[0]({ seriesData: new Map() } as never));
  const hero = (container: HTMLElement) => ({
    value: container.querySelector("[data-slot=account-equity-value], [data-slot=agent-equity-value]")!,
    change: container.querySelector("[data-slot=hero-change]")!,
    when: container.querySelector("[data-slot=hero-when]")!,
    section: container.querySelector("section")!,
  });

  it("moves the hero figure, its change, and the time to the point under the pointer", () => {
    const { container } = renderWithRuntime(<AccountEquityChart />);
    const chart = onlyChart(container);
    const data = chart.series[0].data as Datum[];
    const point = data[Math.floor(data.length / 2)];
    hover(chart, point);
    const h = hero(container);
    expect(spoken(h.value)).toBe(usdLabel(point.value));
    expect(h.value.querySelector("[data-part=fraction]")).toHaveTextContent(usdLabel(point.value).slice(-3));
    expect(h.when).toHaveTextContent(/^Sep \d{2}, \d{2}:\d{2} ET$/);
    expect(h.section).toHaveAttribute("data-scrubbing");
    const change = Math.round((point.value - data[0].value) * 100) / 100;
    expect(h.change.querySelector("[data-direction]")).toHaveTextContent(usdLabel(Math.abs(change)).replace("\u2212", ""));
  });

  it("sets the hero figure in proportional lining figures, and the pill's figures tabular", () => {
    for (const ui of [<AccountEquityChart key="account" />, <AgentEquityChart key="agent" agent={WS.agents[0]} />]) {
      const { container, unmount } = renderWithRuntime(ui);
      const h = hero(container);
      expect(h.value).toHaveClass("text-display", "proportional-nums", "lining-nums");
      expect(h.value).not.toHaveClass("tabular");
      expect(h.value.querySelector(".tabular, .font-mono")).toBeNull();
      const figures = [...h.change.querySelectorAll(".font-mono")];
      expect(figures.length).toBeGreaterThan(0);
      for (const f of figures) expect(f).toHaveClass("tabular");
      expect(h.when).toHaveClass("tabular");
      unmount();
    }
  });

  it("returns to now when the pointer leaves the line", () => {
    const { container } = renderWithRuntime(<AccountEquityChart />);
    const chart = onlyChart(container);
    const data = chart.series[0].data as Datum[];
    hover(chart, data[3]);
    leave(chart);
    const h = hero(container);
    expect(h.value).toHaveTextContent(usd(WS.connection.account_equity));
    expect(h.when).toHaveTextContent("today");
    expect(h.section).not.toHaveAttribute("data-scrubbing");
  });

  it("keeps the sign, the word, and the performance disclosure on a scrubbed gain and a scrubbed loss", () => {
    const { container } = renderWithRuntime(<AccountEquityChart />);
    const chart = onlyChart(container);
    const data = chart.series[0].data as Datum[];
    const low = data.reduce((a, b) => (b.value < a.value ? b : a));
    const high = data.reduce((a, b) => (b.value > a.value ? b : a));
    expect(low.value).toBeLessThan(data[0].value);
    expect(high.value).toBeGreaterThan(data[0].value);
    for (const [point, sign, word] of [
      [low, "\u2212", "loss"],
      [high, "+", "gain"],
    ] as const) {
      hover(chart, point);
      const signed = hero(container).change.querySelector("[data-direction]")!;
      expect(signed).toHaveAttribute("data-direction", word);
      expect(spoken(signed)).toMatch(new RegExp(`^\\${sign}\\$[\\d,]+\\.\\d{2}${word}$`));
      expect(drawn(signed)).toMatch(new RegExp(`^\\${sign}\\$[\\d,]+\\.\\d{2}${word}$`));
      expect(hero(container).section.querySelector("[data-placeholder=performance]")).toHaveTextContent("[[DISCLOSURE-PERFORMANCE]]");
    }
  });

  it("sets the change on a soft pill toned by its sign, neutral at exactly zero, with the disclosure outside it on the same line", () => {
    const { container } = renderWithRuntime(<AccountEquityChart />);
    const chart = onlyChart(container);
    const data = chart.series[0].data as Datum[];
    const low = data.reduce((a, b) => (b.value < a.value ? b : a));
    const high = data.reduce((a, b) => (b.value > a.value ? b : a));
    for (const [point, tone, fill, ink, word] of [
      [high, "gain", "bg-gain-soft", "text-gain", "gain"],
      [low, "loss", "bg-loss-soft", "text-loss", "loss"],
      [data[0], "flat", "bg-muted", "text-foreground", "no change"],
    ] as const) {
      hover(chart, point);
      const pill = hero(container).change;
      expect(pill).toHaveAttribute("data-tone", tone);
      expect(pill.className.split(" ")).toEqual(expect.arrayContaining(["rounded-full", fill, ink]));
      for (const other of ["bg-gain-soft", "bg-loss-soft", "bg-muted"].filter((c) => c !== fill)) expect(pill).not.toHaveClass(other);
      expect(pill.querySelector("[data-direction]")).toHaveAttribute("data-direction", tone);
      expect(pill).toHaveTextContent(word);
      expect(pill.querySelector("[data-slot=hero-when]")).toHaveClass("text-muted-foreground");
      expect(pill.querySelector("[data-placeholder]")).toBeNull();
      const disclosure = pill.parentElement!.querySelector<HTMLElement>(":scope > [data-slot=disclosure]");
      expect(within(disclosure!).getByRole("button", { name: "Performance disclosure" })).toHaveAccessibleDescription("[[DISCLOSURE-PERFORMANCE]]");
      expect(pill.parentElement, "the symbol keeps the pill's line and baseline at every width").toHaveClass("flex", "items-baseline", "text-sm");
      expect(pill.parentElement!.className).not.toMatch(/flex-col/);
    }
    expect(spoken(hero(container).value)).toBe(usdLabel(data[0].value));
    expect(spoken(hero(container).change)).toMatch(/^\$0\.00no change\(0\.00%\)Sep \d{2}, \d{2}:\d{2} ET$/);
    expect(drawn(hero(container).change)).toMatch(/^\$0\.00no change\(0\.00%\)Sep \d{2}, \d{2}:\d{2} ET$/);
  });

  it("follows a finger: the crosshair is pinned to the nearest point and let go on release", () => {
    const { container } = renderWithRuntime(<AgentEquityChart agent={SWING} />);
    const chart = onlyChart(container);
    const data = chart.series[0].data as Datum[];
    const target = data[Math.floor(data.length / 3)];
    pointerTime.at = () => target.time + 7;
    const canvas = container.querySelector<HTMLElement>("[data-slot=chart-canvas]")!;
    fireEvent.pointerDown(canvas, { pointerType: "touch", clientX: 40 });
    expect(chart.pinned).toEqual({ price: target.value, time: target.time });
    expect(spoken(hero(container).value)).toBe(usdLabel(target.value));
    fireEvent.pointerUp(canvas, { pointerType: "touch", clientX: 40 });
    expect(chart.pinned).toBeNull();
    expect(hero(container).value).toHaveTextContent(usdLabel(data.at(-1)!.value));
  });

  it("updates a scrubbed figure at once, never through the roll a live change uses", () => {
    const { container } = renderWithRuntime(<AccountEquityChart />);
    const chart = onlyChart(container);
    expect(hero(container).value.querySelector("[data-instant]")).toBeNull();
    hover(chart, (chart.series[0].data as Datum[])[5]);
    expect(hero(container).value.querySelector("[data-instant]")).not.toBeNull();
    expect(hero(container).change.querySelector("[data-instant]")).not.toBeNull();
  });

  it("draws the line in on first load, and says a range longer than the history starts with it", () => {
    const { container } = renderWithRuntime(<AccountEquityChart />);
    expect(container.querySelector("[data-slot=chart-canvas]")).toHaveAttribute("data-draw-in");
    const picker = within(screen.getByRole("group", { name: "Account equity range" }));
    expect(picker.getAllByRole("button").map((b) => b.textContent)).toEqual(["1D", "1W", "1M", "3M", "1Y", "All"]);
    expect(picker.getByRole("button", { name: ACCOUNT_RANGE }), "the account opens on today, what an owner checks first").toHaveAttribute("aria-pressed", "true");
    expect(hero(container).when).toHaveTextContent("today");
    fireEvent.click(picker.getByRole("button", { name: "1Y" }));
    expect(hero(container).when).toHaveTextContent(/^since Sep 2\d$/);
    fireEvent.click(picker.getByRole("button", { name: "1D" }));
    expect(hero(container).when).toHaveTextContent("today");
  });

  it("keeps a hero chart still: no pan, no zoom, and no labels on the crosshair", () => {
    const { container } = renderWithRuntime(<AgentEquityChart agent={SWING} />);
    expect(onlyChart(container).options).toMatchObject({
      handleScroll: false,
      handleScale: false,
      kineticScroll: { touch: false, mouse: false },
      crosshair: { vertLine: { labelVisible: false }, horzLine: { visible: false } },
    });
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
