import { type Locator, type Page, expect, test } from "@playwright/test";
import { AGENT_IDS } from "../src/fixtures/workspace";

/**
 * The hero figure: the balance in the display size, in proportional lining figures, with its cents at
 * half size, muted and raised to the digits' cap height, read as one value; the change on a soft pill
 * toned by its sign, with the disclosure symbol beside it. Scrubbing the chart changes the figure's glyphs
 * and the pill's, and nothing else moves: not the figure's box, the pill's place or the chart.
 */

const HEROES = [
  { name: "home", path: "/", value: "[data-slot=account-equity-value]" },
  { name: "an agent", path: `/agents/${AGENT_IDS.swing}`, value: "[data-slot=agent-equity-value]" },
];

/** How much of the canvas is the plot: an agent's chart keeps its price axis on the right. */
const PLOT = 0.7;

async function tokenColor(page: Page, token: string): Promise<string> {
  return page.evaluate((token) => {
    const probe = document.createElement("div");
    probe.style.color = `var(${token})`;
    document.body.append(probe);
    const value = getComputedStyle(probe).color;
    probe.remove();
    return value;
  }, token);
}

async function open(page: Page, path: string, width: number) {
  await page.setViewportSize({ width, height: 900 });
  await page.goto(path, { waitUntil: "networkidle" });
  await page.evaluate(() => document.fonts.ready);
  await expect(page.locator("[data-slot=cents]").first()).toBeVisible();
}

/** Hovers the hero chart at a fraction of its width and returns what the figure reads then. */
async function scrubAt(page: Page, canvas: Locator, fraction: number) {
  const box = (await canvas.boundingBox())!;
  await page.mouse.move(box.x + Math.max(1, box.width * fraction), box.y + box.height / 2);
}

async function layout(page: Page, value: string) {
  return page.evaluate((value) => {
    const figure = document.querySelector(value)!;
    const section = figure.closest("section")!;
    const box = figure.getBoundingClientRect();
    const pill = document.querySelector("[data-slot=hero-change]")!.getBoundingClientRect();
    const chart = section.querySelector("[data-slot=chart-canvas]")!.getBoundingClientRect();
    return {
      figure: { top: box.top + window.scrollY, left: box.left, width: box.width, height: box.height },
      pill: { top: pill.top + window.scrollY, left: pill.left, height: pill.height },
      line: document.querySelector("[data-slot=hero-change]")!.parentElement!.getBoundingClientRect().height,
      chart: { top: chart.top + window.scrollY, left: chart.left, width: chart.width, height: chart.height },
    };
  }, value);
}

