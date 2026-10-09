import { type Page, expect, test } from "@playwright/test";
import { buildWorkspace } from "../src/fixtures/workspace";

/**
 * C-14: a phone that cannot show what a position is worth is not a remote control. Below 64rem each
 * position is a two-line row (DEC-207), so its value and its unrealized P&L are on screen with no
 * sideways scroll, and the protection line no longer stretches the instrument into a ribbon. From
 * 64rem the table is unchanged. Each P&L keeps the disclosure symbol beside it (DEC-210), which
 * `disclosure.spec.ts` checks at these widths.
 */

const WS = buildWorkspace("normal");
const HOLDING = WS.agents.filter((a) => a.positions.length > 0);
const MONEY = /\$\d/;

/** An agent's section on the Positions screen, named by its heading. */
const section = (page: Page, agentId: string) => page.locator(`section[aria-labelledby="positions-${agentId}"]`);

async function open(page: Page, width: number, height = 844) {
  await page.setViewportSize({ width, height });
  await page.goto("/positions");
  await page.waitForLoadState("networkidle");
}

type RowRead = {
  symbol: string;
  figures: { text: string; left: number; right: number; top: number; bottom: number }[];
  pnl: { left: number; right: number; top: number; bottom: number; visible: boolean } | null;
  name: { top: number; bottom: number };
  height: number;
  lineHeight: number;
};

/**
 * Each visible position row in an agent's section, found by its instrument link whatever the markup:
 * every dollar figure in the row, the signed P&L, the instrument name's box, and the row's height in
 * lines of its own text.
 */
async function rows(page: Page, agentId: string): Promise<RowRead[]> {
  return section(page, agentId).evaluate((region, money) => {
    const pattern = new RegExp(money);
    const box = (el: Element) => {
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom };
    };
    const links = [...region.querySelectorAll<HTMLAnchorElement>("a[href*='/positions/']")].filter((a) => a.checkVisibility());
    return links.map((link) => {
      const row = link.closest("li, tr")!;
      const figures = [...row.querySelectorAll<HTMLElement>("*")]
        .filter((el) => el.checkVisibility() && !el.closest(".sr-only") && [...el.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && pattern.test(n.textContent ?? "")))
        .map((el) => ({ text: el.textContent ?? "", ...box(el) }));
      const signed = row.querySelector("[data-direction]");
      return {
        symbol: link.textContent?.trim() ?? "",
        figures,
        pnl: signed ? { ...box(signed), visible: signed.checkVisibility() } : null,
        name: box(link),
        height: row.getBoundingClientRect().height,
        lineHeight: parseFloat(getComputedStyle(row).lineHeight) || 20,
      };
    });
  }, MONEY.source);
}

test.describe("at 390px", () => {
  test("every position's figures, its value and its unrealized P&L among them, are inside the phone's width, with no sideways scroll", async ({ page }) => {
    await open(page, 390);
    const width = 390;
    for (const agent of HOLDING) {
      const read = await rows(page, agent.agent_id);
      expect(read.map((r) => r.symbol).sort(), agent.label).toEqual(agent.positions.map((p) => p.instrument.symbol).sort());
      for (const row of read) {
        expect(row.figures.length, `${row.symbol}: the mark, the value and the P&L are drawn`).toBeGreaterThanOrEqual(3);
        for (const f of row.figures) {
          expect(f.left, `${row.symbol} ${f.text}`).toBeGreaterThanOrEqual(0);
          expect(f.right, `${row.symbol} ${f.text}`).toBeLessThanOrEqual(width + 0.5);
        }
        expect(row.pnl?.visible, `${row.symbol}: the unrealized P&L is shown`).toBe(true);
        expect(row.pnl!.left).toBeGreaterThanOrEqual(0);
        expect(row.pnl!.right, `${row.symbol}: the unrealized P&L is on screen`).toBeLessThanOrEqual(width + 0.5);
      }
    }
    const scrolls = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
    expect(scrolls, "the page does not scroll sideways").toBe(false);
  });

  test("the value shares the instrument's line, and the protection line does not stretch the row into a ribbon", async ({ page }) => {
    await open(page, 390);
    for (const agent of HOLDING) {
      for (const row of await rows(page, agent.agent_id)) {
        const value = row.figures.filter((f) => f.top < row.name.bottom && f.bottom > row.name.top);
        expect(value.length, `${row.symbol}: a figure sits on the instrument's line`).toBeGreaterThan(0);
        expect(row.height / row.lineHeight, `${row.symbol}: the row is a few lines, not a ribbon`).toBeLessThan(7);
      }
    }
  });

  test("the unrealized note stays below each agent's positions, with its disclosure", async ({ page }) => {
    await open(page, 390);
    for (const agent of HOLDING) {
      const note = section(page, agent.agent_id).getByText("Unrealized paper P&L, simulated.");
      await expect(note).toBeVisible();
      await expect(note.getByRole("button", { name: "Performance disclosure" })).toBeVisible();
    }
  });
});

test("at 1440px the positions table is unchanged: its column headers are there and every row is in it", async ({ page }) => {
  await open(page, 1440, 900);
  for (const agent of HOLDING) {
    const own = section(page, agent.agent_id);
    const table = own.getByRole("table", { name: "Positions" });
    await expect(table).toBeVisible();
    for (const name of ["Instrument", "Quantity", "Mark", "Value", "Unrealized"]) await expect(table.getByRole("columnheader", { name, exact: true })).toBeVisible();
    for (const p of agent.positions) await expect(table.getByRole("rowheader", { name: new RegExp(`^${p.instrument.symbol.replace("/", "\\/")}`) })).toBeVisible();
    expect(await own.getByRole("link", { name: /./ }).filter({ visible: true }).count()).toBe(agent.positions.length + 1);
  }
});
