import { type Page, expect, test } from "@playwright/test";

/**
 * C-17: at 1440px the dense screens started their content at 40px while every calm screen, and
 * the page's own "Fixture data" tag, started at 120px; moving from More to Audit made the left
 * edge jump, and the tag floated away from the column it labels. The fix keeps the dense
 * column's left edge on the calm page's and widens it only to the right, so the edge is
 * measured here as positions, not classes: the audit content starts exactly where its own tag
 * and every calm screen do, and the dense column reaches farther right than the calm one
 * does; at 390px the previous inset is 0 and the two densities coincide on the shared
 * gutters. The config's light and dark projects run this spec in both themes.
 */

/** A column's measured geometry: its box, where its content starts (padding in), the tag, the window. */
interface Reading {
  mainLeft: number;
  mainRight: number;
  contentLeft: number;
  tagLeft: number | null;
  pageWidth: number;
}

async function opened(page: Page, width: number, path: string): Promise<Reading> {
  await page.setViewportSize({ width, height: 900 });
  await page.goto(path);
  await page.waitForLoadState("networkidle");
  return page.evaluate(() => {
    const main = document.querySelector<HTMLElement>("main")!;
    const box = main.getBoundingClientRect();
    const tag = document.querySelector<HTMLElement>("[data-slot=fixture-tag]");
    return {
      mainLeft: box.left,
      mainRight: box.right,
      contentLeft: box.left + parseFloat(getComputedStyle(main).paddingLeft),
      tagLeft: tag && tag.checkVisibility() ? tag.getBoundingClientRect().left : null,
      pageWidth: window.innerWidth,
    };
  });
}

/** Sub-pixel rendering aside, the two edges are one edge. */
function sameEdge(got: number, want: number, what: string) {
  expect(Math.abs(got - want), `${what}: drew ${got}, wanted ${want}`).toBeLessThanOrEqual(0.5);
}

test("at 1440px the dense audit content and the Fixture data tag share the calm page's left edge, and the dense column reaches farther right than the calm one", async ({ page }) => {
  const audit = await opened(page, 1440, "/audit");
  /** The calm column's own left edge: its centred 80rem bound plus the shared 40px gutter — the row's 120px. */
  const calmEdge = (audit.pageWidth - 1280) / 2 + 40;
  sameEdge(audit.contentLeft, calmEdge, "the audit content keeps the calm page's left edge");
  expect(audit.tagLeft, "the tag is still drawn at the foot of the dense screen").not.toBeNull();
  sameEdge(audit.tagLeft!, calmEdge, "the unmoved tag sits back over the column it labels");
  const home = await opened(page, 1440, "/");
  sameEdge(home.contentLeft, calmEdge, "every calm screen starts at the same edge");
  sameEdge(audit.mainLeft, home.mainLeft, "the dense column's own left edge is the calm one's");
  expect(audit.mainRight, "the dense column only widens to the right").toBeGreaterThan(home.mainRight);
});

test("at 390px the two densities coincide on the shared gutters", async ({ page }) => {
  const audit = await opened(page, 390, "/audit");
  const home = await opened(page, 390, "/");
  expect(audit.mainLeft, "the inset is 0 below the calm bound, so both columns start at the window").toBe(home.mainLeft);
  sameEdge(audit.contentLeft, home.contentLeft, "the shared gutters hold on both densities");
  expect(audit.tagLeft, "the tag is drawn on the phone too").not.toBeNull();
  sameEdge(audit.tagLeft!, home.contentLeft, "the tag shares the phone's edge, as every screen does");
});
