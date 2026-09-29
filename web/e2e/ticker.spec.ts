import { type Page, expect, test } from "@playwright/test";
import { recordHref } from "../src/components/stop/commands";
import { AGENT_IDS, APPROVAL_IDS } from "../src/fixtures/workspace";

/**
 * Under the header, while every feed answers, a glass ticker tape shows each held instrument's last
 * price and change on the day. It drifts left, holds still under the pointer or focus, and stands
 * still for reduced motion, scrolling by hand. On its left, the age of the oldest feed opens the four
 * feeds on hover or keyboard focus. A stale or failing feed, or a frozen record screen, shows the full
 * status strip instead, at the same height, so nothing moves.
 */

const START = new Date("2026-09-28T18:05:20Z");
const ticker = (page: Page) => page.locator("[data-slot=ticker]");
const tape = (page: Page) => page.getByRole("region", { name: /^Held instruments/ });
const track = (page: Page) => page.locator("[data-slot=ticker-track]");

async function open(page: Page, path: string, width = 1440, height = 900) {
  await page.setViewportSize({ width, height });
  await page.goto(path);
  await page.waitForLoadState("networkidle");
}

async function offset(page: Page): Promise<number> {
  return track(page).evaluate((el) => new DOMMatrix(getComputedStyle(el).transform === "none" ? undefined : getComputedStyle(el).transform).m41 + (parseFloat(getComputedStyle(el).translate) || 0));
}

async function mainTop(page: Page): Promise<number> {
  return page.evaluate(() => document.querySelector("main")!.getBoundingClientRect().top + window.scrollY);
}

test("sits under the header, 34 px of glass, and stays there as the page scrolls", async ({ page }) => {
  await open(page, "/");
  const header = (await page.getByRole("banner").boundingBox())!;
  let box = (await ticker(page).boundingBox())!;
  expect(box.y).toBe(header.y + header.height);
  expect(box.height).toBe(34);
  expect(box.width).toBe(1440);
  await page.evaluate(() => window.scrollTo({ top: 400, behavior: "instant" }));
  await page.waitForFunction(() => window.scrollY === 400);
  box = (await ticker(page).boundingBox())!;
  expect(box.y, "pinned under the header").toBe(header.height);
  const paint = await ticker(page).evaluate((el) => ({ filter: getComputedStyle(el).backdropFilter, image: getComputedStyle(el).backgroundImage }));
  expect(paint).toEqual({ filter: "blur(22px) saturate(1.8)", image: "none" });
  await expect(page.getByRole("button", { name: "Stop", exact: true })).toBeInViewport({ ratio: 1 });
  expect(await page.evaluate(() => getComputedStyle(document.documentElement).scrollPaddingTop), "focus scrolls clear of the header and the tape").toBe("114px");
});

test("shows symbol, last price and signed change, in tabular figures", async ({ page }) => {
  await open(page, "/");
  const first = tape(page).locator("ul:not([data-copy]) > li").first();
  await expect(first).toHaveText("BTC/USD$56,789.01+1.07% up today");
  const style = await first.evaluate((li) => ({
    symbol: getComputedStyle(li.children[0]).fontWeight,
    figures: getComputedStyle(li.children[1]).fontVariantNumeric,
    change: getComputedStyle(li.children[2]).fontVariantNumeric,
  }));
  expect(style).toEqual({ symbol: "600", figures: "tabular-nums", change: "tabular-nums" });
  await expect(tape(page)).not.toContainText(/[+−]\$/);
});

test("drifts left, and holds still under the pointer and under focus", async ({ page }) => {
  await open(page, "/");
  const a = await offset(page);
  await page.waitForTimeout(600);
  const b = await offset(page);
  expect(b, "the tape moved left").toBeLessThan(a);

  await tape(page).hover();
  await expect.poll(() => track(page).evaluate((el) => el.getAnimations()[0]?.playState)).toBe("paused");
  const held = await offset(page);
  await page.waitForTimeout(400);
  expect(await offset(page)).toBe(held);

  await page.mouse.move(700, 600);
  await expect.poll(() => track(page).evaluate((el) => el.getAnimations()[0]?.playState)).toBe("running");
  await tape(page).focus();
  await expect.poll(() => track(page).evaluate((el) => el.getAnimations()[0]?.playState)).toBe("paused");
});

