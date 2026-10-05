import { cleanup, renderHook, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import ErrorScreen from "@/app/(app)/error";
import * as dashboard from "@/app/(app)/page";
import type { Scenario } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";
import { dockStop, isDisabled, renderWithRuntime, tabStop } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { AppShell } from "./app-shell";
import { useStopAttention } from "./stop-control";

beforeEach(() => setPathname("/"));

function pill(stop: HTMLElement): HTMLElement {
  const el = stop.querySelector<HTMLElement>("[data-slot=stop-pill]");
  if (!el) throw new Error("Stop has no pill");
  return el;
}

function renderShell(scenario: Scenario) {
  renderWithRuntime(
    <AppShell>
      <dashboard.default />
    </AppShell>,
    scenario,
  );
  return dockStop();
}

/** Classes that only paint: what may differ between the quiet and the loud Stop. */
const PAINT = /^(group-hover:)?(bg|text)-(ink|ink-foreground|card|background|ink\/\d+)$/;

describe("the Stop control is quiet until something needs you (DEC-206)", () => {
  it.each<Scenario>(["normal", "approvals", "paused", "empty", "loading", "result-unknown"])("is quiet in the %s scenario, with no description", (scenario) => {
    const stop = renderShell(scenario);
    expect(stop).toHaveAttribute("data-tone", "quiet");
    expect(stop).not.toHaveAttribute("aria-describedby");
    expect(stop).toHaveAccessibleName("Stop");
    expect(stop).toHaveAccessibleDescription("");
    expect(isDisabled(stop)).toBe(false);
    expect(pill(stop)).toHaveClass("border-ink", "bg-card", "text-ink");
    expect(pill(stop)).not.toHaveClass("bg-ink");
  });

  it.each<[Scenario, string]>([
    ["stale", "Needs attention: 4 alerts"],
    ["drawdown", "Needs attention: 2 alerts, Agent 1 has reached its drawdown 6% level"],
    ["reconciliation", "Needs attention: 1 alert"],
    ["unknown-order", "Needs attention: 1 order the broker has not confirmed"],
    ["unreachable", "Needs attention: the deployment is unreachable, 2 alerts"],
  ])("is loud in the %s scenario and says why to assistive technology", (scenario, description) => {
    const stop = renderShell(scenario);
    expect(stop).toHaveAttribute("data-tone", "loud");
    expect(stop).toHaveAccessibleName("Stop");
    expect(stop).toHaveAccessibleDescription(description);
    expect(stop.textContent).toBe("Stop");
    expect(isDisabled(stop)).toBe(false);
    expect(pill(stop)).toHaveClass("border-ink", "bg-ink", "text-ink-foreground");
  });

  it("keeps its description out of the reading order", () => {
    const stop = renderShell("stale");
    const description = document.getElementById(stop.getAttribute("aria-describedby") ?? "");
    expect(description).toHaveAttribute("hidden");
    expect(stop.contains(description)).toBe(false);
  });

  it("draws both tones in one box: only paint classes differ, and nothing animates", () => {
    const quiet = [...pill(renderShell("normal")).classList];
    cleanup();
    const loud = [...pill(renderShell("stale")).classList];
    const shape = (list: string[]) => list.filter((c) => !PAINT.test(c)).sort();
    expect(shape(loud)).toEqual(shape(quiet));
    for (const list of [quiet, loud]) {
      expect(list.filter((c) => /animate|pulse|ping|shadow|ring-(?!3|ring|offset)/.test(c) && !c.startsWith("group-focus-visible:"))).toEqual([]);
      expect(list).toContain("transition-colors");
      expect(list).toContain("duration-(--duration-hover)");
      expect(list).toContain("motion-reduce:transition-none");
    }
  });

  it("keeps a 44px hit area or more on the dock and the tab bar, and sits after the navigation on both", () => {
    const stop = renderShell("normal");
    expect(stop).toHaveAttribute("data-place", "dock");
    expect(stop).toHaveClass("h-12.5");
    expect(pill(stop)).toHaveClass("h-12.5");
    const tab = tabStop();
    expect(tab).toHaveAttribute("data-place", "tab");
    expect(tab).toHaveClass("h-(--tab-bar)");
    expect(pill(tab)).toHaveClass("h-11");
    for (const nav of [screen.getByRole("navigation", { name: "Primary" }), screen.getByRole("navigation", { name: "Main" })]) {
      const controls = within(nav).getAllByRole("button");
      expect(controls.filter((c) => c.getAttribute("data-slot") === "stop-control")).toHaveLength(1);
      expect(controls.at(-1)).toHaveAttribute("data-slot", "stop-control");
    }
  });

  it("is never in the header", () => {
    renderShell("stale");
    for (const header of screen.getAllByRole("banner")) expect(header.querySelector("[data-slot=stop-control]")).toBeNull();
  });

  it("stays enabled and quiet on the error screen", () => {
    renderWithRuntime(
      <AppShell>
        <ErrorScreen error={new Error("probe")} reset={() => {}} />
      </AppShell>,
    );
    const stop = dockStop();
    expect(isDisabled(stop)).toBe(false);
    expect(stop).toHaveAttribute("data-tone", "quiet");
  });
});

describe("useStopAttention", () => {
  it("starts quiet while the workspace loads", () => {
    const { result } = renderHook(() => useStopAttention(buildWorkspace("loading")));
    expect(result.current).toEqual([]);
  });

  it("keeps the last known reasons while the workspace loads, so Stop never flickers", () => {
    const { result, rerender } = renderHook(({ ws }) => useStopAttention(ws), { initialProps: { ws: buildWorkspace("stale") } });
    expect(result.current).toEqual(["4 alerts"]);
    rerender({ ws: buildWorkspace("loading") });
    expect(result.current).toEqual(["4 alerts"]);
    rerender({ ws: buildWorkspace("normal") });
    expect(result.current).toEqual([]);
    rerender({ ws: buildWorkspace("loading") });
    expect(result.current).toEqual([]);
  });
});
