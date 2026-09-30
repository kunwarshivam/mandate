import { type Page, expect, test } from "@playwright/test";

/**
 * The landing page (DEC-213) in a real browser, in light and dark (the two projects): no sideways
 * scroll from 320 px up, no gradients in any computed style, nothing blinking with motion reduced,
 * nothing shifting as the fonts load, a visible keyboard outline, contents links that land on their
 * section, the tamper demo, and the guestbook against a stubbed /api/beta.
 */

const PATH = "/welcome";
const WIDTHS = [320, 390, 1024, 1440];

async function open(page: Page, width: number, height = 900) {
  await page.setViewportSize({ width, height });
  await page.goto(PATH, { waitUntil: "load" });
  await page.locator("[data-slot=landing]").waitFor();
}

for (const width of WIDTHS) {
  test(`${width} px: nothing scrolls sideways`, async ({ page }) => {
    await open(page, width);
    await page.evaluate(() => document.fonts.ready);
    const { scroll, inner } = await page.evaluate(() => ({ scroll: document.documentElement.scrollWidth, inner: window.innerWidth }));
    expect(scroll).toBeLessThanOrEqual(inner);
  });
}

test("no element draws a gradient", async ({ page }) => {
  await open(page, 1440);
  const offenders = await page.evaluate(() =>
    [...document.querySelectorAll("body *")]
      .filter((el) => {
        const s = getComputedStyle(el);
        return [s.backgroundImage, getComputedStyle(el, "::before").backgroundImage, getComputedStyle(el, "::after").backgroundImage].some((v) => /gradient/.test(v));
      })
      .map((el) => el.outerHTML.slice(0, 120)),
  );
  expect(offenders).toEqual([]);
});

test("with motion reduced, the New tag holds still and stays visible", async ({ browser }, info) => {
  const context = await browser.newContext({ reducedMotion: "reduce", colorScheme: info.project.use.colorScheme });
  const page = await context.newPage();
  await open(page, 1440);
  const tag = page.getByText("New", { exact: true });
  const style = await tag.evaluate((el) => ({ animation: getComputedStyle(el).animationName, opacity: getComputedStyle(el).opacity }));
  expect(style.animation).toBe("none");
  expect(style.opacity).toBe("1");
  await context.close();
});

/** Fonts swap in once they load; the text they reflow must stay under the good-CLS line. */
test("the page barely shifts while it and its fonts load", async ({ page }) => {
  await page.addInitScript(() => {
    type Shift = { value: number; hadRecentInput: boolean };
    (window as unknown as { __cls: number }).__cls = 0;
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries() as unknown as Shift[]) if (!entry.hadRecentInput) (window as unknown as { __cls: number }).__cls += entry.value;
    }).observe({ type: "layout-shift", buffered: true });
  });
  await open(page, 1440);
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(500);
  expect(await page.evaluate(() => (window as unknown as { __cls: number }).__cls)).toBeLessThan(0.1);
});

test("the keyboard goes skip link, then the guide links, with a visible outline", async ({ page }) => {
  await open(page, 1440);
  await page.keyboard.press("Tab");
  expect(await page.evaluate(() => (document.activeElement as HTMLElement).innerText.trim())).toBe("Skip to content");
  await page.keyboard.press("Tab");
  const first = await page.evaluate(() => {
    const el = document.activeElement as HTMLElement;
    return { text: el.innerText.trim(), outline: getComputedStyle(el).outlineStyle, width: parseFloat(getComputedStyle(el).outlineWidth) };
  });
  expect(first.text).toBe("What's New?");
  expect(first.outline).not.toBe("none");
  expect(first.width).toBeGreaterThan(0);
});

test("a contents link scrolls its section to the top of the window", async ({ page }) => {
  await open(page, 1440);
  await page.getByRole("navigation", { name: "Contents" }).getByRole("link", { name: "How it works" }).click();
  await expect(page).toHaveURL(/#how$/);
  // The page scrolls inside the retro browser's window, not the viewport: the section's top is
  // measured against that scroll root, where the page's own 24 px offset and the sunken 2 px
  // border land it at 26.
  await expect
    .poll(() =>
      page.locator("#how").evaluate((el) => {
        const root = el.closest("[data-scroll-root]")!;
        return Math.round(el.getBoundingClientRect().top - root.getBoundingClientRect().top);
      }),
    )
    .toBe(26);
});

test("the contents selects the section being read, and the status bar shows where a hovered link goes", async ({ page }) => {
  await open(page, 1440);
  const contents = page.getByRole("navigation", { name: "Contents" });
  await page.locator("#safety").evaluate((el) => el.scrollIntoView());
  await expect(contents.locator("[aria-current=location]")).toHaveText("How it stays in check");
  await contents.getByRole("link", { name: "The record" }).hover();
  await expect(page.locator("[data-slot=status-text]")).toHaveText("http://www.owlhead.ai/#record");
  await page.mouse.move(0, 0);
  await expect(page.locator("[data-slot=status-text]")).toHaveText("Document: Done");
});

test("390 px: the record fits the phone without scrolling sideways", async ({ page }) => {
  await open(page, 390);
  await page.evaluate(() => document.fonts.ready);
  const fit = await page.locator("[data-slot=record-trace] table").evaluate((t) => t.scrollWidth <= t.parentElement!.clientWidth);
  expect(fit).toBe(true);
});

test("one main landmark, and no site header over the page", async ({ page }) => {
  await open(page, 1440);
  await expect(page.getByRole("main")).toHaveCount(1);
  await expect(page.locator("[data-slot=landing]")).toHaveAttribute("id", "main");
  await expect(page.locator("header.glass")).toHaveCount(0);
});

test("editing a line of the record breaks the chain from there, and undoing it mends it", async ({ page }) => {
  await open(page, 1440);
  await expect(page.getByText("Chain check: all 8 lines match.")).toBeVisible();
  await page.getByRole("button", { name: "Edit line 3" }).click();
  await expect(page.getByText(/Chain check: fails at line 3/)).toBeVisible();
  await page.getByRole("button", { name: "Undo the edit" }).click();
  await expect(page.getByText("Chain check: all 8 lines match.")).toBeVisible();
});

test("the guestbook sends the email and use, and says you're on the list", async ({ page }) => {
  let sent: unknown = null;
  await page.route("**/api/beta", async (route) => {
    sent = route.request().postDataJSON();
    await route.fulfill({ status: 201, contentType: "application/json", body: '{"ok":true}' });
  });
  await open(page, 390);
  await page.getByLabel("Email address:").fill("ada@example.com");
  await page.getByRole("radio", { name: "Running a trading desk" }).check();
  await page.getByRole("button", { name: "Sign the guestbook" }).click();
  await expect(page.locator("[data-slot=beta-done]")).toContainText("We'll write to ada@example.com");
  expect(sent).toEqual({ email: "ada@example.com", role: "desk", website: "" });
});
