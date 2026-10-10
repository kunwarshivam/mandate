import { type Page, expect, test } from "@playwright/test";

/**
 * The long page under the desktop (DEC-907) in a real browser, in light and dark (the two projects):
 * the desktop fills the first screen, the page rises over it with Sign in and Sign up in its bar, no
 * width from 320 px scrolls sideways, each picture of the app is the one for the page's theme, and
 * with motion reduced nothing on the page moves with the scroll.
 */

const PATH = "/welcome";

async function open(page: Page, width: number, height = 900) {
  await page.setViewportSize({ width, height });
  await page.goto(PATH, { waitUntil: "load" });
  await page.evaluate(() => document.fonts.ready);
}

const toBottom = (page: Page) => page.evaluate(() => window.scrollTo({ top: document.documentElement.scrollHeight, behavior: "instant" }));

const scrollDriven = (page: Page) =>
  page.evaluate(() => document.getAnimations().filter((a) => a.timeline && !(a.timeline instanceof DocumentTimeline)).length);

for (const width of [320, 390, 1024, 1440]) {
  test(`${width} px: the desktop fills the first screen, and the page under it keeps Sign in and Sign up in view without scrolling sideways`, async ({ page }) => {
    await open(page, width, 844);
    const desktop = page.locator("[data-slot=desktop-stage]");
    const sheet = page.locator("[data-slot=long-page]");
    const first = await desktop.boundingBox();
    expect(first, "the desktop's stage").not.toBeNull();
    expect(first!.y).toBe(0);
    expect(first!.height).toBeGreaterThanOrEqual(843);
    expect((await sheet.boundingBox())!.y, "the long page starts below the first screen").toBeGreaterThanOrEqual(843);

    await toBottom(page);
    expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBeLessThanOrEqual(0);
    const bar = page.locator("[data-slot=page-bar]");
    expect((await bar.boundingBox())!.y, "the bar sticks to the top edge").toBe(0);
    const account = bar.locator("[data-slot=account-buttons]");
    for (const control of [account.getByRole("link", { name: "Sign in" }), account.getByRole("button", { name: "Sign up" })]) {
      await expect(control).toBeInViewport({ ratio: 1 });
    }
    await expect(account.getByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/login");
  });
}

test("the bar's Sign up brings the request form into view with the cursor in its email field", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await open(page, 1440);
  await page.evaluate(() => window.scrollTo({ top: window.innerHeight * 1.5, behavior: "instant" }));
  await page.locator("[data-slot=page-bar]").getByRole("button", { name: "Sign up" }).click();
  await expect(page.getByLabel("Email address", { exact: true })).toBeFocused();
  await expect(page.getByRole("heading", { level: 2, name: "Ask for a place in the beta." })).toBeInViewport();
});

test("each picture of the app has loaded, and it is the one for the page's theme", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await open(page, 1440);
  const mode = await page.evaluate(() => document.documentElement.dataset.mode);
  expect(mode === "light" || mode === "dark").toBe(true);
  const shots = page.locator("[data-slot=long-page] [data-slot=shot]");
  expect(await shots.count()).toBe(5);
  for (const shot of await shots.all()) {
    await shot.scrollIntoViewIfNeeded();
    const shown = shot.locator("img:visible");
    await expect(shown).toHaveCount(1);
    await expect.poll(() => shown.evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0)).toBe(true);
    expect(decodeURIComponent((await shown.getAttribute("src")) ?? "")).toContain(`-${mode}.png`);
  }
});

test("with motion allowed, the desktop recedes and the pieces rise with the scroll", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await open(page, 1440);
  expect(await scrollDriven(page)).toBeGreaterThan(0);
  await page.evaluate(() => window.scrollTo({ top: window.innerHeight, behavior: "instant" }));
  await expect
    .poll(() => page.locator("[data-slot=desktop-stage] > *").first().evaluate((el) => getComputedStyle(el).transform))
    .not.toBe("none");
});

test("with motion reduced, nothing on the page moves with the scroll and every piece rests in place", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await open(page, 1440);
  await page.evaluate(() => window.scrollTo({ top: window.innerHeight, behavior: "instant" }));
  expect(await scrollDriven(page)).toBe(0);
  expect(await page.locator("[data-slot=desktop-stage] > *").first().evaluate((el) => getComputedStyle(el).transform)).toBe("none");
  const moved = await page.locator("[data-slot=long-page] [data-slot=shot]").evaluateAll((shots) =>
    shots.flatMap((shot) => {
      const off: string[] = [];
      for (let el: Element | null = shot; el && el.getAttribute("data-slot") !== "long-page"; el = el.parentElement) {
        const { opacity, translate } = getComputedStyle(el);
        if (opacity !== "1" || translate !== "none") off.push(`${shot.getAttribute("data-shot")}: opacity ${opacity}, translate ${translate}`);
      }
      return off;
    }),
  );
  expect(moved).toEqual([]);
});

test("the Scroll cue shows on the first screen, takes the visitor to the opening, and is covered once the page has risen", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await open(page, 1440);
  const cue = page.locator("[data-slot=scroll-cue]");
  await expect(cue).toBeInViewport({ ratio: 1 });
  await expect(cue).toHaveAttribute("data-at", /status|edge/);
  await cue.click();
  await expect(page.locator("#intro-title")).toBeInViewport();
  const box = (await cue.boundingBox())!;
  const top = await page.evaluate(([x, y]) => document.elementFromPoint(x!, y!)?.closest("[data-slot=scroll-cue]") !== null, [box.x + box.width / 2, box.y + box.height / 2]);
  expect(top, "the risen page covers the cue").toBe(false);
});

test("with motion allowed, the owl flies down the page and the pieces settle as they come into view", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await open(page, 1440);
  const owl = page.locator("[data-slot=flying-owl]");
  await expect(owl).toHaveAttribute("data-owl", "flying");
  await expect(owl).toHaveAttribute("aria-hidden", "true");
  const where = () => owl.evaluate((el) => getComputedStyle(el).transform);
  await page.locator("#threads").scrollIntoViewIfNeeded();
  const at = await where();
  await page.locator("#limits").scrollIntoViewIfNeeded();
  await expect.poll(where).not.toBe(at);
  await expect(page.locator("#limits-title")).toHaveAttribute("data-shown", "");
  expect(await page.locator("[data-slot=long-page]").getAttribute("data-motion")).toBe("on");
});

test("with motion reduced, the owl stands still on the first perch and asks for no frame", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await open(page, 1440);
  const owl = page.locator("[data-slot=flying-owl]");
  await expect(owl).toHaveAttribute("data-owl", "resting");
  const at = await owl.boundingBox();
  await page.evaluate(() => window.scrollTo({ top: window.innerHeight * 1.2, behavior: "instant" }));
  const perch = await page.locator("[data-perch]").first().boundingBox();
  const now = (await owl.boundingBox())!;
  expect(now.y, "it scrolls with the page, not on its own").toBeCloseTo(at!.y - (await page.evaluate(() => window.scrollY)), 0);
  expect(Math.abs(now.y + now.height / 2 - perch!.y), "it stands on the first perch's top edge").toBeLessThan(now.height);
  expect(await page.locator("[data-slot=long-page]").getAttribute("data-motion")).toBeNull();
});
