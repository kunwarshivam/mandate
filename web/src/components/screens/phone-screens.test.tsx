import { render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { Providers } from "@/components/providers";
import { AppShell } from "@/components/shell/app-shell";
import { AGENT_IDS, APPROVAL_IDS, buildWorkspace } from "@/fixtures/workspace";
import { agentLimits, headroomLine, nextLevel } from "@/lib/limits";
import { usd } from "@/lib/format";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { shownOnDesktop, shownOnPhone } from "@/test/viewport";
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
    expect(canvas.style.getPropertyValue("--chart-height")).toBe("260px");
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
