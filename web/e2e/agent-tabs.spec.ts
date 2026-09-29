import { expect, test } from "@playwright/test";
import { AGENT_IDS } from "../src/fixtures/workspace";
import { AGENT_SECTIONS, agentHref } from "../src/lib/screens";

/**
 * An agent's section tabs mark one tab at a time: on Orders, Orders is current and Overview is not.
 * The current tab's underline starts where its label starts, the first tab included.
 */

const TOP = AGENT_SECTIONS.filter((s) => !s.parent);

test.use({ viewport: { width: 1440, height: 900 } });

for (const section of TOP) {
  test(`${section.label}: only its own tab is current, underlined under its label`, async ({ page }) => {
    await page.goto(agentHref(AGENT_IDS.lmn, section.key));
    const nav = page.getByRole("navigation", { name: "Agent sections" });
    const current = nav.locator('[aria-current="page"]');
    await expect(current).toHaveCount(1);
    await expect(current).toHaveText(section.label);

    const gap = await current.evaluate((el) => {
      const range = document.createRange();
      range.selectNodeContents(el);
      const text = range.getBoundingClientRect();
      const box = el.getBoundingClientRect();
      const after = getComputedStyle(el, "::after");
      const left = box.left + parseFloat(after.left);
      const right = box.right - parseFloat(after.right);
      return { start: Math.abs(left - text.left), end: Math.abs(right - text.right) };
    });
    expect(gap.start).toBeLessThanOrEqual(1);
    expect(gap.end).toBeLessThanOrEqual(1);
  });
}
