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
      range.selectNodeContents(el.firstChild!);
      const text = range.getBoundingClientRect();
      const line = el.querySelector("[data-slot=tab-underline]")!.getBoundingClientRect();
      return { start: Math.abs(line.left - text.left), end: Math.abs(line.right - text.right) };
    });
    expect(gap.start).toBeLessThanOrEqual(1);
    expect(gap.end).toBeLessThanOrEqual(1);
  });
}

/**
 * C-8: on a tab other than Overview the header still says what the agent may do now, beside its
 * name (DEC-512), and says it once.
 */
for (const { scenario, agentId, mode } of [
  { scenario: "normal", agentId: AGENT_IDS.swing, mode: "Trading" },
  { scenario: "drawdown", agentId: AGENT_IDS.btc, mode: "Selling only" },
  { scenario: "paused", agentId: AGENT_IDS.swing, mode: "Paused" },
]) {
  test(`Decisions in ${scenario}: the header says ${mode} beside the agent's name`, async ({ page }) => {
    await page.goto(`${agentHref(agentId, "decisions")}?scenario=${scenario}`);
    const main = page.getByRole("main");
    const title = main.getByRole("heading", { level: 1 });
    const chip = main.getByText(mode, { exact: true }).filter({ visible: true });
    await expect(title).toBeVisible();
    await expect(chip).toHaveCount(1);
    const [name, beside] = await Promise.all([title.boundingBox(), chip.boundingBox()]);
    const middle = beside!.y + beside!.height / 2;
    expect(middle).toBeGreaterThan(name!.y);
    expect(middle).toBeLessThan(name!.y + name!.height);
    expect(beside!.x).toBeGreaterThan(name!.x + name!.width);
  });
}
