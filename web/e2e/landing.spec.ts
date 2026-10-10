import { type Page, expect, test } from "@playwright/test";

/**
 * The landing page (DEC-213) in a real browser, in light and dark (the two projects): no sideways
 * scroll from 320 px up, no gradients in any computed style, nothing blinking with motion reduced,
 * nothing shifting as the fonts load, a visible keyboard outline, contents links that land on their
 * section, and the record's tamper demo and the guestbook, each in its own desktop window, the
 * guestbook against a stubbed /api/beta.
 */

const PATH = "/welcome";
const WIDTHS = [320, 390, 1024, 1440];

async function open(page: Page, width: number, height = 900) {
  await page.setViewportSize({ width, height });
  await page.goto(PATH, { waitUntil: "load" });
  await page.locator("[data-slot=landing]").waitFor();
}

/** Opens one of the desktop's windows from the hero's buttons, as a visitor would, and returns it. */
async function openWindow(page: Page, button: "See why it traded" | "Sign the guestbook", title: string) {
  await page.locator("[data-slot=hero-actions]").getByRole("button", { name: button }).click();
  const win = page.getByRole("region", { name: title });
  await expect(win).toBeVisible();
  return win;
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

type Frame = { x: number; y: number; w: number; h: number; opacity: number } | null;

/** Records the guestbook window's box on every frame from now on; `null` while it is hidden. */
async function track(page: Page) {
  await page.evaluate(() => {
    const log: Frame[] = [];
    Object.assign(window, { __frames: log });
    const tick = () => {
      const el = document.getElementById("win-guestbook");
      if (!el || el.hidden) log.push(null);
      else {
        const r = el.getBoundingClientRect();
        log.push({ x: r.left + r.width / 2, y: r.top + r.height / 2, w: r.width, h: r.height, opacity: Number(getComputedStyle(el).opacity) });
      }
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  return {
    reset: () => page.evaluate(() => (window as unknown as { __frames: Frame[] }).__frames.splice(0)),
    frames: () => page.evaluate(() => [...(window as unknown as { __frames: Frame[] }).__frames]),
  };
}

const centre = async (page: Page, selector: string) => {
  const b = (await page.locator(selector).boundingBox())!;
  return { x: b.x + b.width / 2, y: b.y + b.height / 2, w: b.width, h: b.height };
};

test("a window zooms out of the icon that opened it and into its taskbar button, as a Mac's do", async ({ page }) => {
  await open(page, 1440);
  const frames = await track(page);
  const icon = await centre(page, "[data-slot=desktop-icons] li[data-app=guestbook]");
  await frames.reset();
  await page.locator("[data-slot=desktop-icons] li[data-app=guestbook] button").click();
  await expect(page.locator("#win-guestbook")).toBeVisible();
  await page.waitForTimeout(500);
  const opening = (await frames.frames()).filter((f) => f !== null);
  const first = opening[0]!;
  const last = opening.at(-1)!;
  expect(Math.abs(first.x - icon.x), "the first frame is centred on the icon").toBeLessThan(4);
  expect(Math.abs(first.y - icon.y)).toBeLessThan(4);
  expect(first.w, "and no wider than it").toBeLessThanOrEqual(icon.w + 1);
  expect(first.opacity).toBeLessThan(0.2);
  expect(opening.length, "over several frames, not a jump").toBeGreaterThan(5);
  expect(opening.every((f, i) => i === 0 || f!.w >= opening[i - 1]!.w), "growing all the way").toBe(true);
  expect(last.opacity).toBe(1);

  const task = await centre(page, "[data-task=guestbook]");
  await frames.reset();
  await page.getByRole("button", { name: "Minimize guestbook.cgi" }).click();
  await expect(page.locator("#win-guestbook")).toBeHidden();
  const going = (await frames.frames()).filter((f) => f !== null);
  const end = going.at(-1)!;
  expect(going.length, "minimizing moves too").toBeGreaterThan(5);
  expect(Math.abs(end.x - task.x), "and ends on the taskbar button").toBeLessThan(12);
  expect(Math.abs(end.y - task.y)).toBeLessThan(12);
  expect(end.h).toBeLessThanOrEqual(task.h + 1);
});

test("the Mac on the desktop redraws it as System 7, which the server keeps drawing, and a window collapses into the windows menu", async ({ page }) => {
  await open(page, 1440);
  await page.getByRole("list", { name: "Desktop, right" }).getByRole("button", { name: "Mac" }).click();
  await expect(page.locator("[data-slot=menu-bar]")).toBeVisible();
  await expect(page.locator("[data-slot=taskbar]")).toHaveCount(0);

  const html = await (await page.request.get(PATH)).text();
  expect(html, "the server draws the chosen desktop, so nothing swaps after the page paints").toContain('data-os="mac"');
  await page.reload({ waitUntil: "load" });
  await expect(page.locator("[data-slot=menu-bar]")).toBeVisible();
  const look = await page.evaluate(() => ({
    shadow: getComputedStyle(document.getElementById("win-home")!).boxShadow,
    radius: getComputedStyle(document.querySelector("[data-slot=hero-actions] button")!).borderRadius,
    menus: getComputedStyle(document.querySelector("[data-slot=browser-menus]")!).display,
    pattern: document.querySelector("[data-slot=desktop-pattern]") !== null,
  }));
  expect(look.shadow, "a window casts System 7's hard shadow").toMatch(/2px 2px 0px/);
  expect(look.radius, "its buttons are round").toBe("6px");
  expect(look.menus, "the browser's menus leave its window for the menu bar").toBe("none");
  expect(look.pattern, "and the desktop is the grey pattern").toBe(true);

  const frames = await track(page);
  await page.locator("[data-slot=desktop-icons] li[data-app=guestbook] button").click();
  await expect(page.locator("#win-guestbook")).toBeVisible();
  await page.waitForTimeout(500);
  const menu = await centre(page, "[data-slot=window-menu]");
  await frames.reset();
  await page.getByRole("button", { name: "Minimize guestbook.cgi" }).click();
  await expect(page.locator("#win-guestbook")).toBeHidden();
  const going = (await frames.frames()).filter((f) => f !== null);
  const end = going.at(-1)!;
  expect(going.length, "collapsing moves").toBeGreaterThan(5);
  expect(Math.abs(end.x - menu.x), "and ends on the windows menu").toBeLessThan(12);
  expect(Math.abs(end.y - menu.y)).toBeLessThan(12);

  await page.setViewportSize({ width: 320, height: 800 });
  const { scroll, inner } = await page.evaluate(() => ({ scroll: document.documentElement.scrollWidth, inner: window.innerWidth }));
  expect(scroll, "nothing scrolls sideways at 320 px").toBeLessThanOrEqual(inner);

  await page.locator("[data-bar=owl]").click();
  await page.getByRole("menu", { name: "Owlhead" }).getByRole("menuitem", { name: "PC" }).click();
  await expect(page.locator("[data-slot=taskbar]")).toBeVisible();
});

test("with motion reduced, or from the keyboard, a window opens where it stands without moving", async ({ browser }, info) => {
  for (const reduced of [true, false]) {
    const context = await browser.newContext({ reducedMotion: reduced ? "reduce" : "no-preference", colorScheme: info.project.use.colorScheme });
    const page = await context.newPage();
    await open(page, 1440);
    const frames = await track(page);
    await frames.reset();
    const button = page.locator("[data-slot=hero-actions]").getByRole("button", { name: "Sign the guestbook" });
    if (reduced) await button.click();
    else {
      await button.focus();
      await page.keyboard.press("Enter");
    }
    await expect(page.locator("#win-guestbook")).toBeVisible();
    await page.waitForTimeout(400);
    const shown = (await frames.frames()).filter((f) => f !== null);
    const sizes = new Set(shown.map((f) => `${Math.round(f!.x)},${Math.round(f!.y)},${Math.round(f!.w)}`));
    expect(sizes.size, reduced ? "reduced motion: one place and size throughout" : "keyboard: one place and size throughout").toBe(1);
    await context.close();
  }
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
  // measured against that scroll root, and lands in its first tenth, whatever small offset the
  // heading keeps from the window's edge.
  await expect
    .poll(() =>
      page.locator("#how").evaluate((el) => {
        const root = el.closest("[data-scroll-root]")!;
        const top = el.getBoundingClientRect().top - root.getBoundingClientRect().top;
        return top >= 0 && top <= root.clientHeight / 10;
      }),
    )
    .toBe(true);
});

test("the contents selects the section being read, and the status bar shows where a hovered link goes", async ({ page }) => {
  await open(page, 1440);
  const contents = page.getByRole("navigation", { name: "Contents" });
  await page.locator("#safety").evaluate((el) => el.scrollIntoView());
  await expect(contents.locator("[aria-current=location]")).toHaveText("How it stays in check");
  await contents.getByRole("link", { name: "Who it's for" }).hover();
  await expect(page.locator("[data-slot=status-text]")).toHaveText("http://www.owlhead.ai/#who");
  await page.mouse.move(0, 0);
  await expect(page.locator("[data-slot=status-text]")).toHaveText("Document: Done");
});

test("390 px: the record's window fits the phone without scrolling sideways", async ({ page }) => {
  await open(page, 390);
  await page.evaluate(() => document.fonts.ready);
  const record = await openWindow(page, "See why it traded", "The record - Example decision");
  const table = record.locator("[data-slot=record-trace] table");
  await expect(table).toBeVisible();
  const fit = await table.evaluate((t) => t.parentElement!.clientWidth > 0 && t.scrollWidth <= t.parentElement!.clientWidth);
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
  const record = await openWindow(page, "See why it traded", "The record - Example decision");
  await expect(record.getByText("Chain check: all 8 lines match.")).toBeVisible();
  await record.getByRole("button", { name: "Edit line 3" }).click();
  await expect(record.getByText(/Chain check: fails at line 3/)).toBeVisible();
  await record.getByRole("button", { name: "Undo the edit" }).click();
  await expect(record.getByText("Chain check: all 8 lines match.")).toBeVisible();
});

test("the guestbook sends the email and use, and says you're on the list", async ({ page }) => {
  let sent: unknown = null;
  await page.route("**/api/beta", async (route) => {
    sent = route.request().postDataJSON();
    await route.fulfill({ status: 201, contentType: "application/json", body: '{"ok":true}' });
  });
  await open(page, 390);
  const guestbook = await openWindow(page, "Sign the guestbook", "guestbook.cgi");
  await guestbook.getByLabel("Email address:").fill("ada@example.com");
  await guestbook.getByRole("radio", { name: "Running a trading desk" }).check();
  await guestbook.getByRole("button", { name: "Sign the guestbook" }).click();
  await expect(guestbook.locator("[data-slot=beta-done]")).toContainText("We'll write to ada@example.com");
  expect(sent).toEqual({ email: "ada@example.com", role: "desk", website: "" });
});
