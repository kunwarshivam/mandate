import { type ReactElement, useEffect } from "react";
import { act, cleanup, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as agentRoute from "@/app/(app)/agents/[agentId]/page";
import * as approvalRoute from "@/app/(app)/approvals/[approvalId]/page";
import { DECISION_KEY } from "@/components/kumo/key";
import { AppShell } from "@/components/shell/app-shell";
import type { Workspace } from "@/fixtures/types";
import { AGENT_IDS, APPROVAL_IDS, SCENARIOS, buildWorkspace, findApproval } from "@/fixtures/workspace";
import { clock, price } from "@/lib/format";
import { PURPOSE_LABEL } from "@/lib/labels";
import { headroomLine } from "@/lib/limits";
import { decisionHref } from "@/lib/screens";
import { approvalAt, useRuntime } from "@/lib/mock-runtime";
import { RECORD_AFTER_MS, dockStop, isDisabled, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { AgentDetailScreen, AgentSectionScreen } from "./agent-detail";
import { AlertsScreen } from "./account-screens";
import { AgentsListScreen } from "./agents-list";
import { ApprovalRequestScreen } from "./approval-request";
import { ApprovalsInboxScreen, askSentence } from "./approvals-inbox";
import { DashboardScreen } from "./dashboard";

const SCENARIO_IDS = SCENARIOS.map((s) => s.id);
const PERSUASIVE = /guarantee|safer|safe bet|recommend|profit (estimate|target)|price target|expected (return|profit)|don.t miss|act now|hurry|opportunity/i;

const SCREENS: Array<[string, () => ReactElement]> = [
  ["/", () => <DashboardScreen />],
  ["/agents", () => <AgentsListScreen />],
  [`/agents/${AGENT_IDS.btc}`, () => <AgentDetailScreen agentId={AGENT_IDS.btc} />],
  [`/agents/${AGENT_IDS.swing}`, () => <AgentDetailScreen agentId={AGENT_IDS.swing} />],
  [`/agents/${AGENT_IDS.lmn}`, () => <AgentDetailScreen agentId={AGENT_IDS.lmn} />],
  [`/agents/${AGENT_IDS.btc}/mandate/versions`, () => <AgentSectionScreen agentId={AGENT_IDS.btc} section="mandate/versions" />],
  [`/agents/${AGENT_IDS.swing}/mandate/edit`, () => <AgentSectionScreen agentId={AGENT_IDS.swing} section="mandate/edit" />],
  ["/approvals", () => <ApprovalsInboxScreen />],
  [`/approvals/${APPROVAL_IDS.swingXyz}`, () => <ApprovalRequestScreen approvalId={APPROVAL_IDS.swingXyz} />],
  [`/approvals/${APPROVAL_IDS.btc}`, () => <ApprovalRequestScreen approvalId={APPROVAL_IDS.btc} />],
  [`/approvals/${APPROVAL_IDS.lmn}`, () => <ApprovalRequestScreen approvalId={APPROVAL_IDS.lmn} />],
];

function renderScreen(path: string, ui: ReactElement, scenario: (typeof SCENARIO_IDS)[number] = "normal") {
  setPathname(path);
  return renderWithRuntime(<AppShell>{ui}</AppShell>, scenario);
}

const main = () => screen.getByRole("main");

beforeEach(() => setPathname("/"));

describe("every screen in every scenario", () => {
  it.each(SCREENS.flatMap(([path, ui]) => SCENARIO_IDS.map((scenario) => [path, scenario, ui] as const)))("%s in %s", (path, scenario, ui) => {
    renderScreen(path, ui(), scenario);
    const [header] = screen.getAllByRole("banner");
    expect(within(header).getByText("PAPER")).toBeInTheDocument();
    expect(isDisabled(dockStop())).toBe(false);
    expect(main()).not.toHaveTextContent(PERSUASIVE);

    if (scenario === "loading") {
      expect(main().querySelector("[aria-busy=true]")).not.toBeNull();
      expect(main()).not.toHaveTextContent(/\$\d/);
    } else if (scenario === "unreachable") {
      expect(main().querySelector("[data-slot=unreachable-notice]")).not.toBeNull();
      expect(main()).not.toHaveTextContent(/Agent \d|\$\d/);
    }
    for (const el of main().querySelectorAll("[data-slot=approval-choices] button")) expect(isDisabled(el)).toBe(false);
  });
});

describe("D1 dashboard", () => {
  it("shows one sentence and one action when there are no agents", () => {
    renderScreen("/", <DashboardScreen />, "empty");
    expect(within(main()).getByRole("heading", { name: "No agents yet" })).toBeInTheDocument();
    expect(within(main()).getByRole("link", { name: "Describe your first agent" })).toHaveAttribute("href", "/agents/new");
  });

  it("puts the performance placeholder beside every P&L, and says paper once for the list", () => {
    renderScreen("/", <DashboardScreen />);
    const agents = within(main()).getByRole("region", { name: "Agents" });
    const cards = within(agents).getAllByRole("article");
    expect(cards).toHaveLength(3);
    expect(agents.querySelectorAll("[data-slot=paper-note]")).toHaveLength(1);
    expect(agents.querySelector("[data-slot=paper-note]")).toHaveTextContent("Paper P&L, simulated.");
    for (const card of cards) {
      expect(card).not.toHaveTextContent(/simulated/i);
      expect(card.querySelector("[data-placeholder=performance]")).toHaveTextContent("[[DISCLOSURE-PERFORMANCE]]");
      expect(within(card).getByRole("button", { name: "Performance disclosure" })).toHaveAccessibleDescription("[[DISCLOSURE-PERFORMANCE]]");
    }
  });

  it("gives each agent row its headroom to the next level", () => {
    renderScreen("/", <DashboardScreen />);
    const ws = buildWorkspace("normal");
    const cards = within(within(main()).getByRole("region", { name: "Agents" })).getAllByRole("article");
    cards.forEach((card, i) => expect(card.querySelector("[data-slot=card-headroom]")).toHaveTextContent(headroomLine(ws.agents[i])));
  });

  it("leads the main column with the money, the account then the agents, and keeps what needs you and the latest decisions in the rail", () => {
    renderScreen("/", <DashboardScreen />);
    const column = main().querySelector<HTMLElement>("[data-layout=main]")!;
    const rail = main().querySelector<HTMLElement>("[data-layout=rail]")!;
    expect(main().querySelectorAll("[data-layout=rail]")).toHaveLength(1);
    expect([...column.querySelectorAll("h2")].slice(0, 2).map((h) => h.textContent)).toEqual(["Account equity", "Agents"]);
    expect([...rail.querySelectorAll("h2")].map((h) => h.textContent?.replace(/\d+ items?$/, ""))).toEqual(["Needs you", "Decisions"]);
    expect(rail.compareDocumentPosition(column) & Node.DOCUMENT_POSITION_FOLLOWING, "a phone reads what needs you before the money").toBeTruthy();
    expect(rail.parentElement?.className).toContain("lg:grid-cols-[minmax(0,1fr)_20rem]");
  });

  it("draws the latest decisions as a timeline: the agent's owl, the action, the verdict in words, who and when, and the rule or what came next", () => {
    renderScreen("/", <DashboardScreen />);
    const ws = buildWorkspace("normal");
    const rail = main().querySelector<HTMLElement>("[data-layout=rail]")!;
    const region = rail.querySelector<HTMLElement>("section[aria-labelledby=rail-decisions-title]")!;
    const entries = [...region.querySelectorAll<HTMLElement>("[data-slot=timeline-entry]")];
    const openRequest = (id: string | undefined) => !!id && approvalAt(findApproval(ws, id)!, ws.now).status === "delivered";
    const shown = ws.decisions.filter((d) => !openRequest(d.approval_id));
    expect(entries).toHaveLength(Math.min(4, shown.length));
    entries.forEach((entry, i) => {
      const d = shown[i];
      expect(entry).toHaveAttribute("data-verdict", d.verdict);
      expect(entry.querySelector("svg[data-slot=owl]")).not.toBeNull();
      expect(within(entry).getByRole("link")).toHaveAttribute("href", decisionHref(d.agent_id, d.event_id));
      expect(entry.querySelector("[data-slot=verdict]")).toHaveTextContent(/^(Allowed|Asked you|Not allowed|Held|Waiting)$/);
      if (d.verdict === "allow" && d.approval_id) expect(entry.querySelector("[data-slot=verdict]")).toHaveTextContent("Asked you");
      expect(entry.querySelector("time")).toHaveTextContent(clock(d.at).slice(0, 5));
      if (d.verdict === "allow") expect(entry.querySelector("[data-slot=gate-rule]")).toBeNull();
      else expect(entry.querySelector("[data-slot=gate-rule]")).toHaveTextContent(/\w/);
      expect(entry.innerHTML).not.toMatch(/\b(text|bg|ring|border)-(gain|loss|crimson)\b/);
    });
    expect(region.querySelector("[data-slot=decision-tally]")).toHaveTextContent(/^The latest 4 decisions: \d+ allowed(, \d+ (asked you|not allowed|held|waiting))+\.$/);
    expect(within(region).getByRole("link", { name: /All decisions/ })).toHaveAttribute("href", "/audit/decisions");
  });

  describe("a request appears once on Home (DESIGN.md, Needs you; critique C-5)", () => {
    const ws = buildWorkspace("normal");
    const asked = ws.decisions.find((d) => d.approval_id === APPROVAL_IDS.swingXyz)!;
    const askedHref = decisionHref(asked.agent_id, asked.event_id);
    const others = ws.decisions.filter((d) => d.event_id !== asked.event_id);
    const decisionRegions = () => [...main().querySelectorAll<HTMLElement>("section[aria-labelledby$=decisions-title]")];
    const shownHrefs = (region: HTMLElement) => within(region).getAllByRole("listitem").map((li) => within(li).getByRole("link").getAttribute("href"));
    const closed = (status: "acted" | "expired") => (w: Workspace) => ({
      ...w,
      approvals: w.approvals.map((a) => (a.approval_id === APPROVAL_IDS.swingXyz ? { ...a, status } : a)),
    });

    it("shows an open request once, under Needs you, and leaves its Asked you row out of the Decisions rail", () => {
      renderScreen("/", <DashboardScreen />);
      expect(findApproval(ws, APPROVAL_IDS.swingXyz)?.status, "the request is open").toBe("delivered");
      const links = within(main()).getAllByRole("link", { name: /buy 2 XYZ at/i });
      expect(links).toHaveLength(1);
      expect(links[0]).toHaveAttribute("href", `/approvals/${APPROVAL_IDS.swingXyz}`);
      expect(within(main()).getByRole("region", { name: /^Needs you/ }).contains(links[0])).toBe(true);
      const regions = decisionRegions();
      expect(regions).toHaveLength(2);
      const [rail, phone] = regions;
      expect(shownHrefs(rail)).toEqual(others.slice(0, 4).map((d) => decisionHref(d.agent_id, d.event_id)));
      expect(shownHrefs(phone)).toEqual(others.slice(0, 3).map((d) => decisionHref(d.agent_id, d.event_id)));
      for (const r of regions) expect(r.querySelector(`a[href="${askedHref}"]`)).toBeNull();
    });

    it.each(["acted", "expired"] as const)("shows the request's decision row in the rail again once it is %s", (status) => {
      renderWithRuntime(<AppShell>{<DashboardScreen />}</AppShell>, "normal", { workspace: closed(status) });
      expect(within(within(main()).getByRole("region", { name: /^Needs you/ })).queryByRole("link", { name: /buy 2 XYZ at/i })).toBeNull();
      for (const region of decisionRegions()) {
        const row = within(region).getByRole("link", { name: "Buy 2 XYZ at $141.30" });
        expect(row).toHaveAttribute("href", askedHref);
        expect(row.closest("li")?.querySelector("[data-slot=verdict]")).toHaveTextContent("Asked you");
      }
      const [rail, phone] = decisionRegions();
      expect(shownHrefs(rail)).toEqual(ws.decisions.slice(0, 4).map((d) => decisionHref(d.agent_id, d.event_id)));
      expect(shownHrefs(phone)).toEqual(ws.decisions.slice(0, 3).map((d) => decisionHref(d.agent_id, d.event_id)));
    });

    it("shows the row again once the owner answers the request", () => {
      vi.useFakeTimers();
      try {
        const view = renderScreen(`/approvals/${APPROVAL_IDS.swingXyz}`, <ApprovalRequestScreen approvalId={APPROVAL_IDS.swingXyz} />);
        fireEvent.click(within(main()).getByRole("button", { name: "Approve" }));
        act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 3));
        setPathname("/");
        view.rerender(<AppShell>{<DashboardScreen />}</AppShell>);
        expect(within(within(main()).getByRole("region", { name: /^Needs you/ })).queryByRole("link", { name: /buy 2 XYZ at/i })).toBeNull();
        for (const region of decisionRegions()) expect(region.querySelector(`a[href="${askedHref}"]`)).not.toBeNull();
      } finally {
        vi.useRealTimers();
      }
    });
  });

  it("keeps news off Home: it is read on each agent's page, beside its decisions", () => {
    renderScreen("/", <DashboardScreen />);
    expect(within(main()).queryByRole("region", { name: "News" })).toBeNull();
  });

  it("states a gain or loss in words as well as colour", () => {
    renderScreen("/", <DashboardScreen />, "drawdown");
    const loss = main().querySelector("[data-direction=loss]");
    expect(loss).toHaveTextContent(/−\$\d.*loss/);
  });

  it("names the stale age where values depend on stale marks", () => {
    renderScreen("/", <DashboardScreen />, "stale");
    expect(main()).toHaveTextContent(/Stale: as of 14:02:11, 3 min ago/);
  });

  it("names a paused agent's restriction on its row in one line, since when, with what it blocks and how it ends behind it, not inline (DEC-512)", () => {
    renderScreen("/", <DashboardScreen />, "paused");
    const restrictions = within(main()).getByLabelText("Restrictions");
    expect(restrictions.tagName).toBe("P");
    expect(restrictions).toHaveTextContent(/since \d\d:\d\d/);
    expect(restrictions).not.toHaveTextContent(/Blocks/);
    expect(restrictions.querySelector("[title]")?.getAttribute("title")).toMatch(/^Blocks .+\. Ends when: .+\.$/);
  });
});

