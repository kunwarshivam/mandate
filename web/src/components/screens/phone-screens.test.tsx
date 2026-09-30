import type { ReactElement } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { Providers } from "@/components/providers";
import { AppShell } from "@/components/shell/app-shell";
import { AGENT_IDS, APPROVAL_IDS, buildWorkspace } from "@/fixtures/workspace";
import { agentLimits, headroomLine, nextLevel } from "@/lib/limits";
import { usd } from "@/lib/format";
import { MODE_MEANING } from "@/lib/labels";
import { allOrders } from "@/lib/orders";
import { agentHref } from "@/lib/screens";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { shownOnDesktop, shownOnPhone } from "@/test/viewport";
import { AgentDetailScreen, AgentSectionScreen } from "./agent-detail";
import { ApprovalRequestScreen } from "./approval-request";
import { ApprovalsInboxScreen } from "./approvals-inbox";
import { DashboardScreen } from "./dashboard";

/** The phone layouts (DEC-207), read from the breakpoint classes; `e2e/phone.spec.ts` checks the real layout. */

const main = () => screen.getByRole("main");
const onPhone = <T extends Element>(els: Iterable<T>) => [...els].filter(shownOnPhone);
const onDesktop = <T extends Element>(els: Iterable<T>) => [...els].filter(shownOnDesktop);

function home(scenario: Parameters<typeof renderWithRuntime>[1] = "normal") {
  setPathname("/");
  return renderWithRuntime(
    <AppShell>
      <DashboardScreen />
    </AppShell>,
    scenario,
  );
}

beforeEach(() => setPathname("/"));

