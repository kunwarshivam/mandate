import { expect, test } from "@playwright/test";

/**
 * Critique C-13: the Alerts screen's as-of times read as one column. Each feed's "as of" ends on the
 * same right edge as every other's, on a phone and on a desktop, whether or not a feed is degraded and
 * carries its state beside the time.
 */

for (const scenario of ["normal", "stale"]) {
  for (const width of [390, 1440]) {
    test(`${scenario}, ${width} px: the feeds' as-of times share one right edge`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(`/alerts?scenario=${scenario}`);
      const feeds = page.getByRole("region", { name: "Data and deployment" }).getByRole("listitem");
      await expect(feeds).toHaveCount(4);
      const cells = feeds.locator("[data-slot=as-of]");
      await expect(cells).toHaveCount(4);
      const rights = await cells.evaluateAll((els) => els.map((el) => el.getBoundingClientRect().right));
      const spread = Math.max(...rights) - Math.min(...rights);
      expect(spread, `right edges ${rights.join(", ")}`).toBeLessThanOrEqual(1);
    });
  }
}
