import { render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { Fetch } from "@/api/client";
import { createMockServer } from "@/api/mock-server";
import PortfolioPage from "@/app/(app)/portfolio/page";
import { AGENT_IDS } from "@/fixtures/workspace";
import { ApiRuntimeProvider } from "@/lib/api-runtime";
import { RoleProvider } from "@/lib/roles";
import { renderWithRuntime } from "@/test/harness";
import { setPathname } from "@/test/navigation";

beforeEach(() => setPathname("/portfolio"));

const section = (name: RegExp | string) => screen.getByRole("region", { name });
const breakdown = (id: string) => document.querySelector(`[data-breakdown=${id}]`) as HTMLElement;
/** A figure as a screen reader hears it: its one plain copy (the rolling digits are hidden from it), and the word beside a signed one. */
const spoken = (dd: Element | null | undefined) => {
  const copy = dd?.querySelector("[data-slot=figure] > .sr-only");
  if (!copy) return dd?.textContent;
  const word = dd?.querySelector("[data-direction] > .font-sans")?.textContent;
  return word ? `${copy.textContent} ${word}` : copy.textContent;
};
const rows = (el: HTMLElement) => Array.from(el.querySelectorAll("[data-slot=slice]")).map((li) => li.textContent?.replace(/\s+/g, " ").trim());

describe("the Portfolio page over the fixtures (DEC-2413)", () => {
  it("names the page and its environment, paper, beside the money", () => {
    renderWithRuntime(<PortfolioPage />);
    const header = screen.getByRole("heading", { level: 1, name: "Portfolio" }).closest("header") as HTMLElement;
    expect(within(header).getByText("PAPER")).toBeInTheDocument();
    expect(within(header).getByText("Fixture data")).toBeInTheDocument();
  });

  it("draws the account's value over time with its ranges", () => {
    renderWithRuntime(<PortfolioPage />);
    const hero = section("Account value");
    expect(within(hero).getByRole("group", { name: "Account value range" })).toBeInTheDocument();
    expect(within(within(hero).getByRole("group", { name: "Account value range" })).getAllByRole("button").map((b) => b.textContent)).toEqual(["1D", "1W", "1M", "3M", "All"]);
  });

  it("shows cash and invested, and today's change and the total P&L each with its sign and word", () => {
    renderWithRuntime(<PortfolioPage />);
    const figures = section("Account figures");
    const value = (label: string) => spoken(within(figures).getByText(label).nextElementSibling);
    expect(value("Invested")).toBe("$8,432.77");
    expect(value("Agents' cash")).toBe("$16,632.79");
    expect(value("Not managed by an agent")).toBe("$3,412.80");
    expect(value("Today")).toBe("+$55.56 gain");
    expect(value("Total P&L")).toBe("+$65.56 gain");
    expect(value("Unrealized")).toBe("+$152.30 gain");
    expect(within(figures).getByRole("button", { name: "Performance disclosure" })).toBeInTheDocument();
  });

  it("lists every holding with the agent holding it, each linking to that agent's page", () => {
    renderWithRuntime(<PortfolioPage />);
    const table = within(section("Holdings")).getByRole("table", { name: "Holdings" });
    expect(within(table).getAllByRole("columnheader").map((h) => h.textContent)).toEqual(["Instrument", "Agent", "Quantity", "Avg cost", "Last", "Value", "Unrealized"]);
    const body = within(table).getAllByRole("row").slice(1);
    expect(body.map((r) => within(r).getByRole("link").getAttribute("href"))).toEqual([`/agents/${AGENT_IDS.btc}`, `/agents/${AGENT_IDS.swing}`, `/agents/${AGENT_IDS.swing}`]);
    expect(within(body[2]).getByText("−$5.55").closest("[data-direction]")).toHaveAttribute("data-direction", "loss");
    expect(within(body[2]).getByText("loss")).toHaveClass("sr-only");
  });

  it("breaks the money down by agent, instrument, asset class, and cash and invested", () => {
    renderWithRuntime(<PortfolioPage />);
    expect(rows(breakdown("by-agent-title"))).toEqual(["Agent 1$6,814.6880.8%", "Agent 2$1,618.0919.2%", "Agent 3$0.000%"]);
    expect(rows(breakdown("by-instrument-title"))).toEqual(["BTC/USD$6,814.6880.8%", "XYZ$1,129.8413.4%", "QRS$488.255.8%"]);
    expect(rows(breakdown("by-class-title"))).toEqual(["Crypto$6,814.6880.8%", "US equities$1,618.0919.2%"]);
    expect(rows(breakdown("cash-invested-title"))).toEqual(["Agents' cash$16,632.7958.4%", "Invested$8,432.7729.6%", "Not managed by an agent$3,412.8012%"]);
    expect(within(breakdown("by-agent-title")).getAllByRole("link").map((l) => l.getAttribute("href"))).toEqual([AGENT_IDS.btc, AGENT_IDS.swing, AGENT_IDS.lmn].map((id) => `/agents/${id}`));
  });

  it("says plainly when there are no agents, and offers the one next step", () => {
    renderWithRuntime(<PortfolioPage />, "empty");
    expect(screen.getByRole("heading", { name: "No agents yet" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Set up an agent" })).toHaveAttribute("href", "/new");
    expect(screen.queryByRole("table")).toBeNull();
  });

  it("shows a skeleton while loading, never earlier values", () => {
    renderWithRuntime(<PortfolioPage />, "loading");
    expect(screen.getByLabelText("Loading")).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByText("$8,432.77")).toBeNull();
  });

  it("shows no figures when the deployment cannot be reached", () => {
    renderWithRuntime(<PortfolioPage />, "unreachable");
    expect(screen.getByRole("heading", { name: "Cannot reach your deployment" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Portfolio" })).toBeNull();
  });
});

const WS = "ws_01J9ZQ4M0000000000000000AB";
const ORIGIN = "https://api.owlhead.ai";

type Body = Record<string, unknown>;
const unavailable = () => new Response(JSON.stringify({ code: "unavailable", effect: "none", retryable: true }), { status: 503, headers: { "content-type": "application/json" } });

/**
 * The mock API, with one agent's positions served in full and one agent's read failing. `rewrite`
 * changes a read's body by its path, and `fail` answers a path `unavailable`; every request is kept.
 */
function mountApi({ rewrite, fail }: { rewrite?: (path: string, body: Body) => Body; fail?: (path: string) => boolean } = {}) {
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const requests: string[] = [];
  const fetch: Fetch = async (input, init) => {
    const url = new URL(input);
    const path = url.pathname;
    requests.push(`${path}${url.search}`);
    if (path.endsWith(`/agents/${AGENT_IDS.swing}/positions`) || fail?.(path)) return unavailable();
    const answer = await server.fetch(input, init);
    if (!answer.ok) return answer;
    let body = (await answer.json()) as Body;
    if (path.endsWith(`/agents/${AGENT_IDS.btc}/positions`)) {
      body.positions = (body.positions as Body[]).map((p) => ({
        ...p,
        cost_basis: { qty: "0.12", basis: "6666.666" },
        risk_mark: { bid: "56789.01", sane: true, session: "crypto", marked_at: "2026-10-30T14:00:00.000000000Z" },
        market_value: "6814.6812",
        unrealized_pnl: "148.0152",
      }));
    }
    if (rewrite) body = rewrite(path, body);
    return new Response(JSON.stringify(body), { status: 200, headers: answer.headers });
  };
  render(
    <RoleProvider initial="owner">
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        <PortfolioPage />
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
  return { requests };
}

const figure = (label: string) => within(section("Account figures")).getByText(label).nextElementSibling;
/** The account value the hero shows: its one plain copy, or the words for none. */
const heroValue = () => {
  const value = section("Account value").querySelector("[data-slot=account-equity-value]");
  return value?.querySelector("[data-slot=figure] > .sr-only")?.textContent ?? value?.textContent;
};

describe("the Portfolio page over the workspace API (DEC-2413 item 3)", () => {
  it("shows what the reads serve, and names as not reported what they do not, never a fixture figure", async () => {
    mountApi();
    const table = await screen.findByRole("table", { name: "Holdings" });
    await waitFor(() => expect(within(table).getByText("BTC/USD")).toBeInTheDocument());
    expect(screen.queryByText("Fixture data")).toBeNull();

    const btc = within(table).getByText("BTC/USD").closest("tr") as HTMLElement;
    const cells = within(btc).getAllByRole("cell");
    expect(cells.slice(0, 5).map((c) => c.textContent)).toEqual(["btc-accumulator", "0.12", "$55,555.5500", expect.stringContaining("$56,789.01"), "$6,814.68"]);
    expect(cells[5].querySelector(".sr-only")?.textContent).toBe("+$148.02");

    // The dashboard's snapshot: the broker's equity and cash, and the agents' day as its change.
    await waitFor(() => expect(heroValue()).toBe("$28,478.36"));
    await waitFor(() => expect(spoken(figure("Cash"))).toBe("$20,045.59"));
    await waitFor(() => expect(spoken(figure("Today"))).toBe("+$55.56 gain"));
    // Each agent's realized gross plus unrealized: (−24.56 + 148.01) + (−72.18 + 9.84 − 5.55) + 10.
    await waitFor(() => expect(spoken(figure("Total P&L, gross"))).toBe("+$65.56 gain"));
    expect(document.querySelector("[data-slot=pnl-gross]")).toHaveTextContent("Gross: before fees, which aren't attributed to each agent.");
    expect(within(section("Account figures")).queryByText("Total P&L")).toBeNull();

    // No read serves an agent's equity, so its cash and what no agent manages stay not reported.
    expect(figure("Agents' cash")?.textContent).toBe("not reported");
    expect(rows(breakdown("cash-invested-title"))).toContain("Not managed by an agentnot reported—");
  });

  it("draws the served history for the range picked, reading it again for each range", async () => {
    const { requests } = mountApi();
    const hero = await screen.findByRole("region", { name: "Account value" });
    const picker = within(hero).getByRole("group", { name: "Account value range" });
    expect(within(picker).getAllByRole("button").map((b) => b.textContent)).toEqual(["1D", "1W", "1M", "3M", "1Y", "All"]);
    await waitFor(() => expect(within(hero).getByRole("img", { name: "Account value, past 24 hours" })).toBeInTheDocument());
    expect(requests).toContain(`/v1/workspaces/${WS}/dashboard/history?range=1d`);
    within(picker).getByRole("button", { name: "1W" }).click();
    await waitFor(() => expect(requests).toContain(`/v1/workspaces/${WS}/dashboard/history?range=1w`));
    await waitFor(() => expect(within(hero).getByRole("img", { name: "Account value, past week" })).toBeInTheDocument());
    expect(within(hero).getByText("past week")).toBeInTheDocument();
  });

  it("never reads a missing snapshot, a version 1 snapshot's day or a null P&L as zero", async () => {
    mountApi({
      rewrite: (path, body) => {
        if (path.endsWith("/dashboard")) return { ...body, accounts: [...(body.accounts as Body[]), { account_ref: "acct_2", snapshot: null }] };
        if (path.endsWith(`/agents/${AGENT_IDS.lmn}/pnl`)) return { ...body, realized_gross: null };
        return body;
      },
    });
    await screen.findByRole("table", { name: "Holdings" });
    await waitFor(() => expect(heroValue()).toBe("Not reported"));
    for (const label of ["Cash", "Today", "Total P&L, gross"]) await waitFor(() => expect(figure(label)?.textContent, label).toBe("not reported"));
  });

  it("says so when the account's history cannot be read, and keeps the snapshot's value", async () => {
    mountApi({ fail: (path) => path.endsWith("/dashboard/history") });
    const hero = await screen.findByRole("region", { name: "Account value" });
    await waitFor(() => expect(hero.querySelector("[data-slot=history-not-reported]")).toHaveTextContent(/^Not reported: unavailable\.$/));
    await waitFor(() => expect(heroValue()).toBe("$28,478.36"));
    expect(within(hero).queryByRole("img")).toBeNull();
  });

  it("never sums or labels a live account as the paper workspace's: its value, cash, change and line are not reported", async () => {
    mountApi({
      rewrite: (path, body) => {
        if (!path.endsWith("/dashboard")) return body;
        const [paper] = body.accounts as Body[];
        const live = { account_ref: "acct_live", snapshot: { ...(paper.snapshot as Body), environment: "live" } };
        return { ...body, accounts: [paper, live] };
      },
    });
    await screen.findByRole("table", { name: "Holdings" });
    await waitFor(() => expect(heroValue()).toBe("Not reported"));
    for (const label of ["Cash", "Today"]) expect(figure(label)?.textContent, label).toBe("not reported");
    const hero = section("Account value");
    await waitFor(() => expect(hero.querySelector("[data-slot=history-not-reported]")).toHaveTextContent("Not reported: your accounts are not all in this environment."));
    expect(within(hero).queryByRole("img")).toBeNull();
  });

  it("draws a one-point line with no direction, and says one value was recorded, never no change", async () => {
    mountApi({
      rewrite: (path, body) => {
        if (!path.endsWith("/dashboard/history")) return body;
        const [account] = body.accounts as Array<{ points: Body[] }>;
        return { ...body, accounts: [{ ...account, points: account.points.slice(-1) }] };
      },
    });
    await screen.findByRole("img", { name: "Account value, past 24 hours" });
    expect(section("Account value").textContent).toMatch(/One account value recorded past 24 hours: \$[\d,]+\.\d\d, so no change to measure\. Simulated funds\./);
    expect(document.querySelector("[data-slot=hero-change]")).toBeNull();
  });

  it("names every figure not reported when the dashboard and P&L reads fail", async () => {
    mountApi({ fail: (path) => path.endsWith("/dashboard") || path.endsWith("/pnl") });
    await screen.findByRole("table", { name: "Holdings" });
    await waitFor(() => expect(heroValue()).toBe("Not reported"));
    for (const label of ["Cash", "Agents' cash", "Today", "Total P&L, gross"]) expect(figure(label)?.textContent, label).toBe("not reported");
  });

  it("says an agent's holdings were not reported when its read fails, and so shows no share", async () => {
    mountApi();
    const unread = await screen.findByRole("list", { name: "Holdings not read" });
    await waitFor(() => expect(within(unread).getByRole("alert")).toHaveTextContent(/^two-stock-swing: Not reported: \w+\.$/));
    expect(within(section("Account figures")).getByText("Invested").nextElementSibling?.textContent).toBe("not reported");
    expect(within(breakdown("by-agent-title")).getAllByText("—")).toHaveLength(3);
  });
});
