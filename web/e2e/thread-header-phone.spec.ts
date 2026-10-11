import { type Page, expect, test } from "@playwright/test";
import { twoStockSwing } from "../src/fixtures/mandates";
import { AGENT_IDS } from "../src/fixtures/workspace";

/**
 * C-16: at 390px the thread header's one row packed the mandate's slug in with the back link, the
 * owl, the agent's name, the mode chip, the other view and the paper badge. The register's fix
 * drops the slug alone below the bar's own `lg` boundary, since the owl and the name already say
 * which agent it is (DEC-207: laid out by CSS, no script query), so at 1440px the header keeps
 * drawing it. `toBeVisible` and `toBeHidden` read the browser's computed layout, so the slug is
 * proven drawn, not merely present, at each width. The config's light and dark projects run this
 * spec in both themes.
 */

const THREAD = `/messages/${AGENT_IDS.swing}?scenario=approvals`;

async function open(page: Page, width: number, height = 844) {
  await page.setViewportSize({ width, height });
  await page.goto(THREAD);
  await page.waitForLoadState("networkidle");
}

const bar = (page: Page) => page.locator("[data-slot=thread-bar]");

/** The slug: the mandate's name under the agent's, the only `translate="no"` span in the bar. */
const slug = (page: Page) => bar(page).locator("span[translate=no]");

test("at 390px the header drops the slug alone: the back link, owl, name, mode chip, other view and paper badge stay on the row", async ({ page }) => {
  await open(page, 390);
  await expect(slug(page)).toHaveCount(1);
  await expect(slug(page)).toContainText(twoStockSwing.name);
  await expect(slug(page), "the slug is not drawn on the phone").toBeHidden();
  await expect(bar(page).locator("[data-slot=owl]"), "the owl still says which agent it is").toBeVisible();
  await expect(bar(page).getByRole("link", { name: /^Agent 2/ }), "so does the name").toBeVisible();
  await expect(bar(page).locator("[data-slot=mode-badge]").locator("visible=true"), "the mode chip stays").toBeVisible();
  await expect(bar(page).getByRole("link", { name: "Back to Messages" }), "the way back stays").toBeVisible();
  const other = bar(page).getByRole("link", { name: "Desk" });
  await expect(other, "the other view stays at its own address").toBeVisible();
  await expect(other).toHaveAttribute("href", `/messages/${AGENT_IDS.swing}/desk`);
  await expect(bar(page).locator("[data-slot=environment-badge]"), "the paper badge stays").toBeVisible();
});

test("at 1440px the header keeps the slug, visibly: the desktop draws it as it did", async ({ page }) => {
  await open(page, 1440, 900);
  await expect(slug(page)).toHaveCount(1);
  await expect(slug(page), "the slug is actually visible at desktop width, not merely present").toBeVisible();
  await expect(slug(page)).toContainText(twoStockSwing.name);
});
