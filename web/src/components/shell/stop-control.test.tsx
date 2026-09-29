import { cleanup, renderHook, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import ErrorScreen from "@/app/(app)/error";
import * as dashboard from "@/app/(app)/page";
import type { Scenario } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";
import { isDisabled, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { AppShell } from "./app-shell";
import { useStopAttention } from "./stop-control";

beforeEach(() => setPathname("/"));

function headerStop(): HTMLElement {
  const [header] = screen.getAllByRole("banner");
  return within(header).getByRole("button", { name: "Stop" });
}

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
  return headerStop();
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

  it("keeps a 44px hit area at every width and a 40px pill from lg up, the command bar's height", () => {
    const stop = renderShell("normal");
    expect(stop).toHaveClass("h-11");
    expect([...stop.classList].some((c) => /^(lg|xl|md|sm):h-/.test(c))).toBe(false);
    expect(pill(stop)).toHaveClass("h-11", "lg:h-10");
    expect(document.querySelector("[data-slot=command-bar]")).toHaveClass("h-10");
  });

  it("stays enabled and quiet on the error screen", () => {
    renderWithRuntime(
      <AppShell>
        <ErrorScreen error={new Error("probe")} reset={() => {}} />
      </AppShell>,
    );
    const stop = headerStop();
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
