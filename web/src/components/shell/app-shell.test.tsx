import { act, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as agentsNew from "@/app/agents/new/page";
import * as agents from "@/app/agents/page";
import * as approvals from "@/app/approvals/page";
import * as audit from "@/app/audit/page";
import * as design from "@/app/design/page";
import * as dashboard from "@/app/page";
import * as settings from "@/app/settings/page";
import { SCENARIOS } from "@/fixtures/workspace";
import { RECORD_AFTER_MS, isDisabled, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { ROUTES } from "@/test/routes";
import { AppShell } from "./app-shell";

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
    const titles = [dashboard, agents, agentsNew, approvals, audit, settings, design].map(({ metadata }) => {
      const title = metadata.title;
      return typeof title === "object" && title && "absolute" in title ? title.absolute.replace(/ · Owlhead$/, "") : String(title);
    });
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

describe("Owlhead", () => {
  it("names the product Owlhead in the shell, with a typographic wordmark and no mark", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const home = screen.getAllByRole("link", { name: "Owlhead, dashboard" });
    expect(home.length).toBeGreaterThan(0);
    for (const link of home) {
      expect(link).toHaveTextContent("Owlhead");
      expect(link.querySelector("svg")).toBeNull();
    }
    expect(document.body.textContent).not.toMatch(/\bMandate\b/);
  });

  it("offers no theme toggle: Placard is light only", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    expect(screen.queryByRole("button", { name: /theme/i })).toBeNull();
  });
});

describe("approvals badge", () => {
  it("shows a count and nothing else, in the sidebar, the header, and the phone tab bar", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "approvals");
    const counts = document.querySelectorAll("[data-slot=approvals-count]");
    expect(counts.length).toBe(3);
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

  it.each(SCENARIO_IDS)("writes nothing to browser storage in the %s scenario", (scenario) => {
    renderWithRuntime(
      <AppShell>
        <dashboard.default />
      </AppShell>,
      scenario,
    );
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: /Pause all agents/ }));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));

    expect(writes).toEqual([]);
  });
});
