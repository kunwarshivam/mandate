import type { ComponentType } from "react";
import { act, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as agentsNew from "@/app/agents/new/page";
import * as agents from "@/app/agents/page";
import * as approvals from "@/app/approvals/page";
import * as audit from "@/app/audit/page";
import * as design from "@/app/design/page";
import ErrorScreen from "@/app/error";
import Loading from "@/app/loading";
import NotFound from "@/app/not-found";
import * as dashboard from "@/app/page";
import * as settings from "@/app/settings/page";
import { SCENARIOS } from "@/fixtures/workspace";
import { RECORD_AFTER_MS, isDisabled, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { AppShell } from "./app-shell";
import { THEME_KEY } from "./theme-toggle";

const ROUTES: Array<[string, ComponentType]> = [
  ["/", dashboard.default],
  ["/agents", agents.default],
  ["/agents/new", agentsNew.default],
  ["/approvals", approvals.default],
  ["/audit", audit.default],
  ["/settings", settings.default],
  ["/design", design.default],
  ["/loading", Loading],
  ["/not-found", NotFound],
  ["/error", () => <ErrorScreen error={new Error("render failed")} reset={() => {}} />],
];

const SCENARIO_IDS = SCENARIOS.map((s) => s.id);

beforeEach(() => setPathname("/"));

describe("every screen in every scenario", () => {
  it.each(ROUTES.flatMap(([path, Page]) => SCENARIO_IDS.map((scenario) => [path, scenario, Page] as const)))(
    "%s in %s shows the paper badge and an enabled Stop control",
    (path, scenario, Page) => {
      setPathname(path);
      renderWithRuntime(
        <AppShell>
          <Page />
        </AppShell>,
        scenario,
      );
      const [header] = screen.getAllByRole("banner");
      expect(within(header).getByText("PAPER").closest("[data-slot=environment-badge]")).toHaveTextContent("PAPER·simulated funds");
      const stop = within(header).getByRole("button", { name: "Stop" });
      expect(isDisabled(stop)).toBe(false);
      expect(screen.getByRole("main")).toBeInTheDocument();
      expect(screen.getAllByText("Fixture data").length).toBeGreaterThan(0);
    },
  );

  it("keeps page titles generic: no agent names, tickers, or amounts", () => {
    const titles = [dashboard, agents, agentsNew, approvals, audit, settings, design].map((m) => String(m.metadata.title));
    for (const title of titles) expect(title).toMatch(/^[A-Z][a-z]+( [a-z]+)*$/);
  });
});

describe("status strip", () => {
  it("states stale market data with its age and counts degraded items", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "stale");
    const strip = screen.getByRole("region", { name: "System status" });
    expect(strip).toHaveTextContent("Market data stale: as of 14:02:11, 3 min ago");
    expect(strip).toHaveTextContent("Push relay down");
    expect(within(strip).getByText("2 degraded")).toBeInTheDocument();
  });

  it("says the deployment is unreachable and hides agent data", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "unreachable");
    expect(screen.getByRole("region", { name: "System status" })).toHaveTextContent("Deployment unreachable since 13:58:02; agent data hidden");
  });
});

describe("approvals badge", () => {
  it("shows a count and nothing else", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "approvals");
    const counts = document.querySelectorAll("[data-slot=approvals-count]");
    expect(counts.length).toBe(2);
    for (const c of counts) expect(c.textContent).toMatch(/^\d+ open$/);
  });
});

describe("browser storage", () => {
  let local: ReturnType<typeof vi.spyOn>;
  let writes: Array<[Storage, string]>;

  beforeEach(() => {
    vi.useFakeTimers();
    writes = [];
    local = vi.spyOn(Storage.prototype, "setItem").mockImplementation(function (this: Storage, key: string) {
      writes.push([this, key]);
    });
  });

  afterEach(() => {
    local.mockRestore();
    vi.useRealTimers();
  });

  it.each(SCENARIO_IDS)("writes nothing but the theme in the %s scenario", (scenario) => {
    renderWithRuntime(
      <AppShell>
        <dashboard.default />
      </AppShell>,
      scenario,
    );
    fireEvent.click(screen.getByRole("button", { name: /Use (dark|light) theme/ }));
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: /Pause all agents/ }));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));

    expect(writes.map(([, key]) => key)).toEqual([THEME_KEY]);
    expect(writes.every(([store]) => store === window.localStorage)).toBe(true);
  });
});
