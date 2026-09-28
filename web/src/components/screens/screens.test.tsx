import type { ReactElement } from "react";
import { act, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as agentRoute from "@/app/agents/[agentId]/page";
import * as approvalRoute from "@/app/approvals/[approvalId]/page";
import { AppShell } from "@/components/shell/app-shell";
import { AGENT_IDS, APPROVAL_IDS, SCENARIOS, buildWorkspace, findApproval } from "@/fixtures/workspace";
import { approvalAt } from "@/lib/mock-runtime";
import { RECORD_AFTER_MS, isDisabled, renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";
import { AgentDetailScreen } from "./agent-detail";
import { AgentsListScreen } from "./agents-list";
import { ApprovalRequestScreen } from "./approval-request";
import { ApprovalsInboxScreen } from "./approvals-inbox";
import { DashboardScreen } from "./dashboard";

const SCENARIO_IDS = SCENARIOS.map((s) => s.id);
const PERSUASIVE = /guarantee|safer|safe bet|recommend|profit (estimate|target)|price target|expected (return|profit)|don.t miss|act now|hurry|opportunity/i;

const SCREENS: Array<[string, () => ReactElement]> = [
  ["/", () => <DashboardScreen />],
  ["/agents", () => <AgentsListScreen />],
  [`/agents/${AGENT_IDS.btc}`, () => <AgentDetailScreen agentId={AGENT_IDS.btc} />],
  [`/agents/${AGENT_IDS.swing}`, () => <AgentDetailScreen agentId={AGENT_IDS.swing} />],
  [`/agents/${AGENT_IDS.lmn}`, () => <AgentDetailScreen agentId={AGENT_IDS.lmn} />],
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
    expect(isDisabled(within(header).getByRole("button", { name: "Stop" }))).toBe(false);
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

  it("puts the performance placeholder beside every P&L", () => {
    renderScreen("/", <DashboardScreen />);
    const cards = within(main()).getAllByRole("article");
    expect(cards).toHaveLength(3);
    for (const card of cards) {
      expect(card).toHaveTextContent("Paper P&L, simulated");
      expect(card.querySelector("[data-placeholder=performance]")).toHaveTextContent("[[DISCLOSURE-PERFORMANCE]]");
    }
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

  it("shows a paused agent's restriction with what it blocks and how it ends", () => {
    renderScreen("/", <DashboardScreen />, "paused");
    const restrictions = within(main()).getByRole("list", { name: "Restrictions" });
    expect(restrictions).toHaveTextContent(/Blocks .+\. Ends when: .+\./);
  });
});

describe("D2 agent detail", () => {
  it("draws limits as rails in dollars and the profit stop as a level, never a rail", () => {
    renderScreen(`/agents/${AGENT_IDS.swing}`, <AgentDetailScreen agentId={AGENT_IDS.swing} />);
    const envelope = within(within(main()).getByRole("region", { name: "Limits in dollars" })).getByText(
      (_, el) => el?.getAttribute("data-slot") === "envelope",
    );
    const rails = [...envelope.querySelectorAll("[data-slot=limit-rail]")];
    expect(rails.length).toBeGreaterThan(0);
    for (const rail of rails) expect(rail).toHaveTextContent(/\$[\d,]+\.\d{2} of \$[\d,]+\.\d{2}/);
    expect(rails.some((r) => /profit/i.test(r.textContent ?? ""))).toBe(false);
    expect(envelope.querySelector("[data-level=profit_stop]")).toHaveTextContent("Profit stop");
    expect(envelope.querySelector("[role=progressbar]")).toBeNull();
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

  it("states the default, the trigger, the risk in dollars, and the score's meaning", () => {
    request(APPROVAL_IDS.btc);
    expect(main()).toHaveTextContent("If you do nothing, this action is skipped.");
    expect(main()).toHaveTextContent("Your rule “low_score”: ask when the combined model score is below 0.65.");
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