describe("Home on a phone", () => {
  const needsYou = () => main().querySelector<HTMLElement>("[data-slot=needs-you]")!;

  it("opens with Needs you, then the account, the agents and recent activity, and no positions", () => {
    home();
    const order = onPhone(main().querySelectorAll("h2")).map((h) => h.textContent?.replace(/\d+ items?$/, "").trim());
    expect(order).toEqual(["Needs you", "Account equity", "Agents", "Recent activity"]);
    expect(onDesktop(main().querySelectorAll("h2")).map((h) => h.textContent?.replace(/\d+ requests?$/, "").trim())).toEqual(
      expect.arrayContaining(["Account equity", "Waiting for you", "Agents", "Recent activity", "Positions"]),
    );
    expect(shownOnDesktop(needsYou())).toBe(false);
  });

  it("lists the requests first, soonest deadline first, each with the static time it is skipped at, then the alerts", () => {
    const ws = buildWorkspace("approvals");
    const stale = buildWorkspace("stale");
    render(
      <Providers workspace={{ ...ws, health: stale.health, agents: stale.agents }} tick={false}>
        <AppShell>
          <DashboardScreen />
        </AppShell>
      </Providers>,
    );
    const rows = [...needsYou().querySelectorAll<HTMLElement>("li")];
    const kinds = rows.map((r) => r.dataset.kind);
    expect(kinds).toEqual(["request", "request", "request", "alert", "alert", "alert", "alert"]);
    const deadlines = rows.slice(0, 3).map((r) => Date.parse(r.querySelector("time")!.getAttribute("datetime")!));
    expect(deadlines).toEqual([...deadlines].sort((a, b) => a - b));
    for (const r of rows.slice(0, 3)) {
      expect(r).toHaveTextContent(/asks to buy .+ at a limit of \$[\d,.]+Skipped at \d\d:\d\d:\d\d [A-Z]+ if you do nothing$/);
      expect(r).not.toHaveTextContent(/min left|left\)/);
      expect(within(r).getByRole("link")).toHaveAttribute("href", expect.stringMatching(/^\/approvals\/apr_/));
    }
    expect(rows.slice(3).map((r) => r.textContent)).toEqual(["Market data stale", "Push relay down", "Agent 2: stale price (XYZ)", "Agent 2: stale price (QRS)"]);
    expect(within(rows[3]).getByRole("link")).toHaveAttribute("href", "/alerts");
    expect(within(rows[5]).getByRole("link")).toHaveAttribute("href", `/agents/${AGENT_IDS.swing}`);
    expect(needsYou().querySelector("h2")).toHaveTextContent("Needs you7 items");
  });

  it("shows a request once on Home: in Needs you, and nowhere else a phone sees", () => {
    home();
    const links = onPhone(document.querySelectorAll(`a[href="/approvals/${APPROVAL_IDS.swingXyz}"]`));
    expect(links).toHaveLength(1);
    expect(needsYou().contains(links[0])).toBe(true);
    const asks = /asks to buy|asked you to buy/;
    const innermost = [...document.body.querySelectorAll("*")].filter((el) => asks.test(el.textContent ?? "") && ![...el.children].some((c) => asks.test(c.textContent ?? "")));
    expect(onPhone(innermost)).toHaveLength(1);
    expect(main().querySelector("[data-slot=waiting-notice]")).toBeNull();
    expect(shownOnPhone(main().querySelector("[data-layout=rail]")!)).toBe(false);
  });

  it("says all clear, plainly, with a check, when nothing needs you", () => {
    const ws = buildWorkspace("normal");
    render(
      <Providers workspace={{ ...ws, approvals: [] }} tick={false}>
        <AppShell>
          <DashboardScreen />
        </AppShell>
      </Providers>,
    );
    const clear = needsYou().querySelector("[data-slot=all-clear]")!;
    expect(clear).toHaveTextContent(/^All clear\. Nothing needs you\.$/);
    expect(clear).toHaveClass("text-muted-foreground");
    expect(clear.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
    expect(needsYou().querySelector("ul")).toBeNull();
  });

  it("keeps the account's hero, on a shorter chart", () => {
    home();
    const canvas = main().querySelector<HTMLElement>("[data-slot=account-equity] [data-slot=chart-canvas]")!;
    expect(canvas).toHaveClass("h-(--chart-phone)", "lg:h-(--chart-height)");
    expect(canvas.style.getPropertyValue("--chart-phone")).toBe("180px");
    expect(canvas.style.getPropertyValue("--chart-height")).toBe("340px");
    const hero = main().querySelector<HTMLElement>("[data-slot=account-equity]")!;
    expect(shownOnPhone(hero.querySelector("[data-placeholder=performance]")!)).toBe(true);
    expect(shownOnPhone(hero.querySelector("[data-slot=range-picker]")!)).toBe(true);
    expect(hero).toHaveTextContent("Simulated funds on paper.");
    expect(shownOnPhone(hero.querySelector("[data-slot=chart-credit]")!)).toBe(true);
    expect(shownOnPhone(hero.querySelector("[data-slot=unmanaged]")!)).toBe(false);
  });

  it("gives each agent one row: its name, its state in a word, and its headroom, with no P&L and no disclosure", () => {
    home();
    const rows = onPhone(main().querySelectorAll<HTMLElement>("[data-slot=phone-agent]"));
    const ws = buildWorkspace("normal");
    expect(rows).toHaveLength(ws.agents.length);
    rows.forEach((row, i) => {
      const agent = ws.agents[i];
      expect(within(row).getByRole("link")).toHaveAttribute("href", `/agents/${agent.agent_id}`);
      expect(within(row).getByText(agent.label)).toHaveClass("font-semibold");
      expect(row.querySelector("[data-slot=mode-badge]")).toHaveTextContent(/\w/);
      expect(row.querySelector("[data-slot=mode-badge] svg")).not.toBeNull();
      expect(row.querySelector("[data-slot=headroom]")).toHaveTextContent(/^\$[\d,]+\.\d{2} (above|below) its [a-z0-9%\- ]+$/);
      expect(row.querySelector("[data-direction], [data-placeholder=performance]")).toBeNull();
      expect(row).not.toHaveTextContent(/P&L|profit|today|since deployed|[+−]\$/);
      expect(row.querySelector("button, [aria-expanded]")).toBeNull();
    });
    for (const card of main().querySelectorAll("[data-slot=agent-band]")) expect(shownOnPhone(card)).toBe(false);
  });

  it("keeps the full agent bands on desktop, with P&L and its disclosure, and no phone rows", () => {
    home();
    const bands = onDesktop(main().querySelectorAll<HTMLElement>("[data-slot=agent-band]"));
    expect(bands).toHaveLength(3);
    for (const band of bands) {
      expect(band).toHaveTextContent("Paper P&L, simulated");
      expect(band.querySelector("[data-placeholder=performance]")).not.toBeNull();
      expect(band.querySelector("[data-direction]")).not.toBeNull();
    }
    expect(onDesktop(main().querySelectorAll("[data-slot=phone-agent]"))).toEqual([]);
  });

  it("shows the last three entries of recent activity, then See all activity", () => {
    home();
    const activity = within(main()).getByRole("region", { name: "Recent activity" });
    expect(onPhone(activity.querySelectorAll("ol > li"))).toHaveLength(3);
    expect(onDesktop(activity.querySelectorAll("ol > li")).length).toBeGreaterThan(3);
    const all = onPhone(activity.querySelectorAll("a")).filter((a) => a.textContent === "See all activity");
    expect(all).toHaveLength(1);
    expect(all[0]).toHaveAttribute("href", "/audit/decisions");
    expect(shownOnDesktop(all[0])).toBe(false);
  });

  it("leaves positions off a phone's Home", () => {
    home();
    const positions = within(main()).getByRole("region", { name: "Positions" });
    expect(shownOnPhone(positions)).toBe(false);
    expect(shownOnDesktop(positions)).toBe(true);
  });
});