for (const { name, path, value } of HEROES) {
  test(`${name}: the balance is set large and tight, with its cents half size, muted and raised`, async ({ page }) => {
    await open(page, path, 1440);
    const figure = page.locator(value);
    const type = await figure.evaluate((el) => {
      const s = getComputedStyle(el);
      const cents = el.querySelector("[data-slot=cents]")!;
      const c = getComputedStyle(cents);
      return {
        size: parseFloat(s.fontSize),
        weight: s.fontWeight,
        tracking: parseFloat(s.letterSpacing) / parseFloat(s.fontSize),
        leading: parseFloat(s.lineHeight) / parseFloat(s.fontSize),
        numerals: s.fontVariantNumeric,
        centsSize: parseFloat(c.fontSize),
        centsRaise: parseFloat(c.verticalAlign),
        centsColor: c.color,
        centsText: cents.textContent,
      };
    });
    expect(type.size).toBe(76);
    expect(type.weight).toBe("600");
    expect(type.tracking).toBeCloseTo(-0.03, 3);
    expect(type.leading).toBeCloseTo(0.95, 3);
    // Tabular digits leave visible gaps at 76px, so the hero alone is proportional.
    expect(type.numerals).toContain("proportional-nums");
    expect(type.numerals).toContain("lining-nums");
    expect(type.numerals).not.toContain("tabular-nums");
    expect(type.centsSize).toBe(38);
    expect(type.centsText).toMatch(/^\.\d{2}$/);
    // Raised by the cents' own cap height, so their tops meet the digits' tops.
    expect(type.centsRaise / type.centsSize).toBeGreaterThan(0.6);
    expect(type.centsRaise / type.centsSize).toBeLessThan(0.8);
    expect(type.centsColor).toBe(await tokenColor(page, "--muted-foreground"));
  });

  test(`${name}: the figure is one value to assistive technology`, async ({ page }) => {
    await open(page, path, 1440);
    const figure = page.locator(value);
    const text = (await figure.locator(".sr-only").textContent())!;
    expect(text).toMatch(/^−?\$[\d,]+\.\d{2}$/);
    await expect(figure).toMatchAriaSnapshot(`- paragraph: "${text}"`);
    const canvas = page.locator(`section:has(${value}) [data-slot=chart-canvas]`);
    await scrubAt(page, canvas, 0.4);
    await expect(figure.locator("[data-instant]")).toHaveCount(1);
    const scrubbed = (await figure.locator(".sr-only").textContent())!;
    await expect(figure).toMatchAriaSnapshot(`- paragraph: "${scrubbed}"`);
  });

  test(`${name}: scrubbing changes only the glyphs of the figure and the pill, and nothing moves`, async ({ page }) => {
    for (const width of [1440, 390]) {
      await open(page, path, width);
      const figure = page.locator(value);
      const canvas = page.locator(`section:has(${value}) [data-slot=chart-canvas]`);
      // Centred, so neither the header nor the phone tab bar sits over the line.
      await canvas.evaluate((el) => el.scrollIntoView({ block: "center", behavior: "instant" }));
      const now = (await figure.locator(".sr-only").textContent())!;
      const still = await layout(page, value);
      const seen = new Set<string>();
      for (let i = 0; i <= 24; i++) {
        await scrubAt(page, canvas, PLOT * (i / 24));
        await expect(figure.locator("[data-instant]"), `${width} px at ${i}/24`).toHaveCount(1);
        const read = await figure.evaluate((el) => ({
          spoken: el.querySelector(".sr-only")!.textContent,
          drawn: el.querySelector("[aria-hidden=true]")!.textContent,
        }));
        expect(read.drawn, `${width} px at ${i}/24`).toBe(read.spoken);
        seen.add(read.spoken!);
        expect(await layout(page, value), `${width} px at ${i}/24`).toEqual(still);
      }
      expect(seen.size, `${width} px`).toBeGreaterThan(5);
      await page.mouse.move(0, 0);
      await expect(figure.locator(".sr-only")).toHaveText(now);
      await expect(figure.locator("[data-instant]")).toHaveCount(0);
      expect(await layout(page, value)).toEqual(still);
    }
  });
}

/** A pointer cannot land on the range's first point reliably, so the exact zero is `charts.test.tsx`'s. */
test("the change sits on a pill tinted by its sign, and colour-blind friendly when asked", async ({ page }) => {
  for (const cvd of [false, true]) {
    await open(page, "/", 1440);
    // Outside `next dev` the preference has no switch yet, so the attribute it sets is set directly.
    if (cvd) await page.evaluate(() => (document.documentElement.dataset.cvd = "on"));
    const pill = page.locator("[data-slot=hero-change]");
    const canvas = page.locator("[data-slot=account-equity] [data-slot=chart-canvas]");
    const want = {
      gain: { bg: await tokenColor(page, cvd ? "--gain-cvd-soft" : "--gain-soft"), fg: await tokenColor(page, cvd ? "--gain-cvd" : "--gain"), word: "gain" },
      loss: { bg: await tokenColor(page, cvd ? "--loss-cvd-soft" : "--loss-soft"), fg: await tokenColor(page, cvd ? "--loss-cvd" : "--loss"), word: "loss" },
      flat: { bg: await tokenColor(page, "--muted"), fg: await tokenColor(page, "--foreground"), word: "no change" },
    };
    const seen = new Set<string>();
    for (const fraction of Array.from({ length: 41 }, (_, i) => PLOT * (i / 40))) {
      await scrubAt(page, canvas, fraction);
      const tone = (await pill.getAttribute("data-tone")) as keyof typeof want;
      seen.add(tone);
      const paint = await pill.evaluate((el) => {
        const s = getComputedStyle(el);
        return { bg: s.backgroundColor, fg: s.color, radius: parseFloat(s.borderTopLeftRadius), height: el.getBoundingClientRect().height, image: s.backgroundImage };
      });
      expect(paint.bg, `${tone} at ${fraction}`).toBe(want[tone].bg);
      expect(paint.fg, `${tone} at ${fraction}`).toBe(want[tone].fg);
      expect(paint.image).toBe("none");
      expect(paint.radius).toBeGreaterThanOrEqual(paint.height / 2);
      await expect(pill.locator("[data-direction]")).toContainText(want[tone].word);
    }
    expect(seen).toContain("gain");
    expect(seen).toContain("loss");
    const symbol = page.locator("[data-slot=hero-change] + [data-slot=disclosure]").getByRole("button", { name: "Performance disclosure" });
    await expect(symbol).toBeVisible();
    await expect(symbol).toHaveAccessibleDescription("[[DISCLOSURE-PERFORMANCE]]");
    const line = await pill.evaluate((el) => {
      const disclosure = el.parentElement!.querySelector(":scope > [data-slot=disclosure] button")!;
      const a = el.getBoundingClientRect();
      const b = disclosure.getBoundingClientRect();
      return { sameLine: Math.abs(a.top + a.height / 2 - (b.top + b.height / 2)) < 2, fits: a.width < el.parentElement!.getBoundingClientRect().width };
    });
    expect(line).toEqual({ sameLine: true, fits: true });
  }
});

