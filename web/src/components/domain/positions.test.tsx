import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { buildWorkspace } from "@/fixtures/workspace";
import { price, quantity, signedUsd, usd } from "@/lib/format";
import { PositionsTable, protectionText } from "./positions";

/**
 * C-14: below 64rem each position is a two-line row, from 64rem the table (DEC-207). CSS picks which
 * one shows, so both are rendered; `e2e/positions-phone.spec.ts` checks what a phone and a desktop
 * actually draw. Here: each row says what each figure is, and each P&L keeps its disclosure symbol
 * in its own row (DEC-210).
 */

const WS = buildWorkspace("normal");
const AGENT = WS.agents.find((a) => a.positions.length > 1)!;
const NOW = AGENT.positions[0].mark_as_of;

function renderTable() {
  return render(<PositionsTable positions={AGENT.positions} now={NOW} staleSymbols={new Set()} hrefFor={(p) => `/positions/${p.instrument.asset_id}`} />);
}

describe("positions on a phone", () => {
  it("gives each position one row, hidden from 64rem, beside a table shown only from 64rem", () => {
    renderTable();
    const list = screen.getByRole("list", { name: "Positions" });
    expect(list).toHaveClass("lg:hidden");
    expect(within(list).getAllByRole("listitem")).toHaveLength(AGENT.positions.length);
    expect(screen.getByRole("table", { name: "Positions" })).toHaveClass("max-lg:hidden");
  });

  it("names every figure in a row: the instrument, value, quantity, mark, unrealized P&L and protection", () => {
    renderTable();
    const rows = within(screen.getByRole("list", { name: "Positions" })).getAllByRole("listitem");
    rows.forEach((row, i) => {
      const p = AGENT.positions[i];
      expect(within(row).getByRole("link", { name: p.instrument.symbol })).toHaveAttribute("href", `/positions/${p.instrument.asset_id}`);
      const text = row.textContent ?? "";
      expect(text).toContain(`Value ${usd(p.market_value)}`);
      expect(text).toContain(`Quantity ${quantity(p.qty)} at a mark of ${price(p.mark)}`);
      expect(text).toContain(`Unrealized ${signedUsd(p.unrealized_pnl)}`);
      expect(text).toContain(`Protection: ${protectionText(p)}`);
    });
  });

  it("keeps the disclosure symbol beside each row's P&L, described by the disclosure text in that row", () => {
    renderTable();
    const rows = within(screen.getByRole("list", { name: "Positions" })).getAllByRole("listitem");
    for (const row of rows) {
      const pnl = row.querySelector("[data-direction]")!;
      const trigger = within(row).getByRole("button", { name: "Performance disclosure" });
      expect(trigger).toHaveAccessibleDescription("[[DISCLOSURE-PERFORMANCE]]");
      expect(pnl.parentElement!.contains(trigger), "the symbol sits with the figure, not elsewhere in the row").toBe(true);
    }
  });

  it("keeps the unrealized note and its disclosure under both", () => {
    renderTable();
    const note = screen.getByText(/^Unrealized paper P&L, simulated\./);
    expect(within(note).getByRole("button", { name: "Performance disclosure" })).toBeInTheDocument();
  });
});
