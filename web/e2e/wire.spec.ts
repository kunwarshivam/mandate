import { type Page, expect, test } from "@playwright/test";
import { recordHref } from "../src/components/stop/commands";
import { AGENT_IDS, APPROVAL_IDS } from "../src/fixtures/workspace";

/**
 * Under the header, while every feed answers, the agent wire lists what the agents are doing, the
 * request waiting for you first, each item a link, in ink and muted text only. It drifts left, holds
 * still under the pointer or focus, and stands still for reduced motion, scrolling by hand. On its
 * left, the age of the oldest feed opens the four feeds on hover or keyboard focus. A stale or failing
 * feed, or a frozen record screen, shows the full status strip instead, at the same height, so
 * nothing moves.
 */

const START = new Date("2026-09-28T18:05:20Z");
const wire = (page: Page) => page.locator("[data-slot=wire]");
const tape = (page: Page) => page.getByRole("region", { name: "Agent activity" });
const track = (page: Page) => page.locator("[data-slot=wire-track]");
const items = (page: Page) => tape(page).locator("ul:not([data-copy]) > li");

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
  let box = (await wire(page).boundingBox())!;
  expect(box.y).toBe(header.y + header.height);
  expect(box.height).toBe(34);
  expect(box.width).toBe(1440);
  await page.evaluate(() => window.scrollTo({ top: 400, behavior: "instant" }));
  await page.waitForFunction(() => window.scrollY === 400);
  box = (await wire(page).boundingBox())!;
  expect(box.y, "pinned under the header").toBe(header.height);
  const paint = await wire(page).evaluate((el) => ({ filter: getComputedStyle(el).backdropFilter, image: getComputedStyle(el).backgroundImage }));
  expect(paint).toEqual({ filter: "blur(22px) saturate(1.8)", image: "none" });
  await expect(page.getByRole("button", { name: "Stop", exact: true })).toBeInViewport({ ratio: 1 });
  expect(await page.evaluate(() => getComputedStyle(document.documentElement).scrollPaddingTop), "focus scrolls clear of the header and the wire").toBe("114px");
});

test("the request waiting for you comes first: its time in tabular figures, the agent in semibold, the state in words", async ({ page }) => {
  await open(page, "/");
  const first = items(page).first();
  await expect(first).toHaveText("14:04Agent 2 asked you to buy 2 XYZ at $141.30, Waiting for you");
  const style = await first.evaluate((li) => ({
    time: getComputedStyle(li.querySelector("time")!).fontVariantNumeric,
    agent: getComputedStyle(li.querySelector(".font-semibold")!).fontWeight,
  }));
  expect(style).toEqual({ time: "tabular-nums", agent: "600" });
  await expect(items(page).nth(1)).toContainText("Agent 1 blocked: orders are at most $1,000.00");
  await expect(tape(page).getByRole("list")).toHaveCount(1);
});

test("every item is a link: a request to its page, a decision to the page recent activity opens", async ({ page }) => {
  await open(page, "/");
  await expect(items(page).first().getByRole("link")).toHaveAttribute("href", `/approvals/${APPROVAL_IDS.swingXyz}`);
  await expect(items(page).nth(1).getByRole("link")).toHaveAttribute("href", `/agents/${AGENT_IDS.btc}/decisions/01JBWPQ5E6EYCNDY0YP57RCYBV`);
  expect(await items(page).evaluateAll((lis) => lis.every((li) => li.querySelector("a[href]")))).toBe(true);
  await tape(page).hover();
  await items(page).first().getByRole("link").click();
  await expect(page).toHaveURL(new RegExp(`/approvals/${APPROVAL_IDS.swingXyz}$`));
});

test("ink and muted text only: no gain or loss colour, no crimson, and no amount won or lost", async ({ page }) => {
  await open(page, "/");
  const found = await wire(page).evaluate((root) => {
    const probe = (v: string) => {
      const el = document.createElement("span");
      el.style.color = `var(${v})`;
      document.body.append(el);
      const c = getComputedStyle(el).color;
      el.remove();
      return c;
    };
    const allowed = new Set([probe("--foreground"), probe("--muted-foreground")]);
    const banned = new Set(["--gain", "--loss", "--gain-cvd", "--loss-cvd", "--crimson", "--crimson-edge"].map(probe));
    const colours = [...root.querySelectorAll("[data-slot=wire-tape] *")].filter((el) => el.childNodes.length && [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent!.trim())).map((el) => getComputedStyle(el).color);
    return { off: colours.filter((c) => !allowed.has(c)), banned: colours.filter((c) => banned.has(c)), count: colours.length };
  });
  expect(found.count).toBeGreaterThan(10);
  expect(found.off).toEqual([]);
  expect(found.banned).toEqual([]);
  await expect(tape(page)).not.toContainText(/[+−]\$|P&L|profit/i);
});

test("the loop's copy is hidden from assistive technology and never takes focus", async ({ page }) => {
  await open(page, "/");
  const copy = tape(page).locator("[data-copy]");
  await expect(copy).toHaveAttribute("aria-hidden", "true");
  await expect(copy).toHaveAttribute("inert", "");
  const live = wire(page).getByRole("button", { name: /^Live/ });
  await live.focus();
  await page.keyboard.press("Escape");
  const n = await items(page).count();
  for (let i = 0; i < n + 2; i++) {
    await page.keyboard.press("Tab");
    expect(await page.evaluate(() => Boolean(document.activeElement?.closest("[data-copy]")))).toBe(false);
  }
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
  await items(page).first().getByRole("link").focus();
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
  const live = wire(page).getByRole("button", { name: /^Live/ });
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
  const live = wire(page).getByRole("button", { name: /^Live/ });
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
  await expect(items(page).first().getByRole("link")).toBeFocused();
});

test("a stale feed shows the full status strip in its place, and nothing moves", async ({ page }) => {
  await open(page, "/?scenario=normal", 1280);
  const withTape = await mainTop(page);
  await expect(wire(page)).toBeVisible();
  await open(page, "/?scenario=stale", 1280);
  await expect(wire(page)).toHaveCount(0);
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
    await expect(wire(page)).toHaveCount(0);
    await expect(page.locator("[data-slot=status-strip]")).toBeVisible();
  });
}

test("tabbing through a long page, no focused control sits under the header or the wire", async ({ page }) => {
  await open(page, "/agents", 1440, 700);
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  const bottom = await wire(page).evaluate((el) => el.getBoundingClientRect().bottom);
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
  await expect(wire(page)).toBeHidden();
  await expect(page.locator("[data-slot=status-strip]")).toBeVisible();
});