function agentPage(path: string, ui: ReactElement, scenario: Parameters<typeof renderWithRuntime>[1] = "normal") {
  setPathname(path);
  return renderWithRuntime(<AppShell>{ui}</AppShell>, scenario);
}

const SWING = buildWorkspace("normal").agents.find((a) => a.agent_id === AGENT_IDS.swing)!;
const overview = (scenario?: Parameters<typeof renderWithRuntime>[1]) => agentPage(agentHref(AGENT_IDS.swing, "overview"), <AgentDetailScreen agentId={AGENT_IDS.swing} />, scenario);

describe("an agent on a phone", () => {
  const header = () => main().querySelector<HTMLElement>("[data-slot=page-header]")!;
  const headroom = () => main().querySelector<HTMLElement>("[data-slot=headroom]")!;

  it("opens with the name, its state and Stop this agent, then what waits, the equity, the headroom and the links", () => {
    overview();
    const title = within(header()).getByRole("heading", { level: 1 });
    expect(title).toHaveTextContent(SWING.label);
    const badge = header().querySelector<HTMLElement>("[data-slot=mode-badge]")!;
    expect(title.parentElement!.contains(badge)).toBe(true);
    expect(shownOnPhone(badge)).toBe(true);
    expect(shownOnDesktop(badge)).toBe(false);
    expect(badge.querySelector("svg")).not.toBeNull();
    expect(onPhone(within(header()).getAllByRole("button", { name: "Stop this agent…" }))).toHaveLength(1);
    const order = onPhone(main().querySelectorAll("h2")).map((h) => h.textContent);
    expect(order).toEqual(["Waiting for you", "Equity against your mandate", "Headroom", "This agent"]);
  });

  it("moves key figures, positions, orders, decisions, activity and the mandate card off the phone, and keeps them all on desktop", () => {
    overview();
    const moved = ["Key figures", "Positions", "Working orders", "Recent decisions", "Activity", "Your mandate"];
    const regions = moved.map((name) => within(main()).getByRole("region", { name }));
    for (const r of regions) {
      expect(shownOnPhone(r)).toBe(false);
      expect(shownOnDesktop(r)).toBe(true);
    }
    expect(onDesktop(main().querySelectorAll("h2")).map((h) => h.textContent)).toEqual(
      expect.arrayContaining(["Equity against your mandate", "Your mandate", "Key figures", "Waiting for you", "Positions", "Working orders", "Recent decisions", "Activity"]),
    );
    for (const slot of ["headroom", "phone-waiting", "agent-links", "levels-toggle"]) expect(onDesktop(main().querySelectorAll(`[data-slot=${slot}]`))).toEqual([]);
  });

  it("keeps the route tabs on desktop only", () => {
    overview();
    const tabs = within(header()).getByRole("navigation", { name: "Agent sections" });
    expect(shownOnPhone(tabs)).toBe(false);
    expect(shownOnDesktop(tabs)).toBe(true);
  });

  it("lists a waiting request once, with the static time it is skipped at", () => {
    overview();
    const waiting = main().querySelector<HTMLElement>("[data-slot=phone-waiting]")!;
    const rows = waiting.querySelectorAll("li");
    expect(rows.length).toBeGreaterThan(0);
    for (const r of rows) {
      expect(r).toHaveTextContent(/^Buy .+ at a limit of \$[\d,.]+Skipped at \d\d:\d\d:\d\d [A-Z]+ if you do nothing$/);
      expect(within(r).getByRole("link")).toHaveAttribute("href", expect.stringMatching(/^\/approvals\/apr_/));
    }
    const id = within(rows[0]).getByRole("link").getAttribute("href")!;
    expect(onPhone(document.querySelectorAll(`a[href="${id}"]`))).toHaveLength(1);
  });

  it("keeps the hero's figure, disclosure and chart, and folds the level legend behind Levels", () => {
    overview();
    const hero = main().querySelector<HTMLElement>("[data-slot=agent-equity]")!;
    expect(shownOnPhone(hero.querySelector("[data-slot=agent-equity-value]")!)).toBe(true);
    expect(shownOnPhone(hero.querySelector("[data-placeholder=performance]")!)).toBe(true);
    expect(shownOnPhone(hero.querySelector("[data-slot=chart-canvas]")!)).toBe(true);
    const toggle = within(hero).getByRole("button", { name: "Levels" });
    const legend = hero.querySelector<HTMLElement>("[data-slot=level-legend]")!;
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(toggle).toHaveAttribute("aria-controls", legend.parentElement!.id);
    expect(toggle).toHaveClass("min-h-11", "lg:hidden");
    expect(shownOnPhone(legend)).toBe(false);
    expect(shownOnDesktop(legend)).toBe(true);
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(shownOnPhone(legend)).toBe(true);
    fireEvent.click(toggle);
    expect(shownOnPhone(legend)).toBe(false);
  });

  it("gives each limit a Headroom row: the room left, the limit, and a thin meter in ink with the marker, no gain or loss colour", () => {
    overview();
    const limits = agentLimits(SWING);
    const rows = [...headroom().querySelectorAll<HTMLElement>("[data-slot=headroom-row]")];
    expect(rows).toHaveLength(limits.rails.length + 1);
    for (const row of rows) {
      expect(row).toHaveTextContent(/(\$[\d,]+\.\d{2} headroom|\d+ left)/);
      expect(row).toHaveTextContent(/Limit [$\d]/);
      const meter = row.querySelector<HTMLElement>("[data-slot=headroom-meter]")!;
      expect(meter).toHaveAttribute("role", "img");
      expect(meter).toHaveAccessibleName(/\w/);
      expect(meter.querySelector(".bg-foreground")).not.toBeNull();
      expect(meter.querySelector(".bg-mandate-strong")).not.toBeNull();
    }
    expect(headroom().querySelector("[data-direction], [data-placeholder=performance]")).toBeNull();
    expect(headroom().innerHTML).not.toMatch(/\b(text|bg|ring|border)-(gain|loss|crimson)/);
    expect(headroom()).not.toHaveTextContent(/P&L|profit|gain|[+−]\$/i);
    const daily = rows.find((r) => r.textContent?.startsWith("Daily loss limit"))!;
    expect(daily).toHaveTextContent(`${headroomLine(SWING).split(" ")[0]} headroom`);
    expect(within(headroom()).getByRole("link", { name: "View full mandate" })).toHaveAttribute("href", agentHref(AGENT_IDS.swing, "mandate"));
  });

  it("says when the broker is being checked, in the headroom, as the mandate card does", () => {
    const agent = buildWorkspace("reconciliation").agents.find((a) => a.startup === "reconciling")!;
    agentPage(agentHref(agent.agent_id, "overview"), <AgentDetailScreen agentId={agent.agent_id} />, "reconciliation");
    expect(headroom().querySelector("[data-slot=reconciling]")).toHaveTextContent("Checking with the broker.");
    expect(headroom()).toHaveTextContent(MODE_MEANING[agent.mode]);
  });

  it("replaces the tabs with a plain list of every section, counting positions and orders, the current one marked", () => {
    overview();
    const links = main().querySelector<HTMLElement>("[data-slot=agent-links]")!;
    expect(shownOnPhone(links)).toBe(true);
    expect(shownOnDesktop(links)).toBe(false);
    const items = within(links).getAllByRole("link");
    expect(items.map((a) => a.textContent)).toEqual([
      "Overview",
      `Positions${SWING.positions.length}`,
      `Orders${allOrders(SWING).length}`,
      "Decisions",
      "Approvals",
      "Mandate",
      "Prove",
      "Activity",
    ]);
    expect(items.map((a) => a.getAttribute("href"))).toEqual(
      ["overview", "positions", "orders", "decisions", "approvals", "mandate", "prove", "activity"].map((k) => agentHref(AGENT_IDS.swing, k as Parameters<typeof agentHref>[1])),
    );
    expect(items.filter((a) => a.getAttribute("aria-current") === "page").map((a) => a.textContent)).toEqual(["Overview"]);
    for (const a of items) expect(a).toHaveClass("min-h-12");
  });

  it.each([
    ["positions", "Positions"],
    ["mandate/versions", "Mandate"],
  ] as const)("marks %s's section in the list at the foot of the screen", (key, label) => {
    agentPage(agentHref(AGENT_IDS.swing, key), <AgentSectionScreen agentId={AGENT_IDS.swing} section={key} />);
    const links = main().querySelector<HTMLElement>("[data-slot=agent-links]")!;
    const current = within(links).getAllByRole("link").filter((a) => a.getAttribute("aria-current") === "page");
    expect(current.map((a) => a.textContent?.replace(/\d+$/, ""))).toEqual([label]);
    expect(main().lastElementChild?.lastElementChild).toBe(links);
  });
});