describe("D2 agent detail", () => {
  const rails = (el: Element) => [...el.querySelectorAll("[data-slot=limit-rail]")];

  it("keeps only a compact mandate beside the story: mode, rails in dollars, the next level, and a way to the rest", () => {
    renderScreen(`/agents/${AGENT_IDS.swing}`, <AgentDetailScreen agentId={AGENT_IDS.swing} />);
    const rail = main().querySelector("[data-layout=rail]") as HTMLElement;
    const card = within(rail).getByRole("region", { name: "Your mandate" });
    expect(card).toHaveAttribute("data-slot", "mandate-card");
    expect([...rail.children]).toEqual([card]);
    expect(card.querySelector("[data-slot=mode-badge]")).not.toBeNull();
    expect(rails(card).length).toBeGreaterThan(0);
    for (const r of rails(card)) expect(r).toHaveTextContent(/\$[\d,]+\.\d{2} of \$[\d,]+\.\d{2}/);
    expect(rails(card).some((r) => /profit/i.test(r.textContent ?? ""))).toBe(false);
    expect(card.querySelector("[data-slot=next-level]")).toHaveTextContent(/Next level.*\$[\d,]+\.\d{2}.*below equity now/);
    expect(card.querySelector("[role=progressbar]")).toBeNull();
    expect(within(card).getByRole("link", { name: "View full mandate" })).toHaveAttribute("href", `/agents/${AGENT_IDS.swing}/mandate`);
    expect(main().querySelector("[data-slot=mandate-fields]")).toBeNull();
  });

  it("labels what the next level does 'At the limit:', so it never reads as the mode the chip shows (C-7)", () => {
    for (const agentId of Object.values(AGENT_IDS)) {
      cleanup();
      renderScreen(`/agents/${agentId}`, <AgentDetailScreen agentId={agentId} />);
      const card = within(main()).getByRole("region", { name: "Your mandate" });
      const next = card.querySelector("[data-slot=next-level]");
      if (!next) continue;
      const lines = [...next.querySelectorAll("p")].map((p) => p.textContent ?? "");
      const said = lines.find((line) => /equity now\./.test(line));
      expect(said, `${agentId}: ${lines.join(" | ")}`).toMatch(/equity now\. At the limit: [a-z][^.]*\.$/);
      expect(within(card).getByRole("group", { name: "Mode" })).toBeVisible();
    }
    cleanup();
    renderScreen(`/agents/${AGENT_IDS.btc}`, <AgentDetailScreen agentId={AGENT_IDS.btc} />);
    const card = within(main()).getByRole("region", { name: "Your mandate" });
    const daily = card.querySelector("[data-slot=next-level][data-level=daily]");
    expect(daily).not.toBeNull();
    expect(daily).toHaveTextContent(/below equity now\. At the limit: selling only until a new risk day\./);
    expect(daily).not.toHaveTextContent(/now\. Selling only/);
  });

  it("tells the story in the main column, with a recent slice of activity and a link to all of it", () => {
    renderScreen(`/agents/${AGENT_IDS.btc}`, <AgentDetailScreen agentId={AGENT_IDS.btc} />);
    const story = [...main().querySelectorAll("[data-layout=main]")];
    const headings = story.flatMap((col) => [...col.querySelectorAll("h2")].map((h) => h.textContent));
    expect(headings).toEqual(expect.arrayContaining(["Equity against your mandate", "Key figures", "Positions", "Working orders", "Recent decisions", "Activity", "News"]));
    const activity = within(main()).getByRole("region", { name: "Activity" });
    expect(story.some((col) => col.contains(activity))).toBe(true);
    expect(activity.querySelectorAll("li").length).toBeLessThanOrEqual(5);
    expect(within(activity).getByRole("link", { name: "View all activity" })).toHaveAttribute("href", `/agents/${AGENT_IDS.btc}/activity`);
  });

  it("puts the levels, every rail and every field on the Mandate tab, in two columns on a wide screen", () => {
    renderScreen(`/agents/${AGENT_IDS.swing}/mandate`, <AgentSectionScreen agentId={AGENT_IDS.swing} section="mandate" />);
    const envelope = within(main()).getByRole("region", { name: "Your mandate" });
    expect(envelope).toHaveAttribute("data-slot", "envelope");
    expect(envelope).toHaveTextContent("Limits in dollars");
    expect(rails(envelope).length).toBeGreaterThan(0);
    expect(rails(envelope).some((r) => /profit/i.test(r.textContent ?? ""))).toBe(false);
    expect(envelope.querySelector("[data-level=profit_stop]")).toHaveTextContent("Profit stop");
    expect(envelope.querySelector("[role=progressbar]")).toBeNull();
    const fields = main().querySelector("[data-slot=mandate-fields]") as HTMLElement;
    expect(fields.className).toMatch(/\bgrid-cols-1\b.*\blg:grid-cols-2\b/);
    expect(fields.querySelectorAll("[data-slot=mandate-field]").length).toBeGreaterThan(10);
    expect(fields.querySelector("[data-slot=provenance-badge], [data-provenance]")).not.toBeNull();
  });

  describe("A6 versions", () => {
    const versions = (agentId: string, scenario: (typeof SCENARIO_IDS)[number] = "normal") => {
      renderScreen(`/agents/${agentId}/mandate/versions`, <AgentSectionScreen agentId={agentId} section="mandate/versions" />, scenario);
      return [...main().querySelectorAll<HTMLElement>("[data-slot=version]")];
    };
    const changes = (version: HTMLElement) => [...version.querySelectorAll("[data-slot=version-change]")].map((c) => c.textContent);

    it("lists every version newest first, marks the one in effect, and diffs a reducing one that applied on confirmation", () => {
      const [second, first] = versions(AGENT_IDS.lmn);
      expect(main().querySelector("[data-slot=coming-soon]")).toBeNull();
      expect(within(second).getByRole("heading", { level: 3 })).toHaveTextContent("Version 2");
      expect(second).toHaveAttribute("data-current", "true");
      expect(first).not.toHaveAttribute("data-current");
      expect(second).toHaveTextContent("In effect");
      expect(second).toHaveTextContent("Confirmed by you on Sep 28, 2026 at 09:05:00 ET.");
      expect(second).not.toHaveTextContent("passkey");
      expect(changes(second)).toEqual(["Two approvers aboveNot setto$400.00Risk-reducing", "Largest position$3,000.00to$2,500.00Risk-reducing"]);
      expect(second).toHaveTextContent("Applied when you confirmed it. 1 request waiting for you was canceled; anything still wanted is proposed again under this version.");
      expect(first).toHaveTextContent("Confirmed by you with your passkey on Sep 23, 2026 at 09:18:30 ET.");
      expect(first).toHaveTextContent("Deployed to paper on Sep 23, 2026 at 09:30:00 ET.");
      expect(first.querySelector("[data-slot=version-changes]")).toBeNull();
    });

    it("classifies each path, and says a risk-increasing version took a passkey and applied at the next safe point", () => {
      const [second] = versions(AGENT_IDS.swing);
      const tags = [...second.querySelectorAll("[data-slot=change-class]")].map((t) => t.textContent);
      expect(tags).toEqual(["Risk-increasing", "Neutral", "Risk-increasing"]);
      expect(changes(second)).toEqual(["Quiet hours start22:30to23:00Neutral", "Total holdings limit$1,500.00to$2,000.00Risk-increasing"]);
      expect(second).toHaveTextContent("Confirmed by you with your passkey on Sep 25, 2026 at 08:15:00 ET.");
      expect(second).toHaveTextContent("Applied at the next safe point, Sep 25, 2026 at 08:15:04 ET. No requests were waiting for you.");
      expect(main()).toHaveTextContent("A risk-increasing version takes your passkey and applies at the next safe point");
    });

    it("gives a version rejected at application its reason in words, and keeps the earlier one in effect", () => {
      const [raise, first] = versions(AGENT_IDS.btc, "drawdown");
      expect(raise).toHaveAttribute("data-result", "rejected");
      expect(raise).not.toHaveAttribute("data-current");
      expect(raise).toHaveTextContent("Not applied");
      expect(raise.querySelector("[data-slot=version-rejected]")).toHaveTextContent(
        "Rejected at application, Sep 28, 2026 at 14:03:34 ET. An allocation increase is refused while a limit is latched, and the drawdown ladder holds this agent at selling only. Version 1 stays in effect; version 2 never applied.",
      );
      expect(changes(raise)).toEqual(["Capital$10,000.00to$12,000.00Risk-increasing"]);
      expect(first).toHaveAttribute("data-current", "true");
    });

    it("is reached from the Mandate tab", () => {
      renderScreen(`/agents/${AGENT_IDS.lmn}/mandate`, <AgentSectionScreen agentId={AGENT_IDS.lmn} section="mandate" />);
      expect(within(main()).getByRole("link", { name: "Versions" })).toHaveAttribute("href", `/agents/${AGENT_IDS.lmn}/mandate/versions`);
    });
  });

  it("never implies a limit caps a realized loss", () => {
    renderScreen(`/agents/${AGENT_IDS.btc}`, <AgentDetailScreen agentId={AGENT_IDS.btc} />);
    expect(main()).toHaveTextContent("Gaps, halts, and outages can move prices past any level before an order fills.");
    expect(main()).not.toHaveTextContent(/maximum loss is|cannot lose more|loss is capped|protected from loss/i);
  });

  it("shows an unknown order as unknown and says nothing else is sent in the instrument", () => {
    renderScreen(`/agents/${AGENT_IDS.swing}`, <AgentDetailScreen agentId={AGENT_IDS.swing} />, "unknown-order");
    const row = main().querySelector("li[data-state=Unknown]") as HTMLElement;
    expect(row).toHaveTextContent("Unknown");
    expect(within(row).getByText(/Nothing else is sent in QRS, exits included/)).toHaveAttribute("data-slot", "unknown-order");
    expect(within(row).queryByRole("button")).toBeNull();
  });

  it("renders a denied exit as held, with the rule in plain words", () => {
    renderScreen(`/agents/${AGENT_IDS.swing}`, <AgentDetailScreen agentId={AGENT_IDS.swing} />, "unknown-order");
    const denied = [...main().querySelectorAll("li[data-verdict=deny]")];
    expect(denied.length).toBeGreaterThan(0);
    const exit = denied.find((d) => d.textContent?.includes("Sell 5 QRS"))!;
    expect(exit).toHaveTextContent("Held");
    expect(exit.querySelector("[data-slot=gate-rule]")).toHaveTextContent("An order in this instrument has an unknown state");
    for (const d of denied) expect(d).not.toHaveTextContent(/error|failed|retry|unknown_order_in_flight/i);
  });

  it("says it is checking with the broker during startup reconciliation", () => {
    renderScreen(`/agents/${AGENT_IDS.swing}`, <AgentDetailScreen agentId={AGENT_IDS.swing} />, "reconciliation");
    expect(main().querySelector("[data-slot=reconciling]")).toHaveTextContent("Checking with the broker. Nothing is needed from you.");
    expect(main().querySelector("[data-slot=mode-banner]")).not.toBeNull();
  });

  it("dates a restriction that began on an earlier day on Home's agent card, and leaves today's as a time (C-23)", () => {
    renderScreen("/", <DashboardScreen />, "drawdown");
    const card = within(within(main()).getByRole("region", { name: "Agents" })).getAllByRole("article").find((a) => a.textContent?.includes("Agent 1")) as HTMLElement;
    const restrictions = within(card).getByLabelText("Restrictions");
    expect(restrictions).toHaveTextContent("Drawdown: sizes scaled since Sep 26, 2026, 15:12");
    expect(restrictions).toHaveTextContent("Drawdown: selling only since 14:01:12");
    expect(restrictions).not.toHaveTextContent("since 15:12:40");
  });

  it("dates a restriction that began on an earlier day in the agent's mode banner, and leaves today's as a time (C-23)", () => {
    renderScreen(`/agents/${AGENT_IDS.btc}`, <AgentDetailScreen agentId={AGENT_IDS.btc} />, "drawdown");
    const banner = main().querySelector("[data-slot=mode-banner]") as HTMLElement;
    const item = (title: string) => within(banner).getByText(title).closest("li") as HTMLElement;
    expect(item("Drawdown: sizes scaled")).toHaveTextContent("since Sep 26, 2026, 15:12");
    expect(item("Drawdown: selling only")).toHaveTextContent("since 14:01:12");
    expect(banner).not.toHaveTextContent("since 15:12:40");
  });

  it("dates a restriction on the Alerts screen when it began on an earlier day (C-23)", () => {
    renderScreen("/alerts", <AlertsScreen />, "drawdown");
    const scaled = within(main()).getByRole("link", { name: /Agent 1: Drawdown: sizes scaled/ });
    expect(scaled).toHaveTextContent("Since Sep 26, 2026, 15:12.");
    expect(within(main()).getByRole("link", { name: /Agent 1: Drawdown: selling only/ })).toHaveTextContent("Since 14:01:12.");
  });

  it("dates an older timeline entry through the same formatter, by the day in Eastern time, not the offset it is written in (C-23)", () => {
    setPathname(`/agents/${AGENT_IDS.btc}`);
    renderWithRuntime(<AppShell><AgentDetailScreen agentId={AGENT_IDS.btc} /></AppShell>, "normal", {
      workspace: (ws) => ({
        ...ws,
        timeline: {
          ...ws.timeline,
          [AGENT_IDS.btc]: [
            { event_id: "c23-today", at: "2026-09-28T13:10:00-04:00", kind: "order", text: "An order today." },
            { event_id: "c23-late", at: "2026-09-28T03:59:00Z", kind: "order", text: "An order late the evening before, written in UTC." },
          ],
        },
      }),
    });
    const today = within(main()).getAllByText("An order today.")[0].parentElement as HTMLElement;
    const late = within(main()).getAllByText("An order late the evening before, written in UTC.")[0].parentElement as HTMLElement;
    expect(today.querySelector("time")).toHaveTextContent(/^13:10:00$/);
    expect(late.querySelector("time")).toHaveTextContent(/^Sep 27, 2026, 23:59$/);
  });

  it("shows the mode banner with what is blocked, when it ends, and who acts", () => {
    renderScreen(`/agents/${AGENT_IDS.btc}`, <AgentDetailScreen agentId={AGENT_IDS.btc} />, "drawdown");
    const banner = main().querySelector("[data-slot=mode-banner]") as HTMLElement;
    expect(banner).toHaveTextContent(/Blocks/);
    expect(banner).toHaveTextContent(/Ends when/);
    expect(banner).toHaveTextContent(/Who acts/);
  });

  it("says so for an ID this workspace does not have", () => {
    const missing = "agt_01JB3K8Y4N7QW2M6R9T5V0XZZZ";
    renderScreen(`/agents/${missing}`, <AgentDetailScreen agentId={missing} />);
    expect(main()).toHaveTextContent("No agent with this ID");
  });
});

