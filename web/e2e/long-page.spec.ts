import { type Page, expect, test } from "@playwright/test";

/**
 * The long page under the desktop (DEC-907) in a real browser, in light and dark (the two projects):
 * the desktop fills the first screen, the page rises over it with Sign in and Sign up in its bar, no
 * width from 320 px scrolls sideways, each picture of the app is the one for the page's theme, and
 * with motion reduced nothing on the page moves with the scroll, the owl rests on its first perch,
 * and the pixel thread is sewn whole at once.
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
  await expect(page.getByRole("heading", { level: 2, name: "Put an agent to work." })).toBeInViewport();
});

/** The pictures cut to one piece of a screen, which a wide window draws at the size they were taken. */
const PIECES = ["verdict", "check", "ask", "ladder"];

test("each picture of the app has loaded, it is the one for the page's theme, and none is drawn larger than it was taken", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await open(page, 1440);
  const mode = await page.evaluate(() => document.documentElement.dataset.mode);
  expect(mode === "light" || mode === "dark").toBe(true);
  const shots = page.locator("[data-slot=long-page] [data-slot=shot]");
  expect(await shots.count()).toBe(6);
  for (const shot of await shots.all()) {
    await shot.scrollIntoViewIfNeeded();
    const shown = shot.locator("img:visible");
    await expect(shown).toHaveCount(1);
    await expect.poll(() => shown.evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0)).toBe(true);
    expect(decodeURIComponent((await shown.getAttribute("src")) ?? "")).toContain(`-${mode}.png`);
    const name = (await shot.getAttribute("data-shot"))!;
    const [drawn, taken] = await shown.evaluate((img: HTMLImageElement) => [img.getBoundingClientRect().width, Number(img.getAttribute("width"))]);
    expect(drawn, `${name} is drawn no wider than it was taken`).toBeLessThanOrEqual(taken + 0.5);
    if (PIECES.includes(name)) expect(drawn, `${name} is drawn at the size it was taken`).toBeGreaterThanOrEqual(taken - 0.5);
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

for (const width of [390, 1024, 1280, 1440]) {
  test(`${width} px: the Scroll cue sits in the middle of the screen`, async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await open(page, width);
    const cue = page.locator("[data-slot=scroll-cue]");
    await expect(cue).toHaveAttribute("data-at", /status|edge/);
    const box = (await cue.boundingBox())!;
    expect(Math.abs(box.x + box.width / 2 - width / 2), "the cue's middle from the screen's").toBeLessThanOrEqual(1);
  });
}

test("the Scroll cue follows the browser window's status bar when the window is dragged", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await open(page, 1440);
  const cue = page.locator("[data-slot=scroll-cue]");
  const status = page.locator("[data-window=home] [data-slot=status-text]");
  await expect(cue).toHaveAttribute("data-at", "status");
  const middle = async () => {
    const box = (await status.boundingBox())!;
    return Math.round(box.y + box.height / 2);
  };
  const cueMiddle = async () => {
    const box = (await cue.boundingBox())!;
    return Math.round(box.y + box.height / 2);
  };
  const before = await middle();
  expect(await cueMiddle()).toBe(before);
  const bar = (await page.locator("[data-window=home] [data-slot=title-bar]").first().boundingBox())!;
  await page.mouse.move(bar.x + bar.width / 3, bar.y + bar.height / 2);
  await page.mouse.down();
  await page.mouse.move(bar.x + bar.width / 3, bar.y + bar.height / 2 - 30, { steps: 6 });
  await page.mouse.up();
  await expect.poll(middle, "the window moved up").toBeLessThan(before - 10);
  await expect.poll(async () => (await cueMiddle()) - (await middle()), "the cue stays on the status bar's row").toBe(0);
  await expect(cue).toHaveAttribute("data-at", "status");
});

