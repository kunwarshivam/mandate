import { act, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as agentsNew from "@/app/(app)/agents/new/page";
import * as agents from "@/app/(app)/agents/page";
import * as approvals from "@/app/(app)/approvals/page";
import * as audit from "@/app/(app)/audit/page";
import * as design from "@/app/(app)/design/page";
import * as dashboard from "@/app/(app)/page";
import * as settings from "@/app/(app)/settings/page";
import { AGENT_IDS, APPROVAL_IDS, SCENARIOS } from "@/fixtures/workspace";
import { can } from "@/lib/roles";
import { RECORD_AFTER_MS, dockStop, isDisabled, renderWithRuntime, tabStop } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { ROUTES } from "@/test/routes";
import { asPhone, shownOnDesktop, shownOnPhone } from "@/test/viewport";
import { AppShell } from "./app-shell";
import { fullBleed } from "./frame";
import { hiddenToTheRight } from "./status-strip";

const SCENARIO_IDS = SCENARIOS.map((s) => s.id);

beforeEach(() => setPathname("/"));

describe("every screen in every scenario", () => {
  it.each(ROUTES.flatMap(([path, Page]) => SCENARIO_IDS.map((scenario) => [path, scenario, Page] as const)))(
    "%s in %s shows the paper badge and an enabled Stop control on the dock and the tab bar",
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
      expect(within(header).queryByRole("button", { name: "Stop" })).toBeNull();
      expect(isDisabled(dockStop())).toBe(false);
      expect(isDisabled(tabStop())).toBe(false);
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
  it("names the product Owlhead in the header, with the brand owl at every width and the wordmark from lg (DEC-452)", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const [header] = screen.getAllByRole("banner");
    const home = within(header).getByRole("link", { name: "Owlhead, dashboard" });
    const owl = home.querySelector("[data-slot=brand-owl] svg[data-slot=owl]");
    const wordmark = home.querySelector("[data-slot=owlhead-wordmark]");
    expect(owl).toHaveAttribute("aria-hidden", "true");
    expect(owl).toHaveAttribute("data-mood", "awake");
    expect(owl?.querySelector("rect[fill='var(--brand-owl)']")).not.toBeNull();
    expect(wordmark).toHaveAttribute("aria-hidden", "true");
    expect(wordmark).toHaveClass("hidden", "lg:block");
    expect(home.querySelector("[data-slot=owlhead-mark], [data-slot=owlhead-lockup]")).toBeNull();
    expect(document.body.textContent).not.toMatch(/\bMandate\b/);
  });

  it("renders no sidebar on desktop, where the dock carries the navigation", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    expect(document.querySelector("[data-sidebar=header]")).toBeNull();
    expect(screen.getByRole("navigation", { name: "Primary" })).toHaveAttribute("data-slot", "dock");
  });

  it("sets the brand in the logo colour, never on a block of colour", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const brands = document.querySelectorAll<HTMLElement>("[data-slot=brand-lockup]");
    expect(brands.length).toBeGreaterThan(0);
    for (const brand of brands) {
      expect(brand.closest<HTMLElement>("[style]")?.style.color).toBe("var(--logo)");
      expect(brand.closest("[data-surface]")).toBeNull();
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

const CONTROLS = "a[href], button, [role=button], input, select, textarea";

function phoneControls(root: Element) {
  return [...root.querySelectorAll(CONTROLS)].filter(shownOnPhone);
}

describe("the phone frame (DEC-207)", () => {
  it.each(["owner", "approver", "viewer", "auditor"] as const)("as %s, holds two things in the header: the mark and the paper badge", (role) => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "normal", { role });
    const [header] = screen.getAllByRole("banner");
    const things = [...header.querySelectorAll(`${CONTROLS}, [data-slot=environment-badge]`)].filter(shownOnPhone);
    expect(things.map((c) => c.getAttribute("aria-label") ?? c.textContent)).toEqual([
      expect.stringMatching(/^Owlhead, /),
      expect.stringMatching(/^PAPER/),
    ]);
    expect(within(header).queryByRole("button", { name: "Go to…" })).toBeNull();
    expect(within(header).queryByRole("button", { name: /sidebar/i })).toBeNull();
  });

  it("offers four tabs: Home, Messages with the count of requests waiting, Agents and More, then Stop at the bar's end", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "approvals");
    const tabs = screen.getByRole("navigation", { name: "Main" });
    expect(tabs.parentElement).toHaveClass("lg:hidden");
    const items = phoneControls(tabs);
    expect(items.map((i) => i.textContent?.replace(/\d+ open/, "").trim())).toEqual(["Home", "Messages", "Agents", "More", "Stop"]);
    expect(items.map((i) => i.getAttribute("href"))).toEqual(["/", "/messages", "/agents", null, null]);
    expect(items[1].querySelector("[data-slot=approvals-count]")?.textContent).toMatch(/^\d+ open$/);
    expect(items[3]).toHaveAttribute("aria-haspopup", "dialog");
    expect(items[3]).toHaveAttribute("aria-expanded", "false");
    expect(items[4]).toHaveAttribute("data-slot", "stop-control");
    for (const item of items) expect(item).toHaveClass("h-(--tab-bar)");
    expect(tabs).toHaveClass("pb-[env(safe-area-inset-bottom)]");
  });

  it.each(["viewer", "auditor"] as const)("as %s, who may not stop, shows no Stop on the tab bar or the dock", (role) => {
    expect(can(role, "stop.open")).toBe(false);
    renderWithRuntime(<AppShell>{null}</AppShell>, "normal", { role });
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
    expect(document.querySelector("[data-slot=dock-stop-divider]")).toBeNull();
  });

  it("opens one Stop sheet from either the dock or the tab bar", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    fireEvent.click(tabStop());
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    fireEvent.click(dockStop());
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
  });

  it("keeps one navigation: no sidebar sheet or menu button, even when the browser reports a phone", () => {
    const restore = asPhone();
    try {
      setPathname(`/agents/${AGENT_IDS.btc}`);
      renderWithRuntime(<AppShell>{null}</AppShell>);
      expect(document.querySelector("[data-sidebar]")).toBeNull();
      expect(document.querySelector("nav[data-mobile]")).toBeNull();
      expect(screen.queryByRole("button", { name: /^(Expand|Collapse|Open|Toggle) (sidebar|menu)$/i })).toBeNull();
      expect(screen.getAllByRole("navigation").filter(shownOnPhone).map((n) => n.getAttribute("aria-label"))).toEqual(["Main"]);
    } finally {
      restore();
    }
  });

  it("marks More as current on a screen that lives under it", () => {
    setPathname("/positions");
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const tabs = screen.getByRole("navigation", { name: "Main" });
    expect(within(tabs).getByRole("button", { name: "More" })).toHaveAttribute("aria-current", "page");
    for (const link of within(tabs).getAllByRole("link")) expect(link).not.toHaveAttribute("aria-current");
  });

  it("shows no strip and no banner on a phone while every feed answers", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    expect(document.querySelector("[data-slot=feed-banner]")).toBeNull();
    const strip = document.querySelector("[data-slot=status-strip]");
    if (strip) expect(shownOnPhone(strip)).toBe(false);
  });

  it.each([
    ["stale", ["Market data stale: as of 14:02:11, 3 min ago", "Push relay down"]],
    ["unreachable", ["Deployment unreachable since 13:58:02; agent data hidden"]],
  ] as const)("shows a one-line banner of only the failing feeds when %s, in the strip's words and height", (scenario, lines) => {
    renderWithRuntime(<AppShell>{null}</AppShell>, scenario);
    const banner = screen.getByRole("region", { name: "Feed warning" });
    expect(banner).toHaveAttribute("data-slot", "feed-banner");
    expect(banner).toHaveClass("h-(--status-row)", "whitespace-nowrap");
    expect(shownOnPhone(banner)).toBe(true);
    expect(banner.parentElement?.parentElement).toHaveClass("lg:hidden");
    for (const line of lines) expect(banner).toHaveTextContent(line);
    expect(banner).not.toHaveTextContent(/as of \d\d:\d\d:\d\d$|Broker as of|Push relay as of/);
    expect(shownOnPhone(screen.getByRole("region", { name: "System status" }))).toBe(false);
  });

  it("shows no banner while the deployment is still connecting", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "loading");
    expect(document.querySelector("[data-slot=feed-banner]")).toBeNull();
  });

  it("keeps the full strip on a phone on a record screen, whatever the feeds say", () => {
    setPathname(`/approvals/${APPROVAL_IDS.swingXyz}`);
    renderWithRuntime(<AppShell>{null}</AppShell>, "stale");
    expect(shownOnPhone(screen.getByRole("region", { name: "System status" }))).toBe(true);
    expect(document.querySelector("[data-slot=feed-banner]")).toBeNull();
  });

  it("labels fixture data at the foot of a phone screen, where the strip used to", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "stale");
    const foot = document.querySelector<HTMLElement>("[data-slot=page-footer]")!;
    expect(shownOnPhone(foot)).toBe(true);
    expect(within(foot).getByText("Fixture data")).toBeInTheDocument();
  });
});

