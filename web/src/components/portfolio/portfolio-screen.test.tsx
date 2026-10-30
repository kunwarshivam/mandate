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

/** The mock API, with one agent's positions served in full and one agent's read failing. */
function mountApi() {
  const server = createMockServer({ workspaceId: WS, scenario: "normal" });
  const fetch: Fetch = async (input, init) => {
    const path = new URL(input).pathname;
    if (path.endsWith(`/agents/${AGENT_IDS.swing}/positions`)) return new Response(JSON.stringify({ code: "unavailable", effect: "none", retryable: true }), { status: 503, headers: { "content-type": "application/json" } });
    const answer = await server.fetch(input, init);
    if (!path.endsWith(`/agents/${AGENT_IDS.btc}/positions`)) return answer;
    const body = (await answer.json()) as { positions: Array<Record<string, unknown>> };
    body.positions = body.positions.map((p) => ({
      ...p,
      cost_basis: { qty: "0.12", basis: "6666.666" },
      risk_mark: { bid: "56789.01", sane: true, session: "crypto", marked_at: "2026-10-30T14:00:00.000000000Z" },
      market_value: "6814.6812",
      unrealized_pnl: "148.0152",
    }));
    return new Response(JSON.stringify(body), { status: 200, headers: answer.headers });
  };
  render(
    <RoleProvider initial="owner">
      <ApiRuntimeProvider source={{ origin: ORIGIN, workspaceId: WS }} fetch={fetch} refreshMs={60_000} statusPollMs={5} retryMs={5}>
        <PortfolioPage />
      </ApiRuntimeProvider>
    </RoleProvider>,
  );
}

describe("the Portfolio page over the workspace API (DEC-2413 item 3)", () => {
  it("shows what the reads serve, and names as not reported what they do not, never a fixture figure", async () => {
    mountApi();
    const table = await screen.findByRole("table", { name: "Holdings" });
    await waitFor(() => expect(within(table).getByText("BTC/USD")).toBeInTheDocument());
    expect(screen.queryByText("Fixture data")).toBeNull();
    expect(within(section("Account value")).getByText("Not reported")).toBeInTheDocument();
    expect(document.querySelector("[data-slot=history-not-reported]")).not.toBeNull();
    expect(screen.queryByRole("group", { name: "Account value range" })).toBeNull();

    const btc = within(table).getByText("BTC/USD").closest("tr") as HTMLElement;
    const cells = within(btc).getAllByRole("cell");
    expect(cells.slice(0, 5).map((c) => c.textContent)).toEqual(["btc-accumulator", "0.12", "$55,555.5500", expect.stringContaining("$56,789.01"), "$6,814.68"]);
    expect(cells[5].querySelector(".sr-only")?.textContent).toBe("+$148.02");

    const figures = section("Account figures");
    for (const label of ["Agents' cash", "Today", "Total P&L"]) expect(within(figures).getByText(label).nextElementSibling?.textContent).toBe("not reported");
  });

  it("says an agent's holdings were not reported when its read fails, and so shows no share", async () => {
    mountApi();
    const unread = await screen.findByRole("list", { name: "Holdings not read" });
    await waitFor(() => expect(within(unread).getByRole("alert")).toHaveTextContent(/^two-stock-swing: Not reported: \w+\.$/));
    expect(within(section("Account figures")).getByText("Invested").nextElementSibling?.textContent).toBe("not reported");
    expect(within(breakdown("by-agent-title")).getAllByText("—")).toHaveLength(3);
  });
});
