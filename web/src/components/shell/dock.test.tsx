import { fireEvent, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { canOpen } from "@/lib/access";
import { ROLES, can } from "@/lib/roles";
import { GROUP_LABEL, SCREENS, SECTION_INDEX } from "@/lib/screens";
import { AGENT_IDS } from "@/fixtures/workspace";
import { dockStop, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { asPhone } from "@/test/viewport";
import { AppShell } from "./app-shell";
import { DOCK_LINKS, isCurrent, menuFor, menuGroups } from "./dock";

beforeEach(() => setPathname("/"));

function dock() {
  return screen.getByRole("navigation", { name: "Primary" });
}

async function openMenu(name: string) {
  fireEvent.click(within(dock()).getByRole("button", { name }));
  return screen.findByRole("menu");
}

describe("the desktop dock", () => {
  it("labels every item under its icon: the everyday screens in order, then the More menu; no unbuilt screen has a door (DEC-513)", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "approvals");
    const names = (els: HTMLElement[]) => els.map((el) => el.querySelector("[data-slot=dock-label]")?.textContent);
    expect(within(dock()).getAllByRole("link").map((l) => l.getAttribute("href"))).toEqual(["/", "/messages", "/approvals", "/alerts", "/agents", "/positions"]);
    expect(names(within(dock()).getAllByRole("link"))).toEqual(["Home", "Messages", "Approvals", "Alerts", "Agents", "Positions"]);
    const menus = within(dock()).getAllByRole("button").filter((b) => b.getAttribute("data-slot") !== "stop-control");
    expect(names(menus)).toEqual(["More"]);
    expect(within(dock()).getByRole("link", { name: /^Approvals\s*\d+\s*open$/ })).toBeInTheDocument();
    expect(within(dock()).getByRole("link", { name: "Agents" })).toBeInTheDocument();
    for (const b of menus) {
      expect(b).toHaveAttribute("aria-haspopup", "menu");
      expect(b).not.toHaveAttribute("aria-label");
    }
    for (const label of dock().querySelectorAll("[data-slot=dock-label]")) {
      expect(label, "the current item is marked by a semibold label (DEC-478 item 4)").toHaveClass("group-data-current:font-semibold");
      expect(label.className, "so no label is semibold or bolder until it is current").not.toMatch(/(^|\s)font-(semibold|bold|extrabold|black)\b/);
    }
  });

  it("has no tooltips: the labels name every item, and no dock item has a shortcut to add", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    for (const item of [...within(dock()).getAllByRole("link"), ...within(dock()).getAllByRole("button")]) {
      fireEvent.mouseEnter(item);
      fireEvent.focus(item);
      expect(item).not.toHaveAttribute("aria-describedby");
      expect(item).not.toHaveAttribute("aria-keyshortcuts");
    }
    expect(document.querySelector(".kumo-tooltip-popup")).toBeNull();
  });

  it("reserves a divider between the everyday screens and the menus", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const divider = dock().querySelector("[data-slot=dock-divider]");
    expect(divider).toHaveAttribute("aria-hidden");
    expect(divider?.nextElementSibling).toHaveTextContent(/^More/);
  });

  it("marks the current screen, and only it, with aria-current and the pill", () => {
    setPathname("/positions");
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const current = within(dock()).getAllByRole("link").filter((l) => l.getAttribute("aria-current") === "page");
    expect(current.map((l) => l.getAttribute("href"))).toEqual(["/positions"]);
    expect(current[0]).toHaveAttribute("data-current");
    expect(dock().querySelectorAll("[data-current]")).toHaveLength(1);
    expect(current[0]).toHaveClass("data-current:bg-(--dock-current)", "hover:bg-(--dock-hover)");
    expect(current[0].className).not.toMatch(/bg-(ink|primary|foreground|lapis|mandate)\b/);
  });

  it("keeps Agents current inside an agent, but not on New agent, which lives in More", () => {
    setPathname(`/agents/${AGENT_IDS.btc}/orders`);
    const { unmount } = renderWithRuntime(<AppShell>{null}</AppShell>);
    expect(within(dock()).getByRole("link", { name: "Agents" })).toHaveAttribute("aria-current", "page");
    unmount();
    setPathname("/agents/new");
    renderWithRuntime(<AppShell>{null}</AppShell>);
    expect(within(dock()).queryByRole("link", { current: "page" })).toBeNull();
    expect(within(dock()).getByRole("button", { name: "More" })).toHaveAttribute("aria-current", "true");
  });

  it("marks More current on any audit screen, and the screen inside the menu; an audit screen still to come has no menu item (DEC-513)", async () => {
    setPathname("/audit/decisions");
    renderWithRuntime(<AppShell>{null}</AppShell>);
    expect(within(dock()).getByRole("button", { name: "More" })).toHaveAttribute("aria-current", "true");
    const menu = await openMenu("More");
    expect(within(menu).getByRole("menuitem", { name: "Gate decisions" })).toHaveAttribute("aria-current", "page");
    expect(within(menu).getByRole("menuitem", { name: "Audit overview" })).not.toHaveAttribute("aria-current");
    expect(within(menu).queryByRole("menuitem", { name: "Trace" })).toBeNull();
    expect(within(menu).queryByRole("menuitem", { name: "Connections" })).toBeNull();
  });

  it("heads More with the account, then the workspace overview alone while every workspace screen is still to come", async () => {
    setPathname("/settings/policies");
    renderWithRuntime(<AppShell>{null}</AppShell>);
    expect(within(dock()).getByRole("button", { name: "More" })).toHaveAttribute("aria-current", "true");
    const menu = await openMenu("More");
    expect(menu).toHaveTextContent(/^Account/);
    expect(within(menu).getByRole("menuitem", { name: /Alpaca paper/ })).toBeInTheDocument();
    expect(within(menu).queryByRole("menuitem", { name: "Policies" })).toBeNull();
    expect(within(menu).getByRole("menuitem", { name: "Workspace overview" })).toHaveAttribute("href", "/settings");
  });

  it("shows the open approvals count on Approvals", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "approvals");
    const count = within(dock()).getByRole("link", { name: /^Approvals/ }).querySelector("[data-slot=approvals-count]");
    expect(count?.textContent).toMatch(/^\d+ open$/);
  });

  it("gives every item a visible focus ring", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const stop = dockStop();
    const items = [...within(dock()).getAllByRole("link"), ...within(dock()).getAllByRole("button")].filter((el) => el !== stop);
    for (const item of items) expect(item).toHaveClass("focus-visible:ring-3");
    expect(stop.querySelector("[data-slot=stop-pill]")).toHaveClass("group-focus-visible:ring-3");
  });

  it("sits fixed at the bottom from lg only, and phones keep the tab bar", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const holder = dock().parentElement!;
    expect(holder).toHaveClass("fixed", "hidden", "lg:block", "left-1/2", "-translate-x-1/2");
    expect(holder.className).toMatch(/bottom-\[calc\(var\(--dock-gap\)/);
    expect(document.querySelector("nav[aria-label=Main].grid")?.parentElement).toHaveClass("lg:hidden");
  });

  it("gives way to the tab bar below lg, with no sidebar sheet; More keeps the account", () => {
    const restore = asPhone();
    try {
      setPathname(`/agents/${AGENT_IDS.btc}`);
      renderWithRuntime(<AppShell>{null}</AppShell>);
      expect(document.querySelector("nav[data-mobile], [data-sidebar]")).toBeNull();
      fireEvent.click(within(screen.getByRole("navigation", { name: "Main" })).getByRole("button", { name: "More" }));
      expect(within(screen.getByRole("dialog", { name: "More" })).getByRole("region", { name: "Account and workspace" })).toHaveTextContent("Alpaca paper");
    } finally {
      restore();
    }
  });

});