describe("the desktop frame (DEC-215)", () => {
  it("shows nothing under the header while every feed answers, and labels fixture data at the foot of the page", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    expect(document.querySelector("[data-slot=status-strip]")).toBeNull();
    expect(document.querySelector("[data-slot=wire]")).toBeNull();
    const foot = document.querySelector<HTMLElement>("[data-slot=page-footer]")!;
    expect(foot).not.toHaveClass("lg:hidden");
    expect(foot).toHaveClass("lg:pb-[calc(var(--dock-clearance)+2rem)]");
    expect(within(foot).getByText("Fixture data")).toBeInTheDocument();
  });

  it.each(["stale", "unreachable", "loading"] as const)("shows the full status strip from lg when %s, with its own fixture tag", (scenario) => {
    renderWithRuntime(<AppShell>{null}</AppShell>, scenario);
    const strip = screen.getByRole("region", { name: "System status" });
    expect(strip.parentElement?.parentElement).toHaveClass("max-lg:hidden");
    expect(within(strip).getByText("Fixture data")).toBeInTheDocument();
    expect(document.querySelector("[data-slot=page-footer]")).toHaveClass("lg:hidden");
  });

  it("keeps the full strip at every width on a record screen, and no fixture tag at the foot", () => {
    setPathname(`/approvals/${APPROVAL_IDS.swingXyz}`);
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const strip = screen.getByRole("region", { name: "System status" });
    expect(strip.parentElement?.parentElement).not.toHaveClass("max-lg:hidden");
    expect(within(document.querySelector<HTMLElement>("[data-slot=page-footer]")!).queryByText("Fixture data")).toBeNull();
  });
});

