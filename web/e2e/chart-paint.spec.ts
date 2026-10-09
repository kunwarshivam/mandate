import { type Page, expect, test } from "@playwright/test";

/**
 * The account chart's paint, read from the canvas itself (web/design/plan.md section B). The canvas
 * is drawn on the client only, after hydration; a chart that took the server's light while
 * hydrating a dark page drew a light chart, tore it down and drew a dark one, so its first paint
 * came later than in light. Each spec waits for pixels, never for a fixed time: once the plot shows
 * its line, it must be on the theme's own card, and it must be the only chart drawn. Switching the
 * theme just after the chart first appears must redraw it, painted, in the other theme.
 */

const HOST = "[data-slot=account-equity] [data-slot=chart-canvas]";

type Scheme = "light" | "dark";

interface Paint {
  /** How many charts Lightweight Charts built in the account's host since the page started. */
  made: number;
  /** Frames whose plot was painted on a card of the other theme. */
  wrongTheme: number;
  /** The plot's background, and how many pixels differ from it: the line, the fill, the labels. */
  background: [number, number, number] | null;
  marks: number;
  mode: string | undefined;
  card: [number, number, number];
}

declare global {
  interface Window {
    chartPaint(): Paint;
  }
}

/** Records every chart built in the account's host and checks each frame's paint against the theme. */
async function watch(page: Page) {
  await page.addInitScript((host) => {
    let made = 0;
    let wrongTheme = 0;
    const rgb = (css: string): [number, number, number] => {
      const c = document.createElement("canvas");
      c.width = c.height = 1;
      const ctx = c.getContext("2d")!;
      ctx.fillStyle = css;
      ctx.fillRect(0, 0, 1, 1);
      const d = ctx.getImageData(0, 0, 1, 1).data;
      return [d[0], d[1], d[2]];
    };
    const card = (): [number, number, number] => {
      const probe = document.createElement("div");
      probe.style.backgroundColor = "var(--card)";
      document.body.append(probe);
      const value = getComputedStyle(probe).backgroundColor;
      probe.remove();
      return rgb(value);
    };
    const near = (a: number[], b: number[]) => a.every((v, i) => Math.abs(v - b[i]) <= 3);
    const plot = () => [...(document.querySelector(host)?.querySelectorAll("canvas") ?? [])].find((c) => c.width > 0 && c.height > 0);
    window.chartPaint = () => {
      const canvas = plot();
      const mine = card();
      if (!canvas) return { made, wrongTheme, background: null, marks: 0, mode: document.documentElement.dataset.mode, card: mine };
      const d = canvas.getContext("2d")!.getImageData(0, 0, canvas.width, canvas.height).data;
      const background: [number, number, number] | null = d[3] === 255 ? [d[0], d[1], d[2]] : null;
      let marks = 0;
      if (background) for (let p = 0; p < d.length; p += 4) if (d[p + 3] === 255 && !near([d[p], d[p + 1], d[p + 2]], background)) marks++;
      return { made, wrongTheme, background, marks, mode: document.documentElement.dataset.mode, card: mine };
    };
    new MutationObserver((records) => {
      for (const r of records) {
        if (!(r.target instanceof Element) || !r.target.closest(host)) continue;
        for (const n of r.addedNodes) if (n instanceof Element && n.classList.contains("tv-lightweight-charts")) made++;
      }
    }).observe(document, { childList: true, subtree: true });
    const frame = () => {
      const canvas = plot();
      if (canvas && document.body) {
        const px = canvas.getContext("2d")!.getImageData(0, 0, 1, 1).data;
        if (px[3] === 255 && !near([px[0], px[1], px[2]], card())) wrongTheme++;
      }
      requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
  }, HOST);
}

/** Waits until the plot is painted on the current theme's card with its line drawn. */
async function painted(page: Page): Promise<Paint> {
  await expect
    .poll(
      async () => {
        const p = await page.evaluate(() => window.chartPaint());
        return p.background !== null && p.background.every((v, i) => Math.abs(v - p.card[i]) <= 3) && p.marks > 200;
      },
      { message: "the account chart shows its line on the theme's card", timeout: 30_000 },
    )
    .toBe(true);
  return page.evaluate(() => window.chartPaint());
}

function schemeOf(project: { use: { colorScheme?: string | null } }): Scheme {
  return project.use.colorScheme === "dark" ? "dark" : "light";
}

for (const width of [390, 1440]) {
  test(`Home's account chart paints once, on the theme's card, at ${width}px`, async ({ page }, testInfo) => {
    const scheme = schemeOf(testInfo.project);
    await page.setViewportSize({ width, height: 900 });
    await watch(page);
    await page.goto("/");
    const paint = await painted(page);
    expect(paint.mode).toBe(scheme);
    expect(paint.made, "one chart drawn on load: no redraw from the server's light").toBe(1);
    expect(paint.wrongTheme, "no frame painted on the other theme's card").toBe(0);
  });
}

/**
 * Critique C-22 found the chart blank in light and dark alike, on the normal, paused and
 * reconciliation Homes at 1440px: captures taken before hydration drew the canvas, not a paint that
 * stayed blank. Each of those Homes must show its line once the page has loaded.
 */
for (const scenario of ["paused", "reconciliation"]) {
  test(`Home's account chart paints on the theme's card under the ${scenario} scenario at 1440px`, async ({ page }, testInfo) => {
    const scheme = schemeOf(testInfo.project);
    await page.setViewportSize({ width: 1440, height: 900 });
    await watch(page);
    await page.goto(`/?scenario=${scenario}`);
    const paint = await painted(page);
    expect(paint.mode).toBe(scheme);
    expect(paint.made, "one chart drawn on load").toBe(1);
    expect(paint.wrongTheme, "no frame painted on the other theme's card").toBe(0);
  });
}

test("switching the theme just after the chart appears redraws it, painted, in the other theme", async ({ page }, testInfo) => {
  const scheme = schemeOf(testInfo.project);
  const other: Scheme = scheme === "dark" ? "light" : "dark";
  await page.setViewportSize({ width: 1440, height: 900 });
  await watch(page);
  await page.goto("/");
  await expect(page.locator(`${HOST} canvas`).first()).toBeAttached();
  await page.emulateMedia({ colorScheme: other });
  await expect.poll(() => page.evaluate(() => document.documentElement.dataset.mode)).toBe(other);
  const after = await painted(page);
  expect(after.mode).toBe(other);
  await page.emulateMedia({ colorScheme: scheme });
  await expect.poll(() => page.evaluate(() => document.documentElement.dataset.mode)).toBe(scheme);
  const back = await painted(page);
  expect(back.mode).toBe(scheme);
});

test("with reduced motion the chart paints on the theme's card, with no draw-in", async ({ page }, testInfo) => {
  const scheme = schemeOf(testInfo.project);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await watch(page);
  await page.goto("/");
  const paint = await painted(page);
  expect(paint.mode).toBe(scheme);
  const motion = await page.locator(HOST).evaluate((el) => {
    const style = getComputedStyle(el);
    return { clip: style.clipPath, duration: style.transitionDuration };
  });
  expect(motion, "no clip and no transition: the plot is whole from its first frame").toEqual({ clip: "none", duration: "0s" });
  expect(paint.made).toBe(1);
});