for (const width of [320, 360, 390]) {
  test(`at ${width} px a seven-digit balance and its change fit, gain or loss`, async ({ page }) => {
    await open(page, "/", width);
    for (const [whole, cents] of [
      ["$1,234,567", ".89"],
      ["−$1,234,567", ".89"],
    ]) {
      const fit = await page.evaluate(
        ({ whole, cents }) => {
          const figure = document.querySelector("[data-slot=account-equity-value]")!;
          const copy = figure.querySelector("[aria-hidden=true]")!;
          const part = copy.querySelector("[data-slot=cents]")!;
          const main = [...copy.childNodes].find((n) => n.nodeType === Node.TEXT_NODE)!;
          const before = { main: main.textContent, cents: part.textContent, height: figure.getBoundingClientRect().height };
          main.textContent = whole;
          part.textContent = cents;
          const range = document.createRange();
          range.selectNodeContents(copy);
          const text = range.getBoundingClientRect();
          const section = figure.closest("section")!.getBoundingClientRect();
          const result = {
            inSection: text.left >= section.left - 0.5 && text.right <= section.right + 0.5,
            inViewport: text.right <= window.innerWidth,
            pageScrolls: document.documentElement.scrollWidth > window.innerWidth,
            height: figure.getBoundingClientRect().height,
            drawn: copy.textContent,
          };
          main.textContent = before.main;
          part.textContent = before.cents;
          return { ...result, heightBefore: before.height };
        },
        { whole, cents },
      );
      expect(fit.drawn).toBe(whole + cents);
      expect(fit, `${whole}${cents}`).toMatchObject({ inSection: true, inViewport: true, pageScrolls: false, height: fit.heightBefore });
    }
    const pill = await page.locator("[data-slot=hero-change]").evaluate((el) => el.getBoundingClientRect().right <= window.innerWidth);
    expect(pill).toBe(true);
  });
}

test("every figure but the hero keeps tabular digits: tables, key figures, rows and rails", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  for (const path of ["/", `/agents/${AGENT_IDS.swing}`, `/agents/${AGENT_IDS.lmn}/orders`, "/positions"]) {
    await page.goto(path, { waitUntil: "networkidle" });
    await page.evaluate(() => document.fonts.ready);
    const read = await page.evaluate(() => {
      const hero = document.querySelector("[data-slot=account-equity-value], [data-slot=agent-equity-value]");
      const figures = [...document.querySelectorAll<HTMLElement>(".tabular, .font-mono, .text-figure")].filter((el) => !hero?.contains(el) && el.checkVisibility());
      const digits = new Set<HTMLElement>();
      for (const figure of figures) {
        const walk = document.createTreeWalker(figure, NodeFilter.SHOW_TEXT);
        for (let n = walk.nextNode(); n; n = walk.nextNode()) if (/\d/.test(n.textContent ?? "") && n.parentElement?.checkVisibility()) digits.add(n.parentElement);
      }
      return {
        count: digits.size,
        inTables: [...digits].filter((el) => el.closest("table")).length,
        proportional: [...digits].filter((el) => !getComputedStyle(el).fontVariantNumeric.includes("tabular-nums")).map((el) => el.outerHTML.slice(0, 120)),
        hero: hero ? getComputedStyle(hero).fontVariantNumeric : null,
      };
    });
    expect(read.count, path).toBeGreaterThan(5);
    expect(read.proportional, path).toEqual([]);
    if (path === "/positions") expect(read.inTables, "the positions table is measured").toBeGreaterThan(0);
    if (read.hero) expect(read.hero, path).toContain("proportional-nums");
  }
});