test("stands still for reduced motion, and scrolls by hand instead", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await open(page, "/", 720, 800);
  expect(await track(page).evaluate((el) => el.getAnimations().length)).toBe(0);
  await expect(tape(page).locator("[data-copy]")).toBeHidden();
  const scroll = await tape(page).evaluate((el) => {
    el.scrollLeft = 80;
    return { overflow: getComputedStyle(el).overflowX, left: el.scrollLeft };
  });
  expect(scroll.overflow).toBe("auto");
  expect(scroll.left).toBeGreaterThan(0);
});

test("the age of the oldest feed ticks in a segment that keeps its width", async ({ page }) => {
  await page.clock.install({ time: START });
  await page.clock.pauseAt(START);
  await open(page, "/");
  const live = ticker(page).getByRole("button", { name: /^Live/ });
  const liveAge = live.locator("[data-slot=freshness-age]");
  await expect(live).toHaveAccessibleName("Live: 10 s since the oldest feed answered");
  await expect(live).toHaveText("Live·10 s");
  const width = (await live.boundingBox())!.width;
  for (const [ms, text] of [
    [30_000, "40 s"],
    [60_000, "1 min"],
  ] as const) {
    await page.clock.runFor(ms);
    await expect(liveAge).toHaveText(text);
    expect((await live.boundingBox())!.width, text).toBe(width);
  }
});

test("the age opens the four feeds on hover, and on keyboard focus without reopening as focus comes back", async ({ page }) => {
  await open(page, "/");
  const live = ticker(page).getByRole("button", { name: /^Live/ });
  const feeds = page.locator("[data-slot=feeds]");
  await live.hover();
  await expect(feeds).toBeVisible();
  await expect(feeds.locator("li")).toHaveText([/^Market data as of \d\d:\d\d:\d\d$/, /^Deployment answered at /, /^Broker as of /, /^Push relay as of /]);
  await page.mouse.move(700, 600);
  await expect(feeds).toBeHidden();

  await page.locator("#main").focus();
  for (let i = 0; i < 40 && !(await live.evaluate((el) => el === document.activeElement)); i++) await page.keyboard.press("Shift+Tab");
  await expect(live).toBeFocused();
  await expect(feeds).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(feeds).toBeHidden();
  await expect(live).toBeFocused();
  await page.waitForTimeout(500);
  await expect(feeds, "focus coming back does not reopen it").toBeHidden();
  await page.keyboard.press("Tab");
  await expect(tape(page)).toBeFocused();
});

test("a stale feed shows the full status strip in its place, and nothing moves", async ({ page }) => {
  await open(page, "/?scenario=normal", 1280);
  const withTape = await mainTop(page);
  await expect(ticker(page)).toBeVisible();
  await open(page, "/?scenario=stale", 1280);
  await expect(ticker(page)).toHaveCount(0);
  const strip = page.locator("[data-slot=status-strip][data-degraded]");
  await expect(strip).toBeVisible();
  expect((await strip.boundingBox())!.height).toBe(34);
  expect(await mainTop(page)).toBe(withTape);
});

for (const [name, path] of [
  ["an approval request", `/approvals/${APPROVAL_IDS.swingXyz}`],
  ["the kill-switch record", recordHref("kill", AGENT_IDS.btc)],
] as const) {
  test(`${name} is frozen, so it shows the status strip`, async ({ page }) => {
    await open(page, path);
    await expect(ticker(page)).toHaveCount(0);
    await expect(page.locator("[data-slot=status-strip]")).toBeVisible();
  });
}

test("tabbing through a long page, no focused control sits under the header or the tape", async ({ page }) => {
  await open(page, "/agents", 1440, 700);
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  const bottom = await ticker(page).evaluate((el) => el.getBoundingClientRect().bottom);
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

test("phones keep the status strip", async ({ page }) => {
  await open(page, "/", 390, 844);
  await expect(ticker(page)).toBeHidden();
  await expect(page.locator("[data-slot=status-strip]")).toBeVisible();
});