describe("D5 inbox and D6 request", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const request = (id: string, scenario: (typeof SCENARIO_IDS)[number] = "approvals") =>
    renderScreen(`/approvals/${id}`, <ApprovalRequestScreen approvalId={id} />, scenario);

  it("lays the inbox on the page grid, with why requests come in the rail: each agent's asking rules and its window", () => {
    renderScreen("/approvals", <ApprovalsInboxScreen />, "approvals");
    const column = main().querySelector<HTMLElement>("[data-layout=main]")!;
    const rail = main().querySelector<HTMLElement>("[data-layout=rail]")!;
    expect(column.parentElement).toBe(rail.parentElement);
    expect(rail.parentElement?.className).toContain("lg:grid-cols-[minmax(0,1fr)_20rem]");
    expect(within(column).getByRole("region", { name: "Open, by deadline" })).toBeInTheDocument();
    const why = within(rail).getByRole("region", { name: "What sends you a request" });
    const ws = buildWorkspace("approvals");
    const rows = [...why.querySelectorAll("li")];
    expect(rows).toHaveLength(ws.agents.length);
    rows.forEach((row, i) => {
      const agent = ws.agents[i];
      expect(within(row).getByRole("link", { name: agent.label })).toHaveAttribute("href", `/agents/${agent.agent_id}/mandate`);
      expect(row).toHaveTextContent(askSentence(agent));
      expect(row).toHaveTextContent(/A request waits .+, then is skipped\.$/);
    });
    expect(askSentence(ws.agents.find((a) => a.agent_id === AGENT_IDS.swing)!)).toMatch(/^Your rules?: (.+; )*ask when the combined model score is below 0.65[;.]/);
  });

  it("gives Approve and Skip the same variant and weight, with no focus or selection", () => {
    request(APPROVAL_IDS.btc);
    const choices = main().querySelector("[data-slot=approval-choices]") as HTMLElement;
    const [approve, skip] = within(choices).getAllByRole("button");
    expect(approve).toHaveTextContent("Approve");
    expect(skip).toHaveTextContent("Skip");
    expect(approve.className).toBe(skip.className);
    expect(approve.getAttribute("data-variant")).toBe(skip.getAttribute("data-variant"));
    expect(approve.getAttribute("data-size")).toBe(skip.getAttribute("data-size"));
    for (const b of [approve, skip]) {
      expect(b).not.toHaveAttribute("autofocus");
      expect(b).not.toHaveAttribute("aria-pressed");
      expect(b).not.toHaveAttribute("aria-selected");
      expect(b).not.toHaveAttribute("aria-checked");
      expect(document.activeElement).not.toBe(b);
    }
  });

  it("draws Approve and Skip as one solid lit key in Public Sans, both alike, flat, with Kumo's ring left only for focus (DEC-467)", () => {
    request(APPROVAL_IDS.btc);
    const choices = main().querySelector("[data-slot=approval-choices]") as HTMLElement;
    const [approve, skip] = within(choices).getAllByRole("button");
    expect(approve.className).toBe(skip.className);
    for (const b of [approve, skip]) {
      for (const c of DECISION_KEY.split(" ")) expect(b).toHaveClass(c);
      expect(b).toHaveClass("bg-card", "text-foreground", "font-semibold");
      expect(b).not.toHaveClass("pixel-face", "bg-muted", "border-t-card", "ring", "shadow-xs", "border-0");
      expect(b).toHaveClass("focus-visible:ring-3", "focus-visible:ring-ring");
      expect(b.className).not.toMatch(/bg-\[|shadow-(?!none)/);
    }
    const stop = within(screen.getByRole("navigation", { name: "Primary" })).getByRole("button", { name: "Stop" });
    expect(stop.className + stop.innerHTML, "DEC-469: Stop keeps its own shape, never the key").not.toMatch(/pixel-face|border-t-card|border-b-\[3px\]/);
  });

  it("states the default, the trigger, the risk in dollars, and the score's meaning", () => {
    request(APPROVAL_IDS.btc);
    expect(main()).toHaveTextContent("If you do nothing, this action is skipped.");
    expect(main()).toHaveTextContent("Your rule: ask when the combined model score is below 0.65.");
    expect(within(main()).getByRole("heading", { name: "Risk impact in dollars" })).toBeInTheDocument();
    expect(main()).toHaveTextContent("$850.50");
    expect(main()).toHaveTextContent("Combined model score, not a probability of profit");
    expect(main().querySelector("[data-slot=deadline]")).toHaveTextContent("Skipped at 14:13:40 ET if you do nothing");
  });

  it("keeps model output behind a control and labels its author", () => {
    request(APPROVAL_IDS.btc);
    expect(screen.queryByText(/Output of software you selected/)).toBeNull();
    fireEvent.click(within(main()).getByRole("button", { name: "View model output" }));
    expect(within(main()).getByText(/Output of software you selected/)).toBeInTheDocument();
  });

  it("collapses only model output (D6): every other required value is on screen with it closed (§4.1)", () => {
    request(APPROVAL_IDS.lmn);
    const expanders = Array.from(main().querySelectorAll("[aria-expanded]"));
    expect(expanders.map((e) => e.textContent?.trim())).toEqual(["View model output"]);
    expect(expanders[0]).toHaveAttribute("aria-expanded", "false");
    expect(main().querySelector("details, [hidden]")).toBeNull();
    const approval = findApproval(buildWorkspace("approvals"), APPROVAL_IDS.lmn)!;
    const required = [
      `Buy ${approval.bound.qty}`,
      approval.bound.symbol,
      `at a limit of ${price(approval.bound.limit)}`,
      "Order value",
      PURPOSE_LABEL[approval.bound.purpose],
      approval.bound.mandate_version.slice(7, 19),
      approval.trigger,
      "Risk impact in dollars",
      "Combined model score, not a probability of profit",
      approval.bound.combined_score,
      "If you do nothing, this action is skipped.",
      `Skipped at ${clock(approval.deadline)}`,
      "Needs 2 approvers. Approved so far:",
    ];
    for (const text of required) expect(main().textContent, text).toContain(text);
    expect(main().querySelector("[data-slot=deadline]")).toHaveTextContent(/if you do nothing \(\d+ min left\)/);
  });

  it("records with the response whether model output was opened first", () => {
    function Responses() {
      const { responses } = useRuntime();
      return <output data-slot="responses" data-json={JSON.stringify(responses)} />;
    }
    const recordFor = (id: string) => JSON.parse(document.querySelector("[data-slot=responses]")!.getAttribute("data-json")!)[id].record;
    const both = (id: string) => (
      <>
        <ApprovalRequestScreen approvalId={id} />
        <Responses />
      </>
    );

    const approval = findApproval(buildWorkspace("approvals"), APPROVAL_IDS.btc)!;
    const modelLines = approval.evidence.flatMap((e) => e.lines);

    const closed = renderScreen(`/approvals/${APPROVAL_IDS.btc}`, both(APPROVAL_IDS.btc), "approvals");
    fireEvent.click(within(main()).getByRole("button", { name: "Approve" }));
    const first = recordFor(APPROVAL_IDS.btc);
    expect(first).toMatchObject({ screen: "D6", environment: "paper", modelOutputExpanded: false });
    expect(first.shown).toContain(`Why you are asked: ${approval.trigger}`);
    expect(first.shown).toContain("If you do nothing, this action is skipped.");
    for (const line of modelLines) expect(first.shown).not.toContain(line);
    closed.unmount();

    renderScreen(`/approvals/${APPROVAL_IDS.btc}`, both(APPROVAL_IDS.btc), "approvals");
    const view = within(main()).getByRole("button", { name: "View model output" });
    fireEvent.click(view);
    fireEvent.click(view);
    fireEvent.click(within(main()).getByRole("button", { name: "Skip" }));
    const second = recordFor(APPROVAL_IDS.btc);
    expect(second).toMatchObject({ screen: "D6", environment: "paper", modelOutputExpanded: true });
    for (const line of modelLines) expect(second.shown).toContain(line);
  });

  it("keeps the request as the owner confirmed it, with who has approved so far as it read, and live progress outside it (§4.1)", () => {
    function Responses() {
      const { responses } = useRuntime();
      return <output data-slot="responses" data-json={JSON.stringify(responses)} />;
    }
    renderScreen(
      `/approvals/${APPROVAL_IDS.lmn}`,
      <>
        <ApprovalRequestScreen approvalId={APPROVAL_IDS.lmn} />
        <Responses />
      </>,
      "approvals",
    );
    const record = main().querySelector<HTMLElement>("[data-slot=record]")!;
    const confirmed = record.innerHTML;
    const approvers = record.querySelector("[data-slot=approvers]")!.textContent!;
    expect(approvers).toBe("Needs 2 approvers. Approved so far: Priya (approver) at 14:02:31.");

    fireEvent.click(within(main()).getByRole("button", { name: "Approve" }));
    const stored = JSON.parse(document.querySelector("[data-slot=responses]")!.getAttribute("data-json")!)[APPROVAL_IDS.lmn].record;
    expect(stored.shown).toContain(approvers);
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 3));

    expect(record.innerHTML).toBe(confirmed);
    const after = main().querySelector<HTMLElement>("[data-slot=after-confirm]")!;
    expect(record.contains(after)).toBe(false);
    expect(within(after).getByRole("heading", { name: "You chose Approve" })).toBeInTheDocument();
    expect(after).toHaveTextContent(/Approved so far: Priya \(approver\) at 14:02:31, You at \d{2}:\d{2}:\d{2}\./);
  });

  it("withdraws Approve and Skip when the request closes before a response, and keeps the request as it was shown", () => {
    const elsewhere: { send?: ReturnType<typeof useRuntime>["send"] } = {};
    function Elsewhere() {
      const { send } = useRuntime();
      useEffect(() => {
        elsewhere.send = send;
      }, [send]);
      return null;
    }
    const approval = findApproval(buildWorkspace("approvals"), APPROVAL_IDS.btc)!;
    renderScreen(
      `/approvals/${APPROVAL_IDS.btc}`,
      <>
        <ApprovalRequestScreen approvalId={APPROVAL_IDS.btc} />
        <Elsewhere />
      </>,
      "approvals",
    );
    const record = main().querySelector<HTMLElement>("[data-slot=record]")!;
    const opened = record.innerHTML;
    act(() => elsewhere.send!("pause", approval.agent_id));
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));

    expect(record.innerHTML).toBe(opened);
    expect(within(main()).queryByRole("button", { name: "Approve" })).toBeNull();
    expect(within(main()).queryByRole("button", { name: "Skip" })).toBeNull();
    expect(within(main()).getByRole("region", { name: "Outcome" })).toHaveTextContent("Canceled: you paused the agent.");
  });

  it("shows nothing as approved or submitted until the runtime records it", () => {
    request(APPROVAL_IDS.btc);
    fireEvent.click(within(main()).getByRole("button", { name: "Approve" }));
    const article = within(main()).getByRole("article");
    expect(article).toHaveTextContent("Not recorded yet; if the runtime does not record it before the deadline (14:13:40), the action is skipped.");
    expect(article).not.toHaveTextContent(/\bapproved\b|submitted/i);
    expect(within(main()).queryByRole("button", { name: "Approve" })).toBeNull();

    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS - 1));
    expect(article).not.toHaveTextContent(/\bapproved\b|submitted/i);
    act(() => vi.advanceTimersByTime(1));
    expect(article.querySelector("[data-phase=recorded]")).toHaveTextContent(/Recorded at \d{2}:\d{2}:\d{2}: you approved/);
    expect(article).not.toHaveTextContent(/submitted/i);

    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 0.75));
    const outcome = within(article).getByRole("region", { name: "Outcome" });
    expect(outcome).toHaveAttribute("data-status", "acted");
    expect(outcome).toHaveTextContent("submitted to the paper broker");
  });

  it("turns the pinned choices into the response and its progress in place, and moves focus there without scrolling", () => {
    request(APPROVAL_IDS.btc);
    const bar = within(main()).getByRole("region", { name: "Your response" });
    const scroll = vi.spyOn(HTMLElement.prototype, "focus");
    fireEvent.click(within(bar).getByRole("button", { name: "Approve" }));
    const heading = within(bar).getByRole("heading", { name: "You chose Approve" });
    expect(document.activeElement).toBe(heading);
    expect(scroll).toHaveBeenCalledWith({ preventScroll: true });
    scroll.mockRestore();
    const steps = () => [...bar.querySelectorAll("[data-slot=response-progress] li")].map((li) => [li.textContent, li.getAttribute("data-state")]);
    expect(steps()).toEqual([
      ["Sent, done", "done"],
      ["Recorded in the journal, in progress", "current"],
      ["Result, not yet", "waiting"],
    ]);
    expect(bar).not.toHaveTextContent("If you do nothing");

    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(steps()[1]).toEqual(["Recorded in the journal, done", "done"]);
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS * 0.75));
    expect(within(main()).getByRole("region", { name: "Your response" })).toBe(bar);
    expect(steps()[2]).toEqual(["Approved and submitted, done", "done"]);
    expect(within(bar).getByRole("region", { name: "Outcome" })).toHaveAttribute("data-status", "acted");
  });

  it("records a skip as skipped with nothing sent", () => {
    request(APPROVAL_IDS.btc);
    fireEvent.click(within(main()).getByRole("button", { name: "Skip" }));
    expect(main()).toHaveTextContent("Your skip was sent. Not recorded yet");
    act(() => vi.advanceTimersByTime(RECORD_AFTER_MS));
    expect(within(main()).getByRole("region", { name: "Outcome" })).toHaveTextContent("Skipped by you. Nothing was sent.");
  });

  it("waits for a second approver when the rule needs two", () => {
    request(APPROVAL_IDS.lmn);
    expect(main()).toHaveTextContent("Needs 2 approvers. Approved so far: Priya (approver) at 14:02:31.");
  });

  it("treats a request past its deadline as skipped", () => {
    const pending = findApproval(buildWorkspace("approvals"), APPROVAL_IDS.btc)!;
    expect(approvalAt(pending, "2026-09-28T14:13:39-04:00").status).toBe("delivered");
    const late = approvalAt(pending, "2026-09-28T14:13:40-04:00");
    expect(late.status).toBe("expired");
    expect(late.resolution?.text).toBe("Skipped at the deadline. Nothing was sent.");
  });

  it("lists open requests by deadline, earliest first", () => {
    renderScreen("/approvals", <ApprovalsInboxScreen />, "approvals");
    const open = within(main()).getByRole("region", { name: "Open, by deadline" });
    const deadlines = [...open.querySelectorAll("[data-slot=deadline] time")].map((t) => t.getAttribute("datetime") ?? "");
    expect(deadlines.length).toBe(3);
    expect([...deadlines].sort((a, b) => Date.parse(a) - Date.parse(b))).toEqual(deadlines);
  });
});

describe("routes", () => {
  it("keeps the agent and approval titles generic", () => {
    expect(agentRoute.metadata.title).toBe("Agent");
    expect(approvalRoute.metadata.title).toBe("Approval request");
  });

  it("rejects IDs that are not opaque ULIDs", async () => {
    await expect(agentRoute.default({ params: Promise.resolve({ agentId: "btc-accumulator" }) })).rejects.toThrow("NEXT_NOT_FOUND");
    await expect(approvalRoute.default({ params: Promise.resolve({ approvalId: "1" }) })).rejects.toThrow("NEXT_NOT_FOUND");
    await expect(agentRoute.default({ params: Promise.resolve({ agentId: AGENT_IDS.btc }) })).resolves.toBeTruthy();
  });
});
