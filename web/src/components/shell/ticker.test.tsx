import { fireEvent, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { recordHref } from "@/components/stop/commands";
import { AGENT_IDS, APPROVAL_IDS, buildWorkspace } from "@/fixtures/workspace";
import { positionHref } from "@/lib/screens";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { AppShell } from "./app-shell";

beforeEach(() => setPathname("/"));

const ticker = () => document.querySelector<HTMLElement>("[data-slot=ticker]");
const strip = () => document.querySelector<HTMLElement>("[data-slot=status-strip]")!;

describe("the ticker tape", () => {
  it("shows each held instrument's symbol, last price and change today, with the change coloured, signed and worded", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const tape = screen.getByRole("region", { name: /^Held instruments/ });
    const quotes = within(tape).getAllByRole("listitem");
    expect(quotes.map((q) => q.textContent)).toEqual(["BTC/USD$56,789.01+1.07% up today", "XYZ$141.23−0.36% down today", "QRS$97.65−0.27% down today"]);
    const [btc, xyz] = quotes.map((q) => q.querySelector<HTMLElement>("[data-move]")!);
    expect(btc).toHaveClass("text-gain", "tabular");
    expect(xyz).toHaveClass("text-loss", "tabular");
    expect(quotes[0].firstElementChild).toHaveClass("font-semibold");
  });

  it("never states an amount won or lost", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    expect(ticker()?.textContent).not.toMatch(/[+−-]\$/);
    expect(ticker()?.textContent).not.toMatch(/P&L|profit|unrealized/i);
    expect(ticker()!.querySelector("[data-direction]"), "a price move, not a result the disclosure check looks for").toBeNull();
  });

  it("repeats the row once for a seamless loop, hidden from assistive technology and from focus", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const rows = ticker()!.querySelectorAll("[data-slot=ticker-track] > ul");
    expect(rows).toHaveLength(2);
    expect(rows[0]).not.toHaveAttribute("aria-hidden");
    expect(rows[1]).toHaveAttribute("aria-hidden", "true");
    expect(rows[1]).toHaveAttribute("inert");
    expect(rows[1].textContent).toBe(rows[0].textContent);
  });

  it("sits under the header as part of the frame, from sm, at the status strip's height; phones keep the strip", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    expect(ticker()).toHaveClass("glass", "sticky", "top-[calc(4rem+1px)]", "hidden", "sm:flex", "h-(--status-row)");
    expect(strip()).toHaveClass("h-(--status-row)");
    expect(strip().parentElement?.parentElement).toHaveClass("sm:hidden");
    expect(screen.getByRole("region", { name: /^Held instruments/ })).toHaveAttribute("tabindex", "0");
  });

  it("says how fresh the screen is by its oldest feed, and lists the four feeds", async () => {
    renderWithRuntime(<AppShell>{null}</AppShell>);
    const live = within(ticker()!).getByRole("button", { name: /^Live/ });
    expect(live).toHaveAccessibleName(/^Live: (under 5 s|\d+ s|\d+ min|\d+ h) since the oldest feed answered$/);
    expect(live).toHaveClass("w-36");
    fireEvent.click(live);
    const feeds = await screen.findByRole("dialog");
    expect(within(feeds).getByText("Feeds")).toBeInTheDocument();
    expect(within(feeds).getAllByRole("listitem").map((i) => i.getAttribute("data-feed"))).toEqual(["market", "deployment", "broker", "relay"]);
    expect(within(feeds).getByText("Push relay as of 14:05:10")).toHaveClass("text-foreground");
  });

  it.each(["stale", "unreachable", "loading", "empty"] as const)("gives way to the full status strip in the %s scenario", (scenario) => {
    renderWithRuntime(<AppShell>{null}</AppShell>, scenario);
    expect(ticker()).toBeNull();
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
    expect(ticker()).toBeNull();
    expect(strip()).toBeInTheDocument();
  });

  it("stays off for a role that cannot see agents", () => {
    renderWithRuntime(<AppShell>{null}</AppShell>, "normal", { role: "auditor" });
    expect(ticker()).toBeNull();
  });
});
