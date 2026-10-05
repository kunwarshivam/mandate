import { expect, test } from "@playwright/test";
import { APPROVAL_IDS } from "../src/fixtures/workspace";

/**
 * Phones keep the full status strip on a record screen (DEC-207), where it scrolls sideways. While items lie past its right edge a flat "+N" cue
 * sits at that edge; pressing it scrolls the strip, and it goes once the last item is in view. It
 * overlays the strip, so it moves nothing, never paints a gradient, and never covers Stop.
 */

const PHONES = [320, 390, 430];
const RECORD = `/approvals/${APPROVAL_IDS.swingXyz}`;

for (const scenario of ["normal", "stale"]) {
  for (const width of PHONES) {
    test(`${scenario}, ${width} px: the strip shows how many items lie past its edge, and scrolls to them`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.goto(`${RECORD}?scenario=${scenario}`);
      const strip = page.locator("[data-slot=status-strip]");
      await expect(page.locator("[data-slot=status-strip][data-degraded]")).toHaveCount(scenario === "normal" ? 0 : 1);
      const more = page.locator("[data-slot=status-more]");
      await expect(more).toBeVisible();
      await expect(more).toHaveText(/^\+\d+$/);

      const box = (await more.boundingBox())!;
      const stripBox = (await strip.boundingBox())!;
      expect(box.x + box.width).toBeCloseTo(stripBox.x + stripBox.width, 0);
      expect(box.y).toBeGreaterThanOrEqual(stripBox.y - 0.5);
      expect(box.y + box.height).toBeLessThanOrEqual(stripBox.y + stripBox.height + 0.5);

      const style = await more.evaluate((el) => {
        const s = getComputedStyle(el);
        const parent = getComputedStyle(el.parentElement!);
        const card = getComputedStyle(document.body);
        return { image: s.backgroundImage, mask: s.maskImage, background: s.backgroundColor, behind: parent.backgroundColor, card: card.backgroundColor };
      });
      expect(style.image).toBe("none");
      expect(style.mask).toBe("none");
      expect(style.background).toBe(style.behind === "rgba(0, 0, 0, 0)" ? style.card : style.behind);

      const stop = page.getByRole("navigation", { name: "Main" }).getByRole("button", { name: "Stop", exact: true });
      const stopBox = (await stop.boundingBox())!;
      expect(stopBox.y, "Stop sits on the tab bar, below the strip's cue").toBeGreaterThanOrEqual(box.y + box.height - 0.5);

      const before = Number((await more.textContent())!.slice(1));
      await more.click();
      await expect.poll(() => strip.evaluate((el) => el.scrollLeft)).toBeGreaterThan(0);
      await expect.poll(async () => ((await more.count()) ? Number((await more.textContent())!.slice(1)) : 0)).toBeLessThan(before);

      await strip.evaluate((el) => el.scrollTo({ left: el.scrollWidth, behavior: "instant" }));
      await expect(more).toHaveCount(0);
      await expect(stop).toBeVisible();
    });
  }
}

test("wider screens clip the strip instead and show no cue", async ({ page }) => {
  await page.setViewportSize({ width: 820, height: 800 });
  await page.goto(`${RECORD}?scenario=stale`);
  await expect(page.locator("[data-slot=status-strip][data-degraded]")).toHaveCount(1);
  await expect(page.locator("[data-slot=status-more]")).toBeHidden();
});