describe("the frame", () => {
  it.each([
    ["/messages", true],
    ["/messages/agt_1", true],
    ["/messages/agt_1/desk", true],
    ["/", false],
    ["/messagesx", false],
    ["/agents/agt_1/activity", false],
  ])("fills the window edge to edge on %s: %s", (path, bleed) => {
    expect(fullBleed(path)).toBe(bleed);
  });

  it("holds Messages to the window's height, above the phone's tab bar, with no gutter and no page footer", () => {
    setPathname("/messages");
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const main = screen.getByRole("main");
    expect(main).toHaveClass("flex", "min-h-0", "flex-col");
    expect(main).not.toHaveClass("px-(--page-x)", "max-w-(--content-max)");
    expect(main.parentElement).toHaveClass("h-dvh", "max-lg:pb-[calc(var(--tab-bar)+env(safe-area-inset-bottom))]");
    expect(document.querySelector("[data-slot=page-footer]")).toBeNull();
  });

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
  it("shows a count and nothing else, once at each width: in the dock and the phone tab bar, never the header", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "approvals");
    const counts = [...document.querySelectorAll("[data-slot=approvals-count]")];
    expect(counts.length).toBe(2);
    for (const c of counts) expect(c.textContent).toMatch(/^\d+ open$/);
    const [header] = screen.getAllByRole("banner");
    expect(header.querySelector("[data-slot=approvals-count]")).toBeNull();
    expect(within(header).queryByRole("link", { name: "Approvals" })).toBeNull();
    expect(counts.filter(shownOnDesktop)).toHaveLength(1);
    expect(counts.filter(shownOnPhone)).toHaveLength(1);
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
    fireEvent.click(dockStop());
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: /Pause all agents/ }));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));

    expect(writes).toEqual([]);
  });
});
