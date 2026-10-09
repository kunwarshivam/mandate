import { type Page, expect, test } from "@playwright/test";
import { AGENT_IDS } from "../src/fixtures/workspace";

/**
 * Messages fills the window edge to edge (DEC-481): no gutter and no content width, the panes each
 * scrolling on their own under the header, and the page itself never scrolling. The thread follows
 * the latest entry while the owner is at it, leaves them be once they scroll up, offers a way back,
 * and comes back down when they send. A message's Markdown scrolls inside itself, never the page.
 */

const THREAD = `/messages/${AGENT_IDS.swing}?scenario=normal`;

async function open(page: Page, path: string, width: number, height: number) {
  await page.setViewportSize({ width, height });
  await page.goto(path);
  await page.waitForLoadState("networkidle");
}

const field = (page: Page) => page.getByRole("textbox", { name: /^Ask about Agent 2/ });
const log = (page: Page) => page.locator("[data-slot=thread-scroll]");

async function say(page: Page, text: string) {
  await field(page).fill(text);
  await field(page).press("Enter");
  await expect(field(page)).toHaveValue("");
}

async function gapToEnd(page: Page) {
  return log(page).evaluate((el) => el.scrollHeight - el.scrollTop - el.clientHeight);
}

async function pageScrolls(page: Page) {
  return page.evaluate(() => ({ down: document.documentElement.scrollHeight - innerHeight, across: document.documentElement.scrollWidth - innerWidth }));
}

/** The rail shows from 80rem; from 64 to 80rem the thread takes its room, so it is never the narrowest column (DEC-513 item 8). */
for (const [width, height, withRail] of [
  [1024, 768, false],
  [1440, 900, true],
  [1920, 1080, true],
] as const) {
  test(`${width} px: ${withRail ? "three panes" : "two panes, the rail waiting for 80rem,"} from edge to edge, each to the foot of the window, and the composer above the dock`, async ({ page }) => {
    await open(page, THREAD, width, height);
    const threads = (await page.locator("[data-slot=threads-pane]").boundingBox())!;
    const pane = (await page.locator("[data-slot=thread-pane]").boundingBox())!;
    const header = (await page.getByRole("banner").first().boundingBox())!;
    expect(threads.x, "the threads start at the window's left edge").toBe(0);
    expect(pane.x, "the thread meets the threads").toBeCloseTo(threads.x + threads.width, 0);
    const panes = [threads, pane];
    if (withRail) {
      const rail = (await page.locator("[data-slot=rail-pane]").boundingBox())!;
      expect(rail.x + rail.width, "the rail ends at the window's right edge").toBeCloseTo(width, 0);
      expect(rail.x, "and the rail meets the thread").toBeCloseTo(pane.x + pane.width, 0);
      panes.push(rail);
    } else {
      await expect(page.locator("[data-slot=rail-pane]"), "below 80rem the rail waits for room (DEC-513 item 8)").toBeHidden();
      expect(pane.x + pane.width, "and the thread ends at the window's right edge").toBeCloseTo(width, 0);
      expect(pane.width, "the thread is the widest column").toBeGreaterThan(threads.width);
    }
    for (const box of panes) {
      expect(box.y, "each pane starts under the header").toBeCloseTo(header.y + header.height, 0);
      expect(box.y + box.height, "and runs to the foot of the window").toBeCloseTo(height, 0);
    }
    const composer = (await page.locator("[data-slot=composer]").boundingBox())!;
    const dock = (await page.getByRole("navigation", { name: "Primary" }).boundingBox())!;
    expect(composer.y + composer.height, "the composer ends above the dock").toBeLessThanOrEqual(dock.y);
    expect(composer.x, "the composer spans the thread").toBeLessThan(pane.x + 40);
    await say(page, "How close is it to its limits?");
    await say(page, "What does it hold?");
    expect(await pageScrolls(page), "the page itself never scrolls").toEqual({ down: 0, across: 0 });
  });
}

test("on a phone the thread fills the width between the header and the tab bar", async ({ page }) => {
  await open(page, THREAD, 390, 844);
  const pane = (await page.locator("[data-slot=thread-pane]").boundingBox())!;
  expect(pane.x).toBe(0);
  expect(pane.width).toBe(390);
  const composer = (await page.locator("[data-slot=composer]").boundingBox())!;
  const tabs = (await page.getByRole("navigation", { name: "Main" }).boundingBox())!;
  expect(composer.y + composer.height, "the composer sits above the tab bar").toBeLessThanOrEqual(tabs.y);
  expect(composer.y + composer.height, "and close on it").toBeGreaterThan(tabs.y - 24);
  await say(page, "How close is it to its limits?");
  expect(await pageScrolls(page)).toEqual({ down: 0, across: 0 });
});

/**
 * On a phone the conversation gets the screen (DEC-482): the thread's one-row bar replaces the app
 * header and carries the paper badge, the pinned request is 44px with nothing cut off, and the log
 * is most of what is left, while Stop stays on the tab bar.
 */