test("with motion allowed, the owl peeks over the Scroll cue in the middle of the screen", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await open(page, 1440);
  const owl = page.locator("[data-slot=flying-owl]");
  await expect(owl).toHaveAttribute("data-owl", "peeking");
  await expect.poll(async () => {
    const box = (await owl.boundingBox())!;
    return Math.round(box.x + box.width / 2);
  }).toBe(720);
});

test("with motion allowed, the owl flies down the page and the pieces settle as they come into view", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await open(page, 1440);
  const owl = page.locator("[data-slot=flying-owl]");
  await expect(owl).toHaveAttribute("data-owl", "peeking");
  await expect(owl).toHaveAttribute("aria-hidden", "true");
  const where = () => owl.evaluate((el) => getComputedStyle(el).transform);
  await page.locator("#threads").scrollIntoViewIfNeeded();
  const at = await where();
  await page.locator("#limits").scrollIntoViewIfNeeded();
  await expect.poll(where).not.toBe(at);
  await expect(page.locator("#limits-title")).toHaveAttribute("data-shown", "");
  expect(await page.locator("[data-slot=long-page]").getAttribute("data-motion")).toBe("on");
});

test("with motion allowed, the owl lands, then draws nothing but a blink until the page moves again", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.addInitScript(() => {
    const clear = CanvasRenderingContext2D.prototype.clearRect;
    (window as unknown as { owlPaints: number }).owlPaints = 0;
    CanvasRenderingContext2D.prototype.clearRect = function (...args) {
      if (this.canvas.dataset.slot === "flying-owl") (window as unknown as { owlPaints: number }).owlPaints++;
      return clear.apply(this, args);
    };
  });
  await open(page, 1440);
  const owl = page.locator("[data-slot=flying-owl]");
  const paints = () => page.evaluate(() => (window as unknown as { owlPaints: number }).owlPaints);
  await page.locator("#threads").scrollIntoViewIfNeeded();
  await expect(owl).toHaveAttribute("data-owl", "perched", { timeout: 10_000 });
  const landed = await paints();
  await page.waitForTimeout(2000);
  expect((await paints()) - landed, "in two seconds, at most a blink's two paints").toBeLessThanOrEqual(2);
  expect(await owl.evaluate((el) => el.getAnimations().map((a) => (a as CSSAnimation).animationName)), "it bobs in CSS").toEqual(["owl-bob"]);
  await page.mouse.wheel(0, 400);
  await expect(owl).not.toHaveAttribute("data-owl", "perched");
  await expect(owl).toHaveAttribute("data-owl", "perched", { timeout: 10_000 });
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

const painted = (page: Page) =>
  page.locator("[data-slot=pixel-thread]").evaluate((canvas: HTMLCanvasElement) => {
    const { data } = canvas.getContext("2d")!.getImageData(0, 0, canvas.width, canvas.height);
    let n = 0;
    for (let i = 3; i < data.length; i += 4) if (data[i]! > 0) n++;
    return n;
  });

for (const width of [390, 1440]) {
  test(`${width} px: with motion reduced, the thread is sewn the whole way down at once, under the words`, async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await open(page, width);
    const thread = page.locator("[data-slot=pixel-thread]");
    await expect(thread).toHaveAttribute("aria-hidden", "true");
    await expect(thread).toHaveAttribute("data-thread", "whole");
    for (const part of ["#threads", "#asking", "#limits"]) {
      await page.locator(part).scrollIntoViewIfNeeded();
      await expect.poll(() => painted(page), `stitches beside ${part}`).toBeGreaterThan(0);
    }
    const title = (await page.locator("#limits-title").boundingBox())!;
    const onTop = await page.evaluate(([x, y]) => document.elementFromPoint(x!, y!)?.closest("#limits-title") !== null, [title.x + 4, title.y + title.height / 2]);
    expect(onTop, "the words sit over the thread").toBe(true);
  });
}

test("with motion allowed, the thread is sewn on as the visitor reads down", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await open(page, 1440);
  const thread = page.locator("[data-slot=pixel-thread]");
  await page.locator("#intro-title").scrollIntoViewIfNeeded();
  await expect(thread).toHaveAttribute("data-thread", "sewing");
  await page.evaluate(() => window.scrollTo({ top: document.documentElement.scrollHeight, behavior: "instant" }));
  await expect.poll(() => painted(page)).toBeGreaterThan(0);
});

