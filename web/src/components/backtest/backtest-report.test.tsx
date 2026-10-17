import { readFileSync } from "node:fs";
import { join } from "node:path";
import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { AgentBacktests, BacktestReport, BacktestReportScreen } from "./backtest-report";
import { type Backtest, backtestRead } from "@/api/backtest";
import { type Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import { AgentSectionScreen } from "@/components/screens/agent-detail";
import { BACKTEST_FIXTURES, BACKTEST_IDS } from "@/fixtures/backtests";
import { AGENT_IDS } from "@/fixtures/workspace";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { RoleProvider } from "@/lib/roles";
import { renderWithRuntime } from "@/test/harness";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";
const EXAMPLE = JSON.parse(readFileSync(join(__dirname, "..", "..", "..", "..", "schemas", "workspace-api", "examples", "read-models.backtest.json"), "utf8")) as Record<string, unknown>;

/** Figures, codes and names only (DEC-1003 item 2), and no thesis-quality figure (DEC-1001 item 3). */
const FORBIDDEN = /hit rate|win rate|score|accuracy|thesis quality|expected|projected|forecast|outperform|beat|best|strong|weak/i;

function decoded(body: unknown): Backtest {
  const read = backtestRead(body, "");
  if (!read.ok) throw new Error(`${read.issue.problem} ${read.issue.path}`);
  return read.value.backtest;
}

function withStrategy(patch: Record<string, unknown>): Backtest {
  const body = structuredClone(EXAMPLE) as { backtest: { report: { strategy: Record<string, unknown> } } };
  Object.assign(body.backtest.report.strategy, patch);
  return decoded(body);
}

function apiMount(node: React.ReactNode, tamper?: (route: string, body: Record<string, unknown>) => Record<string, unknown>) {
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const fetch: Fetch = async (input, init) => {
    const route = new URL(input).pathname.slice(`/v1/workspaces/${WS}`.length);
    const answer = await server.fetch(input, init);
    if (!tamper || !answer.ok) return answer;
    return new Response(JSON.stringify(tamper(route, (await answer.json()) as Record<string, unknown>)), { status: answer.status });
  };
  render(
    <RoleProvider>
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        {node}
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
}

const placeholders = () => Array.from(document.querySelectorAll("[data-placeholder]")).map((p) => p.textContent);
const table = (name: string) => screen.getByRole("table", { name });
const cellsOf = (row: HTMLElement) => within(row).getAllByRole("cell").map((c) => c.textContent?.replace(/\s+/g, " ").trim());
function rowNamed(t: HTMLElement, header: RegExp): HTMLElement {
  const found = within(t)
    .getAllByRole("row")
    .find((r) => header.test(within(r).queryByRole("rowheader")?.textContent ?? ""));
  if (!found) throw new Error(`no row ${header}`);
  return found;
}

afterEach(() => cleanup());

describe("the backtest report (B2; DEC-1001, DEC-1003, DEC-2400)", () => {
  it("shows a completed run: the legend first, the version, the instrument, the dates and both columns of FR-4.2", async () => {
    renderWithRuntime(<BacktestReportScreen backtestId={BACKTEST_IDS.lmnCompleted} />);
    expect(await screen.findByRole("heading", { level: 1, name: "Backtest" })).toBeInTheDocument();
    expect(placeholders()[0]).toBe("[[LEGEND-HYPOTHETICAL]]");
    expect(screen.getByText(/^Simulated/)).toBeInTheDocument();
    const facts = screen.getByRole("region", { name: "The run" });
    expect(facts).toHaveTextContent("Completed");
    expect(facts).toHaveTextContent("sha256:95120ef74b1d56aa7db4a8c7c607a374b1f9177d7768d7732483f66ecafd9f9f");
    expect(facts).toHaveTextContent("7b4a1c2e-4444-4a2b-9c3d-000000000004");

    const metrics = table("This version and buy and hold");
    expect(within(metrics).getAllByRole("columnheader").map((h) => h.textContent)).toEqual(["Measure", "This version", "Buy and hold"]);
    expect(cellsOf(rowNamed(metrics, /^First date/))).toEqual(["2026-09-14", "2026-09-14"]);
    expect(cellsOf(rowNamed(metrics, /^Last date/))).toEqual(["2026-09-18", "2026-09-18"]);
    expect(cellsOf(rowNamed(metrics, /^Periods$/))).toEqual(["5", "5"]);
    expect(cellsOf(rowNamed(metrics, /^Starting equity/))).toEqual(["$10,000.00", "$10,000.00"]);
    expect(cellsOf(rowNamed(metrics, /^Ending equity/))).toEqual(["$10,042.18", "$10,105.40"]);
    expect(cellsOf(rowNamed(metrics, /^Total return/))).toEqual(["0.4218%", "1.054%"]);
    expect(cellsOf(rowNamed(metrics, /^Volatility per period/))).toEqual(["0.1904904985%", "0.2313855225%"]);
    expect(cellsOf(rowNamed(metrics, /^Sharpe ratio per period/))).toEqual(["0.442873053324", "0.908141236451"]);
    expect(cellsOf(rowNamed(metrics, /^Maximum drawdown$/))).toEqual(["0.1913951912%", "0.1440893553%"]);
    expect(cellsOf(rowNamed(metrics, /^Turnover/))).toEqual(["0.5014", "0.99935"]);
    expect(cellsOf(rowNamed(metrics, /^Fees/))).toEqual(["$0.00", "$0.00"]);
    expect(cellsOf(rowNamed(metrics, /^Fills/))).toEqual(["1", "1"]);
    expect(screen.getByRole("region", { name: "This version and buy and hold" })).toHaveTextContent("Excess total return−0.6322%");
  });

  it("shows net P&L as a signed figure with the performance disclosure in its region", async () => {
    renderWithRuntime(<BacktestReportScreen backtestId={BACKTEST_IDS.lmnCompleted} />);
    const metrics = await screen.findByRole("table", { name: "This version and buy and hold" });
    const row = rowNamed(metrics, /^Net P&L/);
    const signed = row.querySelectorAll("[data-direction]");
    expect(Array.from(signed).map((s) => s.textContent)).toEqual(["+$42.18 gain", "+$105.40 gain"]);
    const region = row.closest("section")!;
    expect(region.querySelector('[data-placeholder="performance"]')).toHaveTextContent("[[DISCLOSURE-PERFORMANCE]]");
    expect(within(region).getByRole("button", { name: "Performance disclosure" })).toHaveAccessibleDescription("[[DISCLOSURE-PERFORMANCE]]");
  });

  it("shows a statistic the run did not compute as not computed, with its reason code, never a number", () => {
    render(<BacktestReport backtest={withStrategy({ variance: null, volatility: null, volatility_annualized: null, sharpe: null, sharpe_annualized: null, absent: "fewer_than_two_periods" })} />);
    const metrics = table("This version and buy and hold");
    for (const name of [/^Volatility per period/, /^Volatility, annualized/, /^Sharpe ratio per period/, /^Sharpe ratio, annualized/]) {
      const [strategy, benchmark] = within(rowNamed(metrics, name)).getAllByRole("cell");
      expect(strategy).toHaveTextContent(/^Not computed: fewer_than_two_periods$/);
      expect(strategy.querySelector("code")).toHaveTextContent("fewer_than_two_periods");
      expect(strategy.textContent).not.toMatch(/[0-9]/);
      expect(benchmark.textContent).toMatch(/[0-9]/);
    }
  });

  it("shows every decision: the count, the nine outcome counts, the reason counts and one row per decision", () => {
    render(<BacktestReport backtest={decoded(EXAMPLE)} />);
    const decisions = screen.getByRole("region", { name: "Decisions" });
    expect(decisions).toHaveTextContent("9 decisions");
    const outcomes = within(decisions).getByRole("table", { name: "Outcomes" });
    expect(within(outcomes).getAllByRole("row").slice(1).map((r) => r.textContent)).toEqual([
      "submitted1",
      "builder_hold1",
      "builder_refused1",
      "gate_deny1",
      "gate_defer1",
      "gate_hold1",
      "autonomy_deny1",
      "ask_unanswered1",
      "order_working1",
    ]);
    expect(within(decisions).getByRole("table", { name: "Gate reasons" })).toHaveTextContent("None");
    expect(within(decisions).getByRole("table", { name: "Hold reasons" })).toHaveTextContent("at_or_above_target1");
    const rows = within(decisions).getByRole("table", { name: "Each decision" });
    expect(within(rows).getAllByRole("columnheader").map((h) => h.textContent)).toEqual(["Date", "Purpose", "Outcome", "Reason", "Decided by", "Quantity", "Limit", "Outputs used"]);
    const body = within(rows).getAllByRole("row").slice(1);
    expect(body).toHaveLength(9);
    expect(cellsOf(body[0])).toEqual(["2026-09-01", "open", "submitted", "None", "rule:small_opening", "20", "$250.05", "1"]);
    expect(cellsOf(body[1])).toEqual(["2026-09-02", "hold", "builder_hold", "at_or_above_target", "None", "None", "None", "1"]);
    for (const code of ["open", "submitted", "rule:small_opening", "at_or_above_target"]) expect(within(rows).getAllByText(code)[0].tagName).toBe("CODE");
  });

  it("shows the risk section, the limitations as their codes, and each disclosure as its named placeholder", () => {
    render(<BacktestReport backtest={decoded(EXAMPLE)} />);
    const risk = screen.getByRole("region", { name: "Risk" });
    expect(within(risk).getByText("drawdown_ladder[0]").tagName).toBe("CODE");
    expect(risk).toHaveTextContent("Risk exits0");
    expect(within(risk).getByText("normal").tagName).toBe("CODE");
    const limitations = screen.getByRole("region", { name: "Limitations" });
    expect(within(limitations).getAllByRole("listitem").map((li) => li.textContent)).toEqual((EXAMPLE as { backtest: { report: { limitations: string[] } } }).backtest.report.limitations);
    expect(placeholders()).toEqual(["[[LEGEND-HYPOTHETICAL]]", "[[NOTE-BACKTEST-MECHANICS]]", "[[NOTE-BACKTEST-LIMITS]]"]);
  });

  it("shows only the disclosures a report names, the legend always first", () => {
    const body = structuredClone(EXAMPLE) as { backtest: { report: { disclosures: string[]; research_agent: boolean } } };
    body.backtest.report.disclosures = ["NOTE-BACKTEST-LIMITS"];
    body.backtest.report.research_agent = false;
    render(<BacktestReport backtest={decoded(body)} />);
    expect(placeholders()).toEqual(["[[LEGEND-HYPOTHETICAL]]", "[[NOTE-BACKTEST-LIMITS]]"]);
  });

  it("holds no thesis-quality figure, adjective, projection or expected figure, even when the served report carries one", () => {
    const body = structuredClone(EXAMPLE) as { backtest: { report: Record<string, unknown> } };
    body.backtest.report.hit_rate = "0.6";
    body.backtest.report.expected_return = "0.1";
    render(<BacktestReport backtest={decoded(body)} />);
    expect(document.body.textContent).not.toMatch(FORBIDDEN);
    expect(document.body.textContent).not.toContain("0.6");
  });

  it("shows a running run as having no report yet, and a refused run's code verbatim", async () => {
    renderWithRuntime(<BacktestReportScreen backtestId={BACKTEST_IDS.lmnRunning} />);
    const running = await screen.findByRole("region", { name: "The run" });
    expect(running).toHaveTextContent("Running");
    expect(screen.getByText("No report yet.")).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
    cleanup();

    renderWithRuntime(<BacktestReportScreen backtestId={BACKTEST_IDS.btcRefused} />);
    const refused = await screen.findByRole("region", { name: "The run" });
    expect(refused).toHaveTextContent("Refused");
    expect(within(refused).getByText("asset_class_not_supported").tagName).toBe("CODE");
    expect(screen.queryByRole("table")).toBeNull();
    expect(document.body.textContent).not.toMatch(FORBIDDEN);
  });

  it("says a backtest the fixtures do not hold is not found", async () => {
    renderWithRuntime(<BacktestReportScreen backtestId="bt_01JBZZZZZZZZZZZZZZZZZZZZZZ" />);
    expect(await screen.findByRole("heading", { level: 1, name: "Backtest not found" })).toBeInTheDocument();
  });
});

describe("the backtest report over the workspace API (DEC-2400)", () => {
  it("reads the report from the API and shows it, with no fixture tag", async () => {
    apiMount(<BacktestReportScreen backtestId={BACKTEST_IDS.lmnCompleted} />);
    expect(await screen.findByRole("table", { name: "This version and buy and hold" })).toBeInTheDocument();
    expect(document.querySelector("[data-slot=fixture-tag]")).toBeNull();
  });

  it("says not found when the API answers 404", async () => {
    apiMount(<BacktestReportScreen backtestId="bt_01JBZZZZZZZZZZZZZZZZZZZZZZ" />);
    expect(await screen.findByRole("heading", { level: 1, name: "Backtest not found" })).toBeInTheDocument();
  });

  it("refuses an answer for another backtest, and names what it could not read", async () => {
    apiMount(<BacktestReportScreen backtestId={BACKTEST_IDS.lmnCompleted} />, (route, body) =>
      route.startsWith("/backtests/") ? { ...body, backtest: (BACKTEST_FIXTURES[BACKTEST_IDS.lmnRunning] as { backtest: unknown }).backtest } : body,
    );
    expect(await screen.findByRole("heading", { level: 1, name: "Backtest could not be read" })).toBeInTheDocument();
    expect(screen.getByText("/backtest/backtest_id")).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
  });

  it("shows what failed when an answer cannot be decoded", async () => {
    apiMount(<BacktestReportScreen backtestId={BACKTEST_IDS.lmnCompleted} />, (route, body) => {
      if (!route.startsWith("/backtests/")) return body;
      const copy = structuredClone(body) as { backtest: { report: { strategy: Record<string, unknown> } } };
      copy.backtest.report.strategy.net_pnl = 42.18;
      return copy;
    });
    expect(await screen.findByRole("heading", { level: 1, name: "Backtest could not be read" })).toBeInTheDocument();
    expect(screen.getByText("decimal_number")).toBeInTheDocument();
    expect(screen.getByText("/backtest/report/strategy/net_pnl")).toBeInTheDocument();
  });
});

describe("an agent's Backtests section (DEC-2400 item 4)", () => {
  it("lists the agent's backtests on the fixtures, each linking to its report", () => {
    renderWithRuntime(<AgentBacktests agentId={AGENT_IDS.lmn} />);
    const links = screen.getAllByRole("link");
    expect(links.map((a) => a.getAttribute("href"))).toEqual([`/backtests/${BACKTEST_IDS.lmnRunning}`, `/backtests/${BACKTEST_IDS.lmnCompleted}`]);
    expect(links.map((a) => a.textContent)).toEqual([expect.stringContaining("Running"), expect.stringContaining("Completed")]);
  });

  it("says an agent has none when it has none", () => {
    renderWithRuntime(<AgentBacktests agentId={AGENT_IDS.swing} />);
    expect(screen.getByText("No backtest of this agent's mandate yet.")).toBeInTheDocument();
    expect(screen.queryByRole("link")).toBeNull();
  });

  it("is built on the fixtures, and over the API keeps the not-available notice: there is no list route", async () => {
    renderWithRuntime(<AgentSectionScreen agentId={AGENT_IDS.btc} section="prove/backtests" />);
    expect(screen.getByRole("link", { name: /Refused/ }).getAttribute("href")).toBe(`/backtests/${BACKTEST_IDS.btcRefused}`);
    expect(document.querySelector("[data-slot=coming-soon]")).toBeNull();
    cleanup();

    apiMount(<AgentSectionScreen agentId={AGENT_IDS.btc} section="prove/backtests" />);
    expect(await screen.findByRole("heading", { name: "Not available from your deployment yet" })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /Refused/ })).toBeNull();
  });
});
