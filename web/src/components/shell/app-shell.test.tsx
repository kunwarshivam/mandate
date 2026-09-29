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
import { asPhone } from "@/test/viewport";
import { AppShell } from "./app-shell";
import { hiddenToTheRight } from "./status-strip";

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

  it("counts the items that end past its visible right edge, for the phone cue", () => {
    expect(hiddenToTheRight([80, 200, 300.4], 300)).toBe(0);
    expect(hiddenToTheRight([80, 301, 420], 300)).toBe(2);
    expect(hiddenToTheRight([], 300)).toBe(0);
  });

  it("shows no cue while every item is in view", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "stale");
    expect(document.querySelector("[data-slot=status-more]")).toBeNull();
  });
});

describe("Owlhead", () => {
  it("names the product Owlhead in the header, with the founder's mark below lg and the lockup from lg", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const [header] = screen.getAllByRole("banner");
    const home = within(header).getByRole("link", { name: "Owlhead, dashboard" });
    const mark = home.querySelector("[data-slot=owlhead-mark]");
    const lockup = home.querySelector("[data-slot=owlhead-lockup]");
    expect(mark).toHaveAttribute("aria-hidden", "true");
    expect(mark).toHaveClass("lg:hidden");
    expect(lockup).toHaveAttribute("aria-hidden", "true");
    expect(lockup).toHaveClass("hidden", "lg:block");
    expect(document.body.textContent).not.toMatch(/\bMandate\b/);
  });

  it("renders no sidebar on desktop, where the dock carries the navigation", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    expect(document.querySelector("[data-sidebar=header]")).toBeNull();
    expect(screen.getByRole("navigation", { name: "Primary" })).toHaveAttribute("data-slot", "dock");
  });

  it("sets the phone sheet's header on the page's own surface, with the brand in the logo colour and no account block", () => {
    const restore = asPhone();
    try {
      renderWithRuntime(<AppShell>{null}</AppShell>);
      const header = document.querySelector<HTMLElement>("[data-sidebar=header]");
      expect(header).toHaveClass("bg-background");
      expect(header?.closest("[data-surface]")).toBeNull();
      expect(header?.querySelector("[data-surface]")).toBeNull();
      expect(header?.querySelector("[data-slot=owlhead-lockup]")).toHaveAttribute("aria-hidden", "true");
      for (const brand of document.querySelectorAll<HTMLElement>("[data-slot=owlhead-mark], [data-slot=owlhead-lockup]")) {
        expect(brand.closest<HTMLElement>("[style]")?.style.color).toBe("var(--logo)");
        expect(brand.closest("[data-surface]")).toBeNull();
      }
    } finally {
      restore();
    }
  });

  it("offers light, dark and system from the theme menu, and a choice sets the mode and the cookie", async () => {
    const root = document.documentElement;
    try {
      renderWithRuntime(<AppShell>{null}</AppShell>);
      const triggers = screen.getAllByRole("button", { name: "Theme" });
      expect(triggers.length).toBe(1);
      fireEvent.click(triggers[0]);
      const menu = await screen.findByRole("menu");
      expect(within(menu).getAllByRole("menuitem").map((i) => i.textContent)).toEqual(["Light", "Dark", "System"]);
      fireEvent.click(within(menu).getByRole("menuitem", { name: "Dark" }));
      expect(root.dataset.mode).toBe("dark");
      expect(root.classList.contains("dark")).toBe(true);
      expect(root.dataset.themePref).toBe("dark");
      expect(document.cookie).toContain("owlhead-theme=dark");
    } finally {
      document.cookie = "owlhead-theme=; path=/; max-age=0";
      delete root.dataset.mode;
      delete root.dataset.themePref;
      root.classList.remove("dark");
      root.style.colorScheme = "";
    }
  });
});

describe("the frame", () => {
  it("frosts the header, the phone tab bar and the desktop dock, and nothing else", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const [header] = screen.getAllByRole("banner");
    const tabs = document.querySelector<HTMLElement>("nav[aria-label=Main].grid");
    const dock = screen.getByRole("navigation", { name: "Primary" });
    for (const frame of [header, tabs, dock]) {
      expect(frame).toHaveClass("glass");
      expect(frame).not.toHaveClass("bg-card");
    }
    expect([...document.querySelectorAll(".glass")]).toEqual([header, tabs, dock]);
  });
});

describe("approvals badge", () => {
  it("shows a count and nothing else, in the dock, the header, and the phone tab bar", () => {
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
