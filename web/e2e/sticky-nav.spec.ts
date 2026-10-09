import { type Page, expect, test } from "@playwright/test";
import { AGENT_IDS } from "../src/fixtures/workspace";
import { agentHref } from "../src/lib/screens";

/**
 * The desktop dock stays put: however far a long page scrolls, it floats centred at the bottom of the
 * viewport with every item on screen, the current page's item among them, and the page's last line
 * ends above it, with Stop at its end wholly on screen. The header stays at the top. Below the 1024 px breakpoint
 * the tab bar and its More sheet carry the nav instead.
 */

const WIDTHS = [1024, 1280, 1920];
const HEIGHT = 800;
const ROUTE = agentHref(AGENT_IDS.swing, "overview");

async function scrollToBottom(page: Page) {
  const { scrollHeight, innerHeight } = await page.evaluate(() => ({
    scrollHeight: document.documentElement.scrollHeight,
    innerHeight: window.innerHeight,
  }));
  expect(scrollHeight, "the page is long enough to scroll well past the dock").toBeGreaterThan(innerHeight * 1.5);
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await page.waitForFunction(() => Math.ceil(window.scrollY + window.innerHeight) >= document.documentElement.scrollHeight - 1);
}

test.describe("The desktop dock stays pinned while a long page scrolls", () => {
  for (const width of WIDTHS) {
    test(`${width} px`, async ({ page }) => {
      await page.setViewportSize({ width, height: HEIGHT });
      await page.goto(ROUTE);
      await page.waitForLoadState("networkidle");
      await scrollToBottom(page);

      const dock = page.getByRole("navigation", { name: "Primary" });
      const found = await dock.evaluate((el) => {
        const r = el.getBoundingClientRect();
        const main = document.getElementById("main")!;
        const last = [...main.querySelectorAll("*")].filter((n) => n.getBoundingClientRect().height > 0).at(-1)!;
        return {
          box: { top: r.top, bottom: r.bottom, left: r.left, right: r.right },
          scrollY: window.scrollY,
          position: getComputedStyle(el.parentElement!).position,
          paintsCentre: el.contains(document.elementFromPoint(r.left + r.width / 2, r.top + 2)),
          lastBottom: last.getBoundingClientRect().bottom,
        };
      });
      expect(found.scrollY, "the page scrolled").toBeGreaterThan(0);
      expect(found.position, "the dock is fixed to the viewport").toBe("fixed");
      expect(found.box.bottom, "the dock's bottom edge sits above the viewport's").toBeLessThanOrEqual(HEIGHT - 12);
      expect(found.box.bottom, "and close to it").toBeGreaterThanOrEqual(HEIGHT - 20);
      expect(Math.abs((found.box.left + found.box.right) / 2 - width / 2), "the dock is centred").toBeLessThanOrEqual(1);
      expect(found.paintsCentre, "the dock paints over the page").toBe(true);
      expect(found.lastBottom, "the page's last line ends above the dock").toBeLessThanOrEqual(found.box.top);

      const items = dock.locator("a[href], button");
      expect(await items.count(), "six links, the More menu, and Stop (DEC-513 item 2)").toBe(8);
      for (const item of await items.all()) await expect(item).toBeInViewport({ ratio: 1 });
      await expect(dock.getByRole("link", { name: "Agents", exact: true }), "the current page's item").toHaveAttribute("aria-current", "page");

      const header = await page.locator("header").first().boundingBox();
      expect(header?.y, "the header stays at the top").toBe(0);
      expect(found.box.top, "the dock stays clear of the header").toBeGreaterThan(header!.y + header!.height);
      await expect(dock.getByRole("button", { name: "Stop", exact: true }), "Stop").toBeInViewport({ ratio: 1 });
      await expect(page.locator("[data-sidebar]"), "no sidebar").toHaveCount(0);
    });
  }
});