for (const [width, height, share] of [
  [390, 844, 0.6],
  [375, 667, 0.5],
  [320, 568, 0.4],
] as const) {
  test(`${width}×${height}: the thread's bar is the only chrome above it, and the log has ${share * 100}% of the screen or more`, async ({ page }) => {
    await open(page, THREAD.replace("normal", "approvals"), width, height);
    await expect(page.locator("[data-slot=app-header]"), "the app header gives way to the thread's bar").toBeHidden();
    const bar = page.locator("[data-slot=thread-bar]");
    expect((await bar.boundingBox())!.y, "the bar is at the top of the window").toBe(0);
    expect((await bar.boundingBox())!.height, "in one row").toBeLessThanOrEqual(60);
    await expect(bar.locator("[data-slot=environment-badge]"), "and it carries the paper badge").toBeInViewport({ ratio: 1 });
    await expect(bar.locator("[data-slot=environment-badge]")).toContainText("PAPER");
    await expect(page.getByRole("navigation", { name: "Main" }).getByRole("button", { name: "Stop", exact: true }), "Stop stays on the tab bar").toBeInViewport({ ratio: 1 });
    for (const target of await bar.locator("a[href]").locator("visible=true").all()) {
      const r = await target.evaluate((el: HTMLElement) => (getComputedStyle(el, "::after").position === "absolute" ? el.offsetParent! : el).getBoundingClientRect());
      expect(Math.min(r.width, r.height), `${await target.textContent()} is a 44px target`).toBeGreaterThanOrEqual(44);
    }

    const pinned = page.locator("[data-slot=pinned-request]");
    expect((await pinned.boundingBox())!.height, "the pinned request is 44px").toBeLessThanOrEqual(45);
    for (const line of await pinned.locator(":scope > span:first-child > span").all()) {
      const { scroll, client } = await line.evaluate((el) => ({ scroll: el.scrollWidth, client: el.clientWidth }));
      expect(scroll, `"${await line.innerText()}" is not cut off`).toBeLessThanOrEqual(client);
    }

    expect((await log(page).boundingBox())!.height, "the log has most of the screen").toBeGreaterThanOrEqual(height * share);
    expect(await pageScrolls(page)).toEqual({ down: 0, across: 0 });
  });
}

test("the thread follows the latest entry, leaves a reader who scrolled up, and comes back when they send", async ({ page }) => {
  await open(page, THREAD, 1440, 900);
  await expect.poll(() => gapToEnd(page), { message: "it opens at the latest entry" }).toBeLessThanOrEqual(1);
  for (const ask of ["How close is it to its limits?", "What does it hold?", "What happened today?"]) await say(page, ask);
  await expect.poll(() => gapToEnd(page), { message: "it follows each answer" }).toBeLessThanOrEqual(1);

  const jump = page.getByRole("button", { name: "Jump to latest" });
  await expect(jump).toHaveCount(0);
  await page.mouse.move(700, 400);
  await page.mouse.wheel(0, -900);
  await expect(jump).toBeVisible();
  const away = await gapToEnd(page);
  expect(away).toBeGreaterThan(300);
  await say(page, "Why did it ask?");
  await expect.poll(() => gapToEnd(page), { message: "sending brings the reader to their message" }).toBeLessThanOrEqual(1);
  await expect(jump).toHaveCount(0);

  await page.mouse.wheel(0, -900);
  await expect(jump).toBeVisible();
  await jump.click();
  await expect.poll(() => gapToEnd(page)).toBeLessThanOrEqual(1);
  await expect(jump).toHaveCount(0);
  await expect(field(page), "the composer has focus again").toBeFocused();
});

test("a message's table and code scroll inside it, and nothing the owner writes is fetched or run", async ({ page }) => {
  const requests: string[] = [];
  page.on("request", (r) => requests.push(r.url()));
  await open(page, THREAD, 1024, 768);
  const wide = Array.from({ length: 12 }, (_, i) => `Column ${i + 1}`);
  await say(
    page,
    [
      `| ${wide.join(" | ")} |`,
      `| ${wide.map(() => "---").join(" | ")} |`,
      `| ${wide.map((_, i) => `$${(i + 1) * 1000}.00`).join(" | ")} |`,
      "",
      "```",
      `const line = "${"x".repeat(240)}";`,
      "```",
      "",
      "![tracker](https://example.com/pixel.png) <img src=https://example.com/raw.png> <script>window.ran = true</script>",
    ].join("\n"),
  );
  const mine = page.locator("[data-slot=owner-message]").last();
  const table = mine.locator("[data-slot=markdown-table]");
  const code = mine.locator("[data-slot=markdown-code] pre");
  for (const box of [table, code]) {
    const { scroll, client } = await box.evaluate((el) => ({ scroll: el.scrollWidth, client: el.clientWidth }));
    expect(scroll, "it is wider than the message").toBeGreaterThan(client);
  }
  expect((await mine.boundingBox())!.x + (await mine.boundingBox())!.width, "the message stays inside the thread").toBeLessThanOrEqual((await log(page).boundingBox())!.x + (await log(page).boundingBox())!.width);
  expect(await pageScrolls(page)).toEqual({ down: 0, across: 0 });
  await expect(mine.locator("img, script")).toHaveCount(0);
  await expect(mine).toContainText("[Image not shown: tracker]");
  expect(await page.evaluate(() => (window as unknown as { ran?: boolean }).ran)).toBeUndefined();
  expect(requests.filter((u) => u.includes("example.com"))).toEqual([]);
});
