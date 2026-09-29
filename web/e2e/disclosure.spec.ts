import { type Locator, type Page, expect, test } from "@playwright/test";
import { AGENT_IDS } from "../src/fixtures/workspace";

/**
 * DEC-210: beside every P&L, the performance disclosure is an info symbol, muted, 16px in a 24px
 * target (44px on touch), that takes no more room than the text beside it. It opens the disclosure
 * text in a Kumo popover on hover, click, tap, or focus and Enter or Space; Escape closes it and gives
 * focus back. The text is the symbol's description, and print shows it in full in the symbol's place.
 */

const TEXT = "[[DISCLOSURE-PERFORMANCE]]";
const START = new Date("2026-09-28T18:05:20Z");
const REGION = "section, article, header, [role=region], li";
const PAGES = ["/", "/positions", `/agents/${AGENT_IDS.swing}`];

const triggers = (page: Page) => page.locator("main").getByRole("button", { name: "Performance disclosure" });
const popover = (page: Page) => page.getByRole("dialog", { name: "Performance disclosure" });
/** The hero's symbol sits just outside its change pill, on the pill's line. */
const heroTrigger = (page: Page) => page.locator("[data-slot=hero-change] + [data-slot=disclosure]").getByRole("button", { name: "Performance disclosure" });

async function open(page: Page, path: string, width: number, height = 900, still = false) {
  if (still) {
    await page.clock.install({ time: START });
    await page.clock.pauseAt(START);
  }
  await page.setViewportSize({ width, height });
  await page.goto(path);
  await page.waitForLoadState("networkidle");
  if (still) await page.clock.runFor(2000);
  await page.waitForFunction(() => {
    const all = Array.from(document.querySelectorAll('main button[aria-label="Performance disclosure"]'));
    return all.length > 0 && all.every((t) => Object.keys(t).some((k) => k.startsWith("__reactProps")));
  });
}

/** Signed P&L figures in `main` with no disclosure symbol in their region. */
async function undisclosed(page: Page): Promise<string[]> {
  return page.locator("main").evaluate((main, region) => {
    return Array.from(main.querySelectorAll("[data-direction]"))
      .filter((el) => !el.closest(region)?.querySelector('button[aria-label="Performance disclosure"]'))
      .map((el) => el.textContent ?? "");
  }, REGION);
}

async function expectOpens(page: Page, trigger: Locator) {
  await expect(popover(page)).toBeVisible();
  await expect(popover(page)).toContainText(TEXT);
  await expect(trigger).toHaveAttribute("aria-expanded", "true");
}