for (const width of [390, 1440]) {
  test(`${width} px: the opening's picture floats on a pixel sea that leaves the words on paper`, async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await open(page, width);
    const sea = page.locator("[data-slot=long-page] [data-slot=pixel-sea]");
    await sea.scrollIntoViewIfNeeded();
    await expect(sea).toHaveAttribute("aria-hidden", "true");
    expect(await sea.locator("[data-slot=sea-wave]:visible").count(), "rows of waves down to the bottom of the sea").toBeGreaterThan(10);
    const water = (await sea.boundingBox())!;
    const shot = (await page.locator("[data-slot=shot][data-shot=agent]").boundingBox())!;
    expect(water.y, "the sea starts above the picture").toBeLessThan(shot.y);
    expect(water.y + water.height, "and runs below it").toBeGreaterThan(shot.y + shot.height);
    const words = (await page.locator("[data-slot=reassure]").boundingBox())!;
    expect(words.y + words.height, "the line under the buttons sits above the sea").toBeLessThanOrEqual(water.y);
    const onTop = await page.evaluate(([x, y]) => document.elementFromPoint(x!, y!)?.closest("[data-shot=agent]") !== null, [shot.x + shot.width / 2, shot.y + shot.height / 2]);
    expect(onTop, "the picture sits over the sea").toBe(true);
    expect(await sea.evaluate((el) => el.getAnimations({ subtree: true }).length), "the sea rests with motion reduced").toBe(0);
    expect(await risen(page), "the moon rests half risen").toBeCloseTo(0.5, 2);
  });
}

/** How much of the moon stands above the horizon, 0 to 1. */
const risen = (page: Page) =>
  page.locator("[data-slot=long-page] [data-slot=pixel-sea]").evaluate((sea) => {
    const moon = [...sea.querySelectorAll("[data-slot=sea-moon]")].map((m) => m.getBoundingClientRect()).find((r) => r.width > 0)!;
    return (sea.getBoundingClientRect().top - moon.top) / moon.height;
  });

test("with motion allowed, the water moves and the moon rises out of it as the opening comes into view", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await open(page, 1440);
  const sea = page.locator("[data-slot=long-page] [data-slot=pixel-sea]");
  await sea.evaluate((el) => window.scrollTo({ top: el.getBoundingClientRect().top + window.scrollY - window.innerHeight + 20, behavior: "instant" }));
  await expect.poll(() => risen(page), "the moon is still under the water as the sea comes in").toBeLessThan(0.3);
  await sea.evaluate((el) => window.scrollTo({ top: el.getBoundingClientRect().top + window.scrollY - 200, behavior: "instant" }));
  await expect.poll(() => risen(page), "and half risen once the opening is in view").toBeCloseTo(0.5, 2);
  expect(await sea.evaluate((el) => el.getAnimations({ subtree: true }).filter((a) => a.playState === "running").length)).toBeGreaterThan(5);
});

test("the lock screen behind the notification is a night in pixels, and the notification stays readable over it", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await open(page, 1440);
  await page.locator("#asking").scrollIntoViewIfNeeded();
  const night = page.locator("#asking [data-slot=pixel-night]");
  await expect(night).toHaveAttribute("aria-hidden", "true");
  await expect(night).toBeVisible();
  expect(await night.locator("rect").count()).toBeGreaterThan(100);
  const push = page.locator("#asking [data-reveal=drop]");
  await expect(push).toBeInViewport({ ratio: 1 });
  const box = (await push.boundingBox())!;
  const onTop = await page.evaluate(([x, y]) => document.elementFromPoint(x!, y!)?.closest("[data-reveal=drop]") !== null, [box.x + box.width / 2, box.y + box.height / 2]);
  expect(onTop, "the notification sits over the night").toBe(true);
});
