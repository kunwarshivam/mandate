import { act } from "@testing-library/react";
import { hydrateRoot } from "react-dom/client";
import { renderToString } from "react-dom/server";
import { afterEach, describe, expect, it } from "vitest";
import { toHex } from "@/lib/color";
import { PALETTE, PALETTE_DARK } from "@/lib/palette";
import { charts } from "@/test/chart-mock";
import { CHART_COLORS, setChartMode } from "./options";
import { type ChartSeries, TimeChart } from "./time-chart";

/**
 * The account chart's dark-mode paint (web/design/plan.md section B). The server has no document, so
 * it renders the chart's frame in light; the head script sets `<html data-mode>` before React
 * hydrates, and the canvas is drawn only on the client. A chart that took the server's light on
 * hydration drew a light chart into a dark page, then tore it down and drew again a moment later,
 * which left the plot blank while the page was still busy loading. Hydration must draw once, in the
 * document's own mode and colour-blind setting.
 */

const SERIES: ChartSeries = {
  kind: "area",
  tone: "account",
  trend: "gain",
  points: [
    { time: 1_790_000_000, value: 100 },
    { time: 1_790_000_060, value: 101 },
    { time: 1_790_000_120, value: 103 },
  ],
};
const NO_SCRUB = () => {};

function Hero() {
  return <TimeChart label="Account equity, today" summary="Account equity today." series={SERIES} onScrub={NO_SCRUB} pulse />;
}

async function hydrateIn(mode: "light" | "dark", cvd: boolean): Promise<() => void> {
  delete document.documentElement.dataset.mode;
  delete document.documentElement.dataset.cvd;
  const html = renderToString(<Hero />);
  const host = document.createElement("div");
  host.innerHTML = html;
  document.body.append(host);
  document.documentElement.dataset.mode = mode;
  if (cvd) document.documentElement.dataset.cvd = "on";
  const root = await act(async () => hydrateRoot(host, <Hero />));
  return () => {
    act(() => root.unmount());
    host.remove();
  };
}

function background(chart: (typeof charts)[number]): unknown {
  return (chart.options.layout as { background?: { color?: string } } | undefined)?.background?.color;
}

afterEach(() => {
  delete document.documentElement.dataset.mode;
  delete document.documentElement.dataset.cvd;
  setChartMode("light");
});

describe("a hydrated chart draws once, in the document's mode", () => {
  it.each(["light", "dark"] as const)("%s: one chart, on that mode's card", async (mode) => {
    const unmount = await hydrateIn(mode, false);
    try {
      expect(charts.map(background), "every chart drawn while hydrating").toEqual([CHART_COLORS[mode].card]);
      expect(CHART_COLORS[mode].card).toBe(toHex((mode === "dark" ? PALETTE_DARK : PALETTE).tokens.card.value));
    } finally {
      unmount();
    }
  });

  it("dark and colour-blind: the first chart already uses the colour-blind dark area", async () => {
    const unmount = await hydrateIn("dark", true);
    try {
      expect(charts, "no redraw after hydration").toHaveLength(1);
      expect(background(charts[0])).toBe(CHART_COLORS.dark.card);
      expect(charts[0].series[0].options.lineColor).toBe(CHART_COLORS.dark.gainCvd);
    } finally {
      unmount();
    }
  });
});
