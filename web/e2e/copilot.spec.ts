import { type Page, expect, test } from "@playwright/test";
import { AGENT_IDS } from "../src/fixtures/workspace";

/**
 * Owlhead (DEC-476 item 8) never hides the frame's safety: below 110rem it floats between the
 * header and the dock, so the paper badge and Stop stay whole; from 110rem it docks and the page and
 * the dock make room; on a phone it is a sheet between the header and the tab bar.
 */

const panel = (page: Page) => page.getByRole("complementary", { name: "Owlhead" });

async function open(page: Page, path: string, width: number, height: number) {
  await page.setViewportSize({ width, height });
  await page.goto(path);
  await page.waitForLoadState("networkidle");
}

type Box = { x: number; y: number; width: number; height: number };
const overlaps = (a: Box, b: Box) => a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

for (const [width, height] of [
  [1024, 768],
  [1440, 900],
  [1920, 1080],
] as const) {
  test(`${width} px: Cmd+J opens it clear of the header and the dock, and the page never scrolls sideways`, async ({ page }) => {
    await open(page, "/", width, height);
    await page.keyboard.press("ControlOrMeta+j");
    await expect(panel(page)).toBeVisible();
    await expect(panel(page).getByRole("textbox", { name: "Ask Owlhead" })).toBeFocused();
    const box = (await panel(page).boundingBox())!;
    const dock = (await page.getByRole("navigation", { name: "Primary" }).boundingBox())!;
    const badge = (await page.getByRole("banner").locator("[data-slot=environment-badge]").boundingBox())!;
    expect(overlaps(box, dock), "the dock and Stop stay whole").toBe(false);
    expect(overlaps(box, badge), "the paper badge stays whole").toBe(false);
    await expect(page.getByRole("navigation", { name: "Primary" }).getByRole("button", { name: "Stop", exact: true })).toBeInViewport({ ratio: 1 });
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    if (width >= 1760) {
      const main = (await page.locator("main").boundingBox())!;
      expect(main.x + main.width, "the page makes room for the docked panel").toBeLessThanOrEqual(box.x + 1);
    }
    await page.keyboard.press("Escape");
    await expect(panel(page)).toHaveCount(0);
  });
}

test("on an agent's page it answers about that agent, from the record", async ({ page }) => {
  await open(page, `/agents/${AGENT_IDS.swing}`, 1440, 900);
  await page.getByRole("button", { name: "Ask Owlhead" }).click();
  await expect(panel(page).locator("[data-slot=looking-at]")).toContainText("Agent 2");
  const field = panel(page).getByRole("textbox", { name: "Ask Owlhead" });
  await field.fill("How close is it to its limits?");
  await field.press("Enter");
  const answer = panel(page).locator("[data-slot=record-answer]");
  await expect(answer).toContainText("From the record");
  await expect(answer).toContainText(/Daily loss limit: \$[\d,]+\.\d{2} headroom/);
  await expect(answer).not.toContainText(/Agent 1|Agent 3/);
});

for (const width of [320, 390] as const) {
  test(`${width} px: from Messages it opens as a sheet between the header and the tab bar, with Stop pressable`, async ({ page }) => {
    await open(page, "/messages", width, 844);
    await page.locator("[data-slot=copilot-row]").click();
    await expect(panel(page)).toBeVisible();
    const box = (await panel(page).boundingBox())!;
    const header = (await page.getByRole("banner").boundingBox())!;
    const tabs = (await page.getByRole("navigation", { name: "Main" }).boundingBox())!;
    expect(box.y).toBeGreaterThanOrEqual(header.y + header.height - 1);
    expect(box.y + box.height).toBeLessThanOrEqual(tabs.y + 1);
    await expect(page.getByRole("navigation", { name: "Main" }).getByRole("button", { name: "Stop", exact: true })).toBeInViewport({ ratio: 1 });
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    await panel(page).getByRole("button", { name: "Close Owlhead" }).click();
    await expect(panel(page)).toHaveCount(0);
  });
}
