import { type Locator, type Page, expect, test } from "@playwright/test";

/**
 * The frame is frosted glass: the header, and the tab bar on phones, sit over the page as it
 * scrolls and blur what passes underneath. Nothing else is glass. Where the owner asks for less
 * transparency, or in forced colours, the frame is the solid card.
 */

const BLUR = "blur(22px) saturate(1.8)";

async function tokenColor(page: Page, token: string): Promise<string> {
  return page.evaluate((token) => {
    const probe = document.createElement("div");
    probe.style.backgroundColor = `var(${token})`;
    document.body.append(probe);
    const value = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return value;
  }, token);
}

async function paint(locator: Locator) {
  return locator.evaluate((el) => {
    const style = getComputedStyle(el);
    return { background: style.backgroundColor, filter: style.backdropFilter, image: style.backgroundImage };
  });
}

/** Whether the page's own content lies under the middle of an element, so the element sits over the scrolling page. */
async function contentUnder(locator: Locator): Promise<boolean> {
  return locator.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const main = document.getElementById("main")!;
    const m = main.getBoundingClientRect();
    return document.elementsFromPoint(m.left + m.width / 2, r.top + r.height / 2).some((hit) => hit !== main && main.contains(hit));
  });
}

async function scrollBy(page: Page, y: number) {
  await page.evaluate((y) => window.scrollTo({ top: y, behavior: "instant" }), y);
  await page.waitForFunction((y) => Math.abs(window.scrollY - y) < 1, y);
}

const header = (page: Page) => page.getByRole("banner");
const tabBar = (page: Page) => page.locator("nav[aria-label=Main].grid");

test.describe("the frame is frosted glass over the scrolling page", () => {
  test("desktop, 1440 x 900: the header", async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto("/");
    await page.waitForLoadState("networkidle");
    const glass = await tokenColor(page, "--glass");
    expect(glass).not.toBe(await tokenColor(page, "--card"));
    expect(await paint(header(page))).toEqual({ background: glass, filter: BLUR, image: "none" });

    await scrollBy(page, 330);
    expect((await header(page).boundingBox())?.y, "the header stays at the top").toBe(0);
    expect(await contentUnder(header(page)), "the page passes under the header").toBe(true);
    await expect(page.getByRole("button", { name: "Stop", exact: true })).toBeInViewport({ ratio: 1 });
  });

  test("phone, 390 x 844: the header and the tab bar", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/");
    await page.waitForLoadState("networkidle");
    const glass = await tokenColor(page, "--glass");
    for (const frame of [header(page), tabBar(page)]) expect(await paint(frame)).toEqual({ background: glass, filter: BLUR, image: "none" });

    await scrollBy(page, 420);
    expect((await header(page).boundingBox())?.y, "the header stays at the top").toBe(0);
    const bar = (await tabBar(page).boundingBox())!;
    expect(bar.y + bar.height, "the tab bar stays at the bottom").toBeCloseTo(844, 0);
    expect(await contentUnder(header(page)), "the page passes under the header").toBe(true);
    expect(await contentUnder(tabBar(page)), "the page passes under the tab bar").toBe(true);
    await expect(page.getByRole("button", { name: "Stop", exact: true })).toBeInViewport({ ratio: 1 });
  });

  test("nothing but the frame is glass", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/");
    await page.waitForLoadState("networkidle");
    const frosted = await page.evaluate(() =>
      [...document.querySelectorAll("*")].filter((el) => getComputedStyle(el).backdropFilter !== "none").map((el) => el.tagName.toLowerCase()),
    );
    expect(frosted).toEqual(["header", "nav"]);
  });
});

test.describe("the frame is the solid card where transparency is unwanted", () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test("prefers-reduced-transparency", async ({ page }, testInfo) => {
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Emulation.setEmulatedMedia", {
      features: [
        { name: "prefers-color-scheme", value: testInfo.project.use.colorScheme === "dark" ? "dark" : "light" },
        { name: "prefers-reduced-transparency", value: "reduce" },
      ],
    });
    await page.goto("/");
    await page.waitForLoadState("networkidle");
    test.skip(
      !(await page.evaluate(() => matchMedia("(prefers-reduced-transparency: reduce)").matches)),
      "this Chromium cannot emulate prefers-reduced-transparency; src/lib/tokens.test.ts checks the fallback in the CSS",
    );
    const card = await tokenColor(page, "--card");
    for (const frame of [header(page), tabBar(page)]) expect(await paint(frame)).toEqual({ background: card, filter: "none", image: "none" });
  });

  test("forced colours", async ({ page }) => {
    await page.emulateMedia({ forcedColors: "active" });
    await page.goto("/");
    await page.waitForLoadState("networkidle");
    for (const frame of [header(page), tabBar(page)]) {
      const { background, filter } = await paint(frame);
      expect(filter).toBe("none");
      expect(background, "an opaque system colour").toMatch(/^rgb\(\d+, \d+, \d+\)$/);
    }
  });
});
