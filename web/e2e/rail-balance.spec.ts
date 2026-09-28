import { expect, test } from "@playwright/test";
import { AGENT_IDS } from "../src/fixtures/workspace";

/**
 * A side rail never runs on long after the story beside it has ended. Either its bottom sits no more
 * than about one viewport below the main column's bottom, or it is sticky and shorter than the
 * viewport, so it stays in view the whole way down.
 */

const WIDTHS = [1280, 1440, 1920];
const HEIGHT = 800;

const ROUTES = [
  { name: "home", path: "/" },
  ...Object.entries(AGENT_IDS).map(([name, id]) => ({ name: `agent ${name}`, path: `/agents/${id}` })),
];

for (const { name, path } of ROUTES) {
  for (const width of WIDTHS) {
    test(`${name} at ${width} px: the rail ends with the story or stays in view`, async ({ page }) => {
      await page.setViewportSize({ width, height: HEIGHT });
      await page.goto(path, { waitUntil: "networkidle" });
      await expect(page.locator("main [data-layout=rail]")).toHaveCount(1);
      await page.evaluate(() => document.fonts.ready);

      const layout = await page.evaluate(() => {
        /** Where a column's content ends: a grid item stretches to its row, so its own box would not tell. */
        const contentBottom = (el: Element) => Math.max(...[...el.children].map((c) => c.getBoundingClientRect().bottom + window.scrollY));
        const rail = document.querySelector("main [data-layout=rail]")!;
        const main = [...document.querySelectorAll("main [data-layout=main]")];
        const top = rail.getBoundingClientRect().top + window.scrollY;
        return {
          viewport: window.innerHeight,
          railBottom: contentBottom(rail),
          railHeight: contentBottom(rail) - top,
          sticky: getComputedStyle(rail).position === "sticky",
          mainBottom: Math.max(...main.map(contentBottom)),
          mainCount: main.length,
        };
      });

      expect(layout.mainCount, "the page marks its main column").toBeGreaterThan(0);
      const overrun = layout.railBottom - layout.mainBottom;
      const staysInView = layout.sticky && layout.railHeight < layout.viewport;
      expect(
        overrun <= layout.viewport || staysInView,
        `rail ends ${Math.round(overrun)} px below the main column (viewport ${layout.viewport} px); rail ${Math.round(layout.railHeight)} px tall, ${layout.sticky ? "sticky" : "not sticky"}`,
      ).toBe(true);
    });
  }
}
