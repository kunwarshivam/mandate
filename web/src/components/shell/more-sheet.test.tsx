import { act, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { canOpen } from "@/lib/access";
import { ROLES, can } from "@/lib/roles";
import { SCREENS, SECTION_INDEX } from "@/lib/screens";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { AppShell } from "./app-shell";
import { moreGroups, phoneTabs } from "./more-sheet";

beforeEach(() => setPathname("/"));

function openMore() {
  fireEvent.click(within(screen.getByRole("navigation", { name: "Main" })).getByRole("button", { name: "More" }));
  return screen.getByRole("dialog", { name: "More" });
}

describe("what the phone reaches (DEC-207)", () => {
  it.each(ROLES.map((r) => r.id))("as %s, reaches every screen the role may open through the tabs and More, once, and only those", (role) => {
    const tabbed = phoneTabs(role).map((t) => t.href);
    const more = moreGroups(role).flatMap((g) => g.links.map((l) => l.href));
    const reached = new Set([...tabbed, ...more]);
    expect(reached.size).toBe(tabbed.length + more.length);
    for (const s of SCREENS) expect(reached.has(s.href), s.href).toBe(can(role, s.needs));
    for (const href of reached) expect(canOpen(role, href), href).toBe(true);
    const audits = SCREENS.some((s) => s.group === "audit" && can(role, s.needs));
    expect(reached.has(SECTION_INDEX.audit.href)).toBe(audits);
    const settings = SCREENS.some((s) => s.group === "workspace" && can(role, s.needs));
    expect(reached.has(SECTION_INDEX.workspace.href)).toBe(settings);
  });

  it("gives a role that sees agents Home, Approvals and Agents as tabs", () => {
    expect(phoneTabs("owner").map((t) => [t.label, t.href])).toEqual([
      ["Home", "/"],
      ["Approvals", "/approvals"],
      ["Agents", "/agents"],
    ]);
  });

  it("gives an auditor its own home, the audit, as its one tab", () => {
    expect(phoneTabs("auditor").map((t) => t.href)).toEqual(["/audit"]);
    expect(moreGroups("auditor").flatMap((g) => g.links.map((l) => l.href))).not.toContain("/audit");
  });

  it("groups Positions, Audit, Connections and Settings, in that order", () => {
    const groups = moreGroups("owner");
    const labels = groups.map((g) => g.label);
    const hrefs = groups.flatMap((g) => g.links.map((l) => l.href));
    expect(hrefs.indexOf("/positions")).toBeLessThan(hrefs.indexOf("/audit"));
    expect(hrefs.indexOf("/audit")).toBeLessThan(hrefs.indexOf("/settings"));
    expect(hrefs).toContain("/connections");
    expect(labels).toContain("Audit");
    expect(labels).toContain("Settings");
  });
});

describe("the More sheet", () => {
  afterEach(() => vi.useRealTimers());

  it("opens from the fourth tab with the account and workspace at the top, then Search, then every other screen", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const more = within(screen.getByRole("navigation", { name: "Main" })).getByRole("button", { name: "More" });
    const sheet = openMore();
    expect(more).toHaveAttribute("aria-expanded", "true");
    expect(sheet).toHaveAttribute("data-slot", "more-sheet");
    expect(sheet).not.toHaveClass("glass");
    const account = within(sheet).getByRole("region", { name: "Account and workspace" });
    expect(account).toHaveTextContent("Alpaca paper");
    expect(within(account).getByRole("button", { name: /^Workspace: / })).toBeInTheDocument();
    const search = within(sheet).getByRole("button", { name: /^Search/ });
    expect(account.compareDocumentPosition(search) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const hrefs = within(sheet)
      .getAllByRole("link")
      .map((a) => a.getAttribute("href"));
    expect(hrefs).toEqual(moreGroups("owner").flatMap((g) => g.links.map((l) => l.href)));
    for (const name of ["Positions", "Audit overview", "Connections", "All settings"]) expect(within(sheet).getByRole("link", { name })).toBeInTheDocument();
  });

  it("gives every row a 44px target and a visible focus ring", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const sheet = openMore();
    for (const row of [...within(sheet).getAllByRole("link"), within(sheet).getByRole("button", { name: /^Search/ })]) {
      expect(row).toHaveClass("min-h-11", "focus-visible:ring-3");
    }
    expect(within(sheet).getByRole("button", { name: "Close" })).toHaveClass("size-11");
    expect(sheet.className).toMatch(/bottom-\[calc\(var\(--tab-bar\)\+env\(safe-area-inset-bottom\)\)\]/);
  });

  it("opens in the frame's sheet layer, under the header and the tab bar, so it never covers Stop (DEC-452)", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const sheet = openMore();
    const layer = sheet.closest("[data-slot=sheet-layer]");
    expect(layer).not.toBeNull();
    const tabBar = screen.getByRole("navigation", { name: "Main" }).parentElement!;
    expect(layer?.parentElement).toBe(tabBar.parentElement);
    expect(layer!.compareDocumentPosition(tabBar) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    for (const el of [sheet, document.querySelector("[data-slot=sheet-backdrop]")]) expect(el).toHaveClass("z-20");
    expect(tabBar).toHaveClass("z-30");
  });

  it("closes when a screen is chosen", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const sheet = openMore();
    fireEvent.click(within(sheet).getByRole("link", { name: "Positions" }));
    expect(screen.queryByRole("dialog", { name: "More" })).toBeNull();
  });

  it("lists the feeds in the strip's words", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "stale");
    const feeds = within(openMore()).getByRole("region", { name: "Feeds" });
    expect(feeds).toHaveTextContent("Market data stale: as of 14:02:11");
    expect(feeds).toHaveTextContent("Fixture data");
  });

  it("hands Search to the command palette", () => {
    vi.useFakeTimers();
    renderWithRuntime(<AppShell>{null}</AppShell>);
    fireEvent.click(within(openMore()).getByRole("button", { name: /^Search/ }));
    act(() => vi.advanceTimersByTime(0));
    expect(screen.getByRole("combobox", { name: "Command" })).toBeInTheDocument();
  });
});
