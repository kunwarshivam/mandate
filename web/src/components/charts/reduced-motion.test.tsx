import { act } from "@testing-library/react";
import { beforeAll, describe, expect, it } from "vitest";
import { chartIn } from "@/test/chart-mock";
import { renderWithRuntime } from "@/test/harness";
import { AccountEquityChart } from "./equity-chart";

/** Its own file: Motion reads the reduced-motion preference once per module graph. */
beforeAll(() => {
  const matchMedia = window.matchMedia;
  window.matchMedia = (query: string) => ({ ...matchMedia(query), matches: /prefers-reduced-motion/.test(query), media: query }) as MediaQueryList;
});

describe("the hero chart with reduced motion", () => {
  it("does not draw the line in, and the scrubbed figure still follows the pointer", () => {
    const { container } = renderWithRuntime(<AccountEquityChart />);
    const canvas = container.querySelector("[data-slot=chart-canvas]")!;
    expect(canvas).not.toHaveAttribute("data-draw-in");
    expect(canvas.className).not.toMatch(/\bdraw-in\b/);
    const chart = chartIn(canvas)!;
    const point = (chart.series[0].data as Array<{ time: number; value: number }>)[4];
    act(() => chart.crosshair[0]({ time: point.time, point: { x: 1, y: 1 }, seriesData: new Map([[chart.series[0].api, point]]) } as never));
    expect(container.querySelector("[data-slot=account-equity-value] [data-instant]")).not.toBeNull();
  });

  it("rolls no figure: a live value swaps in place", () => {
    const { container } = renderWithRuntime(<AccountEquityChart />);
    const moving = container.querySelectorAll("[data-slot=account-equity-value] [style*=translateY]");
    for (const el of moving) expect((el as HTMLElement).style.transform).toMatch(/translateY\(0%\)/);
  });
});
