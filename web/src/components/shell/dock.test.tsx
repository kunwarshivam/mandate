import { fireEvent, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { canOpen } from "@/lib/access";
import { ROLES, can } from "@/lib/roles";
import { SCREENS } from "@/lib/screens";
import { AGENT_IDS } from "@/fixtures/workspace";
import { dockStop, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { asPhone } from "@/test/viewport";
import { AppShell } from "./app-shell";
import { DOCK_LINKS, isCurrent } from "./dock";

beforeEach(() => setPathname("/"));

function dock() {
  return screen.getByRole("navigation", { name: "Primary" });
}

describe("the desktop dock", () => {
  it("labels every item under its icon: Home, New agent and Settings, then Stop (DEC-2293)", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "approvals");
    const names = (els: HTMLElement[]) => els.map((el) => el.querySelector("[data-slot=dock-label]")?.textContent);
    expect(within(dock()).getAllByRole("link").map((l) => l.getAttribute("href"))).toEqual(["/", "/new", "/settings"]);
    expect(names(within(dock()).getAllByRole("link"))).toEqual(["Home", "New", "Settings"]);
    expect(within(dock()).getAllByRole("button").filter((b) => b.getAttribute("data-slot") !== "stop-control")).toEqual([]);
    for (const label of dock().querySelectorAll("[data-slot=dock-label]")) {
      expect(label, "the current item is marked by a semibold label (DEC-478 item 4)").toHaveClass("group-data-current:font-semibold");
      expect(label.className, "so no label is semibold or bolder until it is current").not.toMatch(/(^|\s)font-(semibold|bold|extrabold|black)\b/);
    }
  });

  it("has no tooltips: the labels name every item", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    for (const item of [...within(dock()).getAllByRole("link"), ...within(dock()).getAllByRole("button")]) {
      fireEvent.mouseEnter(item);
      fireEvent.focus(item);
      expect(item).not.toHaveAttribute("aria-describedby");
      expect(item).not.toHaveAttribute("aria-keyshortcuts");
    }
    expect(document.querySelector(".kumo-tooltip-popup")).toBeNull();
  });

  it("marks the current screen, and only it, with aria-current and the pill", () => {
    setPathname("/settings");
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const current = within(dock()).getAllByRole("link").filter((l) => l.getAttribute("aria-current") === "page");
    expect(current.map((l) => l.getAttribute("href"))).toEqual(["/settings"]);
    expect(current[0]).toHaveAttribute("data-current");
    expect(dock().querySelectorAll("[data-current]")).toHaveLength(1);
    expect(current[0]).toHaveClass("data-current:bg-(--dock-current)", "hover:bg-(--dock-hover)");
  });

  it("keeps Home current inside an agent's thread", () => {
    setPathname(`/agents/${AGENT_IDS.btc}`);
    renderWithRuntime(<AppShell>{null}</AppShell>);
    expect(within(dock()).getByRole("link", { name: /^Home/ })).toHaveAttribute("aria-current", "page");
  });

  it("shows the open approvals count on Home", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "approvals");
    const count = within(dock()).getByRole("link", { name: /^Home/ }).querySelector("[data-slot=approvals-count]");
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
    expect(document.querySelector("nav[aria-label=Main].grid")?.parentElement).toHaveClass("lg:hidden");
  });

  it("gives way to the tab bar below lg, whose More keeps the account", () => {
    const restore = asPhone();
    try {
      setPathname(`/agents/${AGENT_IDS.btc}`);
      renderWithRuntime(<AppShell>{null}</AppShell>);
      fireEvent.click(within(screen.getByRole("navigation", { name: "Main" })).getByRole("button", { name: "More" }));
      expect(within(screen.getByRole("dialog", { name: "More" })).getByRole("region", { name: "Account and workspace" })).toHaveTextContent("Alpaca paper");
    } finally {
      restore();
    }
  });
});

describe("what the dock reaches", () => {
  it.each(ROLES.map((r) => r.id))("as %s, reaches every screen the role may open, and only those", (role) => {
    const reached = SCREENS.filter((s) => (DOCK_LINKS as readonly string[]).includes(s.key) && can(role, s.needs)).map((s) => s.href);
    for (const s of SCREENS) expect(reached.includes(s.href), s.href).toBe(s.built && can(role, s.needs));
    for (const href of reached) expect(canOpen(role, href), href).toBe(true);
  });

  it("matches the current screen by path", () => {
    expect(isCurrent("/", "/")).toBe(true);
    expect(isCurrent("/agents/agt_1", "/")).toBe(true);
    expect(isCurrent("/new", "/")).toBe(false);
    expect(isCurrent("/settings", "/settings")).toBe(true);
    expect(isCurrent("/connections/con_1/stop-all", "/settings")).toBe(true);
    expect(isCurrent("/newx", "/new")).toBe(false);
  });
});