describe("what the dock reaches", () => {
  it.each(ROLES.map((r) => r.id))("as %s, reaches every screen the role may open, and only those", (role) => {
    const docked = SCREENS.filter((s) => (DOCK_LINKS as readonly string[]).includes(s.key) && can(role, s.needs)).map((s) => s.href);
    const menus = menuGroups("more", role).flatMap((g) => g.links.map((l) => l.href));
    const reached = new Set([...docked, ...menus]);
    expect(reached.size).toBe(docked.length + menus.length);
    for (const s of SCREENS) expect(reached.has(s.href), s.href).toBe(s.built && can(role, s.needs));
    for (const href of reached) expect(canOpen(role, href), href).toBe(true);
    const audits = SCREENS.some((s) => s.group === "audit" && can(role, s.needs));
    expect(reached.has(SECTION_INDEX.audit.href)).toBe(audits);
  });

  it("puts every screen that is not on the dock in More, under its group label, the audit and workspace groups opening with their overview", () => {
    const groups = menuGroups("more", "owner");
    expect(groups.map((g) => g.label)).toContain(GROUP_LABEL.audit);
    expect(groups.find((g) => g.label === GROUP_LABEL.audit)?.links[0]).toMatchObject({ href: SECTION_INDEX.audit.href });
    expect(groups.find((g) => g.label === GROUP_LABEL.workspace)?.links.map((l) => l.href)).toEqual([SECTION_INDEX.workspace.href]);
    expect(menuFor("audit")).toBe("more");
    expect(menuFor("workspace")).toBe("more");
  });

  it("matches the current screen by path", () => {
    expect(isCurrent("/", "/")).toBe(true);
    expect(isCurrent("/agents", "/")).toBe(false);
    expect(isCurrent("/audit/trace", "/audit")).toBe(false);
    expect(isCurrent("/settings/billing", "/settings")).toBe(false);
    expect(isCurrent("/agents/agt_1/mandate", "/agents")).toBe(true);
    expect(isCurrent("/agents/new", "/agents")).toBe(false);
    expect(isCurrent("/connections/con_1/stop-all", "/connections")).toBe(true);
    expect(isCurrent("/positionsx", "/positions")).toBe(false);
  });
});