for (const width of [1440, 390, 360]) {
  test.describe(`at ${width}px`, () => {
    for (const path of PAGES) {
      test(`${path}: every P&L has the symbol beside it, named and described by the disclosure`, async ({ page }) => {
        await open(page, path, width);
        expect(await undisclosed(page)).toEqual([]);
        const all = triggers(page);
        expect(await all.count()).toBeGreaterThan(0);
        for (const t of await all.all()) {
          await expect(t).toBeVisible();
          await expect(t).toHaveAccessibleDescription(TEXT);
          await expect(t).toHaveAttribute("aria-expanded", "false");
        }
      });

      test(`${path}: the symbol holds at least 3:1 against what it sits on`, async ({ page }) => {
        await open(page, path, width);
        const ratios = await triggers(page).evaluateAll((els) => {
          const rgba = (c: string) => {
            const cv = document.createElement("canvas");
            cv.width = cv.height = 1;
            const ctx = cv.getContext("2d")!;
            ctx.fillStyle = c;
            ctx.fillRect(0, 0, 1, 1);
            return Array.from(ctx.getImageData(0, 0, 1, 1).data);
          };
          const lum = ([r, g, b]: number[]) => {
            const f = (v: number) => ((v /= 255) <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
            return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
          };
          return els.map((el) => {
            const fg = rgba(getComputedStyle(el.querySelector("svg")!).color);
            let bg = [0, 0, 0, 0];
            for (let n: Element | null = el; n && bg[3] < 255; n = n.parentElement) bg = rgba(getComputedStyle(n).backgroundColor);
            const [a, b] = [lum(fg), lum(bg)].sort((x, y) => y - x);
            return { fgAlpha: fg[3], ratio: (a + 0.05) / (b + 0.05) };
          });
        });
        for (const r of ratios) {
          expect(r.fgAlpha).toBe(255);
          expect(r.ratio).toBeGreaterThanOrEqual(3);
        }
      });
    }

    test("the symbol takes the line height of the text beside it, sits on its baseline, and opening it moves nothing", async ({ page }) => {
      await open(page, "/", width, 900, true);
      const rows = await triggers(page).evaluateAll((els) =>
        els.map((t) => {
          const row = t.closest("[data-slot=disclosure]")!.parentElement!;
          const range = (n: Node) => {
            const r = document.createRange();
            r.selectNodeContents(n);
            return r.getBoundingClientRect();
          };
          const zwsp = range(t.querySelector("span")!);
          const walker = document.createTreeWalker(row, NodeFilter.SHOW_TEXT, {
            acceptNode: (n) => (n.textContent!.trim() && !n.parentElement!.closest("button, .sr-only, [data-slot=disclosure]") ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP),
          });
          const neighbours: DOMRect[] = [];
          for (let n = walker.nextNode(); n; n = walker.nextNode()) {
            const r = range(n);
            const sameLine = r.top < zwsp.bottom && r.bottom > zwsp.top;
            if (sameLine && getComputedStyle(n.parentElement!).fontSize === getComputedStyle(t).fontSize) neighbours.push(r);
          }
          return {
            height: t.getBoundingClientRect().height,
            lineHeight: parseFloat(getComputedStyle(t).lineHeight),
            baselineGaps: neighbours.map((r) => Math.abs(zwsp.bottom - r.bottom)),
            glyph: t.querySelector("svg")!.getBoundingClientRect().width,
          };
        }),
      );
      // A phone's Home shows one P&L, the account hero's: its agent rows show headroom instead (DEC-207).
      expect(rows.length).toBeGreaterThanOrEqual(width >= 1024 ? 5 : 1);
      for (const r of rows) {
        expect(Math.abs(r.height - r.lineHeight)).toBeLessThanOrEqual(0.5);
        expect(r.baselineGaps.length, "never alone on its line: it wraps with the words before it").toBeGreaterThan(0);
        for (const gap of r.baselineGaps) expect(gap).toBeLessThanOrEqual(1);
        expect(r.glyph).toBe(16);
      }

      const layout = () =>
        page.locator("main").evaluate((main) => ({
          main: Math.round(main.getBoundingClientRect().height),
          figures: Array.from(main.querySelectorAll("[data-direction]")).map((el) => {
            const r = el.getBoundingClientRect();
            return [r.x, r.y, r.width, r.height].map(Math.round);
          }),
          rows: Array.from(main.querySelectorAll("[data-slot=disclosure]")).map((d) => Math.round(d.closest("p")!.getBoundingClientRect().height)),
        }));
      const closed = await layout();

      if (width >= 1024) {
        await page.locator("main").evaluate((main) => main.querySelectorAll<HTMLElement>("[data-slot=disclosure]").forEach((d) => (d.style.display = "none")));
        const withoutSymbols = await layout();
        await page.locator("main").evaluate((main) => main.querySelectorAll<HTMLElement>("[data-slot=disclosure]").forEach((d) => (d.style.display = "")));
        expect(withoutSymbols.figures, "no P&L figure moves for its symbol").toEqual(closed.figures);
        expect(withoutSymbols.rows, "no row is taller for its symbol").toEqual(closed.rows);
      }
      await page.emulateMedia({ media: "print" });
      const withTags = await layout();
      await page.emulateMedia({ media: "screen" });
      closed.rows.forEach((h, i) => expect(h, "no row is taller for the symbol than for the inline tag it replaced").toBeLessThanOrEqual(withTags.rows[i]));

      await heroTrigger(page).focus();
      await page.keyboard.press("Enter");
      await page.clock.runFor(500);
      await expectOpens(page, heroTrigger(page));
      expect(await layout(), "opening the popover moves nothing").toEqual(closed);
    });

    test("print shows the full text in the symbol's place", async ({ page }) => {
      await open(page, "/", width);
      const count = await triggers(page).count();
      await page.emulateMedia({ media: "print" });
      for (const t of await triggers(page).all()) await expect(t).toBeHidden();
      const shown = page.locator("main [data-slot=disclosure]").filter({ visible: true }).locator("[data-placeholder=performance]");
      await expect(shown).toHaveCount(count);
      for (const s of await shown.all()) {
        await expect(s).toBeVisible();
        await expect(s).toHaveText(TEXT);
        expect((await s.boundingBox())!.width).toBeGreaterThan(100);
      }
      await page.emulateMedia({ media: "screen" });
      await expect(triggers(page).first()).toBeVisible();
      for (const s of await shown.all()) expect((await s.boundingBox())!.width).toBeLessThanOrEqual(1);
    });
  });
}

test("at 1440px every agent row on Home keeps its P&L and the symbol beside it", async ({ page }) => {
  await open(page, "/", 1440);
  const rows = page.locator("main [data-slot=agent-band]").filter({ visible: true });
  expect(await rows.count()).toBeGreaterThan(1);
  for (const row of await rows.all()) {
    await expect(row.locator("[data-direction]").filter({ visible: true }).first()).toBeVisible();
    await expect(row.getByRole("button", { name: "Performance disclosure" })).toBeVisible();
  }
});

for (const width of [390, 360]) {
  test(`at ${width}px the phone's agent rows carry no P&L and no symbol, and every P&L left keeps its symbol`, async ({ page }) => {
    await open(page, "/", width);
    const rows = page.locator("main [data-slot=phone-agents] > li");
    expect(await rows.count()).toBeGreaterThan(1);
    for (const row of await rows.all()) {
      await expect(row).toBeVisible();
      await expect(row.locator("[data-direction]")).toHaveCount(0);
      await expect(row.getByRole("button", { name: "Performance disclosure" })).toHaveCount(0);
      await expect(row.locator("[data-slot=headroom]")).toBeVisible();
    }
    for (const path of ["/", `/agents/${AGENT_IDS.swing}`]) {
      await open(page, path, width);
      await expect(heroTrigger(page)).toBeVisible();
      const visible = await page.locator("main").evaluate((main, region) => {
        const shown = (el: Element) => (el as HTMLElement).checkVisibility();
        return Array.from(main.querySelectorAll("[data-direction]"))
          .filter(shown)
          .filter((el) => !Array.from(el.closest(region)?.querySelectorAll('button[aria-label="Performance disclosure"]') ?? []).some(shown))
          .map((el) => el.textContent ?? "");
      }, REGION);
      expect(visible, `${path}: every P&L on screen has a visible symbol in its region`).toEqual([]);
    }
  });
}

test.describe("with a mouse and keyboard, at 1440px", () => {
  test("hover opens it after a short delay, it stays open over the popover, and closes on leave", async ({ page }) => {
    await open(page, "/", 1440);
    const t = heroTrigger(page);
    await t.hover();
    await expectOpens(page, t);
    const box = (await popover(page).boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 4 });
    await page.waitForTimeout(400);
    await expect(popover(page)).toBeVisible();
    const top = await page.evaluate(({ x, y }) => document.elementFromPoint(x, y)?.closest("[role=dialog]") !== null, { x: box.x + box.width / 2, y: box.y + box.height / 2 });
    expect(top, "the popover is on top of the page").toBe(true);
    await page.mouse.move(20, 880, { steps: 4 });
    await expect(popover(page)).toBeHidden();
    await expect(t).toHaveAttribute("aria-expanded", "false");
  });

  test("a brush past the symbol does not open it", async ({ page }) => {
    await open(page, "/", 1440);
    const box = (await heroTrigger(page).boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.move(20, 880);
    await page.waitForTimeout(400);
    await expect(popover(page)).toBeHidden();
  });

  test("click toggles it", async ({ page }) => {
    await open(page, "/", 1440);
    const t = heroTrigger(page);
    await t.click();
    await expectOpens(page, t);
    await t.click();
    await expect(popover(page)).toBeHidden();
    await expect(t).toHaveAttribute("aria-expanded", "false");
  });

  test("the target is 24px around a 16px glyph", async ({ page }) => {
    await open(page, "/", 1440);
    const size = await heroTrigger(page).evaluate((t) => {
      const before = getComputedStyle(t, "::before");
      return { target: [before.width, before.height], glyph: [t.querySelector("svg")!.getBoundingClientRect().width, t.querySelector("svg")!.getBoundingClientRect().height] };
    });
    expect(size).toEqual({ target: ["24px", "24px"], glyph: [16, 16] });
  });

  test("focus alone does not open it; Enter and Space do; Escape closes it and gives focus back", async ({ page }) => {
    await open(page, "/", 1440);
    const t = heroTrigger(page);
    await t.focus();
    await page.waitForTimeout(400);
    await expect(popover(page)).toBeHidden();
    await expect(t).toBeFocused();
    const ring = await t.evaluate((el) => getComputedStyle(el, "::before").boxShadow);
    expect(ring, "a visible focus ring").not.toBe("none");

    for (const key of ["Enter", "Space"]) {
      await page.keyboard.press(key);
      await expectOpens(page, t);
      await page.keyboard.press("Escape");
      await expect(popover(page)).toBeHidden();
      await expect(t).toBeFocused();
    }
  });
});

test.describe("on a phone, at 390 × 844", () => {
  test.use({ isMobile: true, hasTouch: true, viewport: { width: 390, height: 844 } });

  test("a tap opens it, a tap outside closes it, and the hit area is 44px", async ({ page }) => {
    await open(page, "/", 390, 844);
    const t = heroTrigger(page);
    await t.scrollIntoViewIfNeeded();
    const target = await t.evaluate((el) => getComputedStyle(el, "::before").width);
    expect(target).toBe("44px");
    const box = (await t.boundingBox())!;
    const cx = box.x + box.width / 2;
    const cy = box.y + box.height / 2;

    const title = (await page.locator("[data-slot=account-equity-value]").boundingBox())!;
    const outside = () => page.touchscreen.tap(title.x + 8, title.y + title.height / 2);

    await page.touchscreen.tap(cx, cy);
    await expectOpens(page, t);
    await outside();
    await expect(popover(page)).toBeHidden();

    await page.touchscreen.tap(cx - 20, cy);
    await expectOpens(page, t);
    await outside();
    await expect(popover(page)).toBeHidden();
    await expect(page).toHaveURL(/\/$/);
  });
});
