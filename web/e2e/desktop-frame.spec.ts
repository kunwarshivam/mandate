import { type Page, expect, test } from "@playwright/test";
import { recordHref } from "../src/components/stop/commands";
import { AGENT_IDS, APPROVAL_IDS } from "../src/fixtures/workspace";

/**
 * From 64rem, as on a phone (DEC-215, DEC-207): nothing under the header while every feed answers,
 * and the "Fixture data" tag at the foot of the page, clear of the dock. A stale or failing feed, or
 * a record screen, shows the full status strip under the header instead.
 */

const strip = (page: Page) => page.locator("[data-slot=status-strip]");
const foot = (page: Page) => page.locator("[data-slot=page-footer]");

async function open(page: Page, path: string, width = 1440, height = 900) {
  await page.setViewportSize({ width, height });
  await page.goto(path);
  await page.waitForLoadState("networkidle");
}

for (const width of [1024, 1280, 1440]) {
  test(`${width} px, every feed answering: nothing under the header, and the fixture tag at the foot, clear of the dock`, async ({ page }) => {
    await open(page, "/?scenario=normal", width);
    await expect(strip(page)).toHaveCount(0);
    await expect(page.locator("[data-slot=wire]")).toHaveCount(0);
    await expect(page.locator("[data-slot=feed-banner]")).toHaveCount(0);
    const header = (await page.getByRole("banner").boundingBox())!;
    const main = (await page.locator("#main").boundingBox())!;
    expect(main.y, "the page starts under the header").toBe(header.y + header.height);
    const scrollPadding = parseFloat(await page.evaluate(() => getComputedStyle(document.documentElement).scrollPaddingTop));
    expect(scrollPadding, "an anchored or focused element lands clear of the header").toBeGreaterThanOrEqual(header.height);

    await page.evaluate(() => window.scrollTo({ top: document.documentElement.scrollHeight, behavior: "instant" }));
    const tag = foot(page).getByText("Fixture data");
    await expect(tag).toBeInViewport({ ratio: 1 });
    const dock = (await page.getByRole("navigation", { name: "Primary" }).boundingBox())!;
    expect((await tag.boundingBox())!.y + (await tag.boundingBox())!.height, "the tag ends above the dock").toBeLessThan(dock.y);
  });
}

test("a stale feed shows the full status strip under the header, 34 px, and moves the tag into it", async ({ page }) => {
  await open(page, "/?scenario=stale", 1280);
  const degraded = page.locator("[data-slot=status-strip][data-degraded]");
  await expect(degraded).toBeVisible();
  expect((await degraded.boundingBox())!.height).toBe(34);
  const header = (await page.getByRole("banner").boundingBox())!;
  expect((await degraded.boundingBox())!.y).toBe(header.y + header.height);
  await expect(degraded.getByText("Fixture data")).toBeVisible();
  await expect(foot(page)).toBeHidden();
});

for (const [name, path] of [
  ["an approval request", `/approvals/${APPROVAL_IDS.swingXyz}`],
  ["the kill-switch record", recordHref("kill", AGENT_IDS.btc)],
] as const) {
  test(`${name} keeps the status strip while every feed answers`, async ({ page }) => {
    await open(page, path);
    await expect(strip(page)).toBeVisible();
    await expect(strip(page)).not.toHaveAttribute("data-degraded");
  });
}

test("tabbing through a long page, no focused control sits under the header", async ({ page }) => {
  await open(page, "/agents", 1440, 700);
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  const bottom = await page.getByRole("banner").evaluate((el) => el.getBoundingClientRect().bottom);
  await page.locator("#main").focus();
  const covered: string[] = [];
  for (let i = 0; i < 40; i++) {
    await page.keyboard.press("Shift+Tab");
    const hit = await page.evaluate(() => {
      const el = document.activeElement as HTMLElement | null;
      if (!el || !document.getElementById("main")?.contains(el)) return null;
      return { top: el.getBoundingClientRect().top, label: `${el.tagName} ${el.textContent?.trim().slice(0, 40)}` };
    });
    if (hit && hit.top < bottom) covered.push(hit.label);
  }
  expect(covered).toEqual([]);
});