describe("approvals on a phone", () => {
  const inbox = () => agentPage("/approvals", <ApprovalsInboxScreen />, "approvals");

  it("lists each request as one hairline row with its static time, and keeps the tinted rows and the minutes on desktop", () => {
    inbox();
    const open = within(main()).getByRole("region", { name: "Open, by deadline" });
    const links = within(open).getAllByRole("link");
    expect(links.length).toBe(buildWorkspace("approvals").approvals.filter((a) => a.status === "delivered").length);
    for (const link of links) {
      expect(link.parentElement).toHaveClass("max-lg:border-b");
      expect(link).toHaveClass("bg-lapis-soft", "max-lg:bg-transparent", "max-lg:rounded-xl");
      const remaining = link.querySelector<HTMLElement>("[data-slot=remaining]")!;
      expect(remaining).toHaveTextContent(/min left/);
      expect(shownOnPhone(remaining)).toBe(false);
      expect(shownOnDesktop(remaining)).toBe(true);
      expect(shownOnPhone(link.querySelector("time")!)).toBe(true);
      expect(onPhone(link.querySelectorAll("svg")).length).toBeGreaterThan(0);
    }
  });

  it("gives a request one screen, with Approve and Skip equal and pinned above the tab bar", () => {
    agentPage(`/approvals/${APPROVAL_IDS.swingXyz}`, <ApprovalRequestScreen approvalId={APPROVAL_IDS.swingXyz} />);
    const bar = within(main()).getByRole("region", { name: "Your response" });
    expect(bar).toHaveClass("sticky", "bottom-[calc(var(--tab-bar)+1px+env(safe-area-inset-bottom))]", "lg:bottom-0");
    const approve = within(bar).getByRole("button", { name: "Approve" });
    const skip = within(bar).getByRole("button", { name: "Skip" });
    expect(approve.className).toBe(skip.className);
    expect(approve.dataset.variant).toBe(skip.dataset.variant);
    expect(approve.parentElement).toBe(skip.parentElement);
    expect(approve.parentElement).toHaveClass("grid-cols-2");
    expect(document.activeElement).not.toBe(approve);
    expect(document.activeElement).not.toBe(skip);
    expect(bar.querySelector("[data-slot=deadline] time")).not.toBeNull();
    expect(main().querySelectorAll("article")).toHaveLength(1);
  });
});

describe("headroom in one line", () => {
  it.each(Object.values(AGENT_IDS))("for %s, reads the distance to the next level from the mandate panel's helper", (id) => {
    const agent = buildWorkspace("normal").agents.find((a) => a.agent_id === id)!;
    const next = nextLevel(agentLimits(agent));
    expect(next).not.toBeNull();
    expect(headroomLine(agent)).toMatch(new RegExp(`^\\${usd(next!.distance)} (above|below) its `));
  });

  it("names the daily loss limit by its words", () => {
    const agent = buildWorkspace("normal").agents.find((a) => a.agent_id === AGENT_IDS.btc)!;
    const next = nextLevel(agentLimits(agent))!;
    if (next.level.kind === "daily") expect(headroomLine(agent)).toMatch(/above its daily loss limit$/);
  });
});
