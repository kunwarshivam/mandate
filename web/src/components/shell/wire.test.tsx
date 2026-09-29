import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { Providers } from "@/components/providers";
import { recordHref } from "@/components/stop/commands";
import { AGENT_IDS, APPROVAL_IDS, buildWorkspace } from "@/fixtures/workspace";
import { positionHref } from "@/lib/screens";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { AppShell } from "./app-shell";

beforeEach(() => setPathname("/"));

const wire = () => document.querySelector<HTMLElement>("[data-slot=wire]");
const strip = () => document.querySelector<HTMLElement>("[data-slot=status-strip]")!;
const region = () => screen.getByRole("region", { name: "Agent activity" });

describe("the agent wire", () => {
  it("lists what the agents did, once, as links: the request waiting for you first, each with its time, its agent and its state in words", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const list = within(region()).getByRole("list");
    const items = within(list).getAllByRole("listitem");
    expect(items[0]).toHaveTextContent(/^14:04Agent 2 asked you to buy 2 XYZ at \$141\.30, Waiting for you$/);
    expect(items[0].querySelector("time")).toHaveClass("text-muted-foreground", "tabular");
    expect(within(items[0]).getByText("Agent 2")).toHaveClass("font-semibold");
    expect(within(items[0]).getByRole("link")).toHaveAttribute("href", `/approvals/${APPROVAL_IDS.swingXyz}`);
    expect(within(items[1]).getByRole("link")).toHaveAttribute("href", `/agents/${AGENT_IDS.btc}/decisions/01JBWPQ5E6EYCNDY0YP57RCYBV`);
    expect(items[1]).toHaveTextContent("Agent 1 blocked: orders are at most $1,000.00, Blocked");
    for (const item of items) {
      const state = item.querySelector("[data-slot=wire-state]")!;
      expect(state.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
      expect(state.textContent?.replace(", ", "")).toMatch(/^(Waiting for you|Blocked|Held|Waiting|Done|Paused|Stopped)$/);
    }
    expect(screen.getAllByRole("list", { hidden: false }).filter((l) => region().contains(l))).toHaveLength(1);
  });

  it("uses ink and muted text only: no gain or loss colour, no crimson, and no amount won or lost", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const html = wire()!.innerHTML;
    expect(html).not.toMatch(/text-(gain|loss)|crimson|data-direction/);
    expect(wire()!.textContent).not.toMatch(/[+−-]\$|P&L|profit|unrealized/i);
  });

  it("repeats the row once for a seamless loop, hidden from assistive technology and from focus", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const rows = wire()!.querySelectorAll("[data-slot=wire-track] > ul");
    expect(rows).toHaveLength(2);
    expect(rows[0]).not.toHaveAttribute("aria-hidden");
    expect(rows[1]).toHaveAttribute("aria-hidden", "true");
    expect(rows[1]).toHaveAttribute("inert");
    expect(rows[1].textContent).toBe(rows[0].textContent);
  });

  it("sits under the header as part of the frame, from sm, at the status strip's height; phones keep the strip", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    expect(wire()).toHaveClass("glass", "sticky", "top-[calc(4rem+1px)]", "hidden", "sm:flex", "h-(--status-row)");
    expect(strip()).toHaveClass("h-(--status-row)");
    expect(strip().parentElement?.parentElement).toHaveClass("sm:hidden");
  });

  it("says how fresh the screen is by its oldest feed, and lists the four feeds", async () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const live = within(wire()!).getByRole("button", { name: /^Live/ });
    expect(live).toHaveAccessibleName(/^Live: (under 5 s|\d+ s|\d+ min|\d+ h) since the oldest feed answered$/);
    expect(live).toHaveClass("w-36");
    fireEvent.click(live);
    const feeds = await screen.findByRole("dialog");
    expect(within(feeds).getByText("Feeds")).toBeInTheDocument();
    expect(within(feeds).getAllByRole("listitem").map((i) => i.getAttribute("data-feed"))).toEqual(["market", "deployment", "broker", "relay"]);
    expect(within(feeds).getByText("Push relay as of 14:05:10")).toHaveClass("text-foreground");
  });

  it("says so, beside the freshness, when no agent has done anything today", () => {
    const ws = buildWorkspace("normal");
    render(
      <Providers workspace={{ ...ws, approvals: [], decisions: [] }} tick={false}>
        <AppShell>{null}</AppShell>
      </Providers>,
    );
    expect(within(region()).getByText("No agent activity yet today")).toHaveClass("text-muted-foreground");
    expect(within(region()).queryByRole("list")).toBeNull();
    expect(within(wire()!).getByRole("button", { name: /^Live/ })).toBeInTheDocument();
  });

  it.each(["stale", "unreachable", "loading", "empty"] as const)("gives way to the full status strip in the %s scenario", (scenario) => {
    renderWithRuntime(<AppShell>{null}</AppShell>, scenario);
    expect(wire()).toBeNull();
    expect(strip().parentElement?.parentElement).not.toHaveClass("sm:hidden");
  });

  it.each([
    ["an approval request", `/approvals/${APPROVAL_IDS.swingXyz}`],
    ["the kill switch", recordHref("kill", AGENT_IDS.btc)],
    ["stop all", recordHref("stop_all", buildWorkspace("normal").connection.connection_id)],
    ["closing a position", `${positionHref(AGENT_IDS.swing, "asset_1")}/close`],
  ])("stays off %s, a frozen record, which shows the status strip", (_name, path) => {
    setPathname(path);
    renderWithRuntime(<AppShell>{null}</AppShell>);
    expect(wire()).toBeNull();
    expect(strip()).toBeInTheDocument();
  });

  it("stays off for a role that cannot see agents", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "normal", { role: "auditor" });
    expect(wire()).toBeNull();
  });
});
