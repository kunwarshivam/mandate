import { existsSync } from "node:fs";
import { join } from "node:path";
import { type Page, expect, test } from "@playwright/test";

/**
 * The landing page (DEC-212) in a real browser, in light and dark (the two projects): no sideways
 * scroll from 320 px up, no gradients in any computed style, the dark screenshots in dark, nothing
 * moving with motion reduced, nothing shifting as the images load, and a keyboard order that follows
 * the page.
 *
 * The page renders at /welcome once the site route group exists (`src/app/(site)/welcome/page.tsx`,
 * owned by the sign-in work). Until then the spec skips. To run it against another route that renders
 * `<Landing />`, set `LANDING_E2E_PATH`.
 */

const ROUTE = join(__dirname, "../src/app/(site)/welcome/page.tsx");
const PATH = process.env.LANDING_E2E_PATH ?? "/welcome";
const PREVIEW = Boolean(process.env.LANDING_E2E_PATH);

test.skip(!PREVIEW && !existsSync(ROUTE), "the /welcome route does not exist yet; set LANDING_E2E_PATH to run against another route");

const WIDTHS = [320, 390, 1024, 1440];

async function open(page: Page, width: number, height = 900) {
  await page.setViewportSize({ width, height });
  await page.goto(PATH, { waitUntil: "load" });
  await page.locator("[data-slot=landing]").waitFor();
}

for (const width of WIDTHS) {
  test(`${width} px: nothing scrolls sideways`, async ({ page }) => {
    await open(page, width);
    const { scroll, inner } = await page.evaluate(() => ({ scroll: document.documentElement.scrollWidth, inner: window.innerWidth }));
    expect(scroll).toBeLessThanOrEqual(inner);
    for (const box of await page.locator("[data-slot=landing] img:visible").evaluateAll((imgs) => imgs.map((i) => i.getBoundingClientRect().right))) {
      expect(box).toBeLessThanOrEqual(width);
    }
  });
}

test("no element draws a gradient", async ({ page }) => {
  await open(page, 1440);
  const offenders = await page.evaluate(() =>
    [...document.querySelectorAll("[data-slot=landing] *, [data-slot=landing] ~ footer *")]
      .filter((el) => {
        const s = getComputedStyle(el);
        return [s.backgroundImage, getComputedStyle(el, "::before").backgroundImage, getComputedStyle(el, "::after").backgroundImage].some((v) => /gradient/.test(v));
      })
      .map((el) => el.outerHTML.slice(0, 120)),
  );
  expect(offenders).toEqual([]);
});

test("the screenshots follow the system theme", async ({ page }, info) => {
  await open(page, 1440);
  const dark = info.project.use.colorScheme === "dark";
  const hero = page.locator('[data-shot="hero-desktop"] img');
  await expect(hero).toHaveJSProperty("complete", true);
  const src = await hero.evaluate((img: HTMLImageElement) => img.currentSrc);
  expect(src).toContain(dark ? "hero-desktop-dark.png" : "hero-desktop-light.png");
});

test("with motion reduced, nothing on the hero moves", async ({ browser }, info) => {
  const context = await browser.newContext({ reducedMotion: "reduce", colorScheme: info.project.use.colorScheme });
  const page = await context.newPage();
  await open(page, 1440);
  const phone = page.locator('[data-shot="hero-phone"]').locator("xpath=ancestor::*[@data-slot='phone-frame']");
  const style = await phone.evaluate((el) => ({ duration: getComputedStyle(el).transitionDuration, opacity: getComputedStyle(el).opacity }));
  expect(style.duration.split(",").every((d) => parseFloat(d) === 0)).toBe(true);
  expect(style.opacity).toBe("1");
  await context.close();
});

/** Shifts of the page's own content; the layout around it answers for its own. */
test("nothing on the page shifts while it and its images load", async ({ page }) => {
  await page.addInitScript(() => {
    type Shift = { value: number; hadRecentInput: boolean; sources: { node?: Node }[] };
    (window as unknown as { __cls: number }).__cls = 0;
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries() as unknown as Shift[]) {
        const ours = entry.sources.some((s) => s.node instanceof Element && s.node.closest("[data-slot=landing], [data-slot=landing] ~ footer"));
        if (!entry.hadRecentInput && ours) (window as unknown as { __cls: number }).__cls += entry.value;
      }
    }).observe({ type: "layout-shift", buffered: true });
  });
  await open(page, 1440);
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await page.waitForTimeout(800);
  expect(await page.evaluate(() => (window as unknown as { __cls: number }).__cls)).toBeLessThan(0.02);
});

test("the keyboard reaches the calls to action first, then the page in order, with a visible ring", async ({ page }) => {
  await open(page, 1440);
  await page.locator("[data-slot=landing]").focus();
  const names: string[] = [];
  for (let i = 0; i < 6; i++) {
    await page.keyboard.press("Tab");
    names.push(await page.evaluate(() => (document.activeElement as HTMLElement).innerText.trim()));
    if (i === 0) {
      const ring = await page.evaluate(() => getComputedStyle(document.activeElement as HTMLElement).boxShadow);
      expect(ring).not.toBe("none");
    }
  }
  expect(names.slice(0, 2)).toEqual(["Get started", "How it works"]);
  const footer = await page.locator("footer nav a").allInnerTexts();
  expect(footer).toEqual(["How it works", "Questions", "Sign in"]);
});

test("How it works scrolls to its section, below the header", async ({ page }) => {
  await open(page, 1440);
  await page.getByRole("link", { name: "How it works" }).first().click();
  await expect(page).toHaveURL(/#how-it-works$/);
  const top = await page.locator("#how-it-works-title").evaluate((el) => el.getBoundingClientRect().top);
  expect(top).toBeGreaterThan(64);
});

test("one main landmark: the layout around the page adds none", async ({ page }) => {
  test.skip(PREVIEW, "a preview route renders inside the app shell's main");
  await open(page, 1440);
  await expect(page.getByRole("main")).toHaveCount(1);
  await expect(page.locator("[data-slot=landing]")).toHaveAttribute("id", "main");
});
