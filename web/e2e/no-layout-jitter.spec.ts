import { type Page, expect, test } from "@playwright/test";

/**
 * Ages tick every second ("as of 14:05:18, 15 s ago"). As one changes, nothing above the page may
 * move: the header keeps its height and `main` keeps its top. The clock is paused before the
 * runtime starts, so each check lands on an exact age; each boundary is read just before and just
 * after it, at widths across the range where the status strip used to wrap.
 */

const WIDTHS = Array.from({ length: 121 }, (_, i) => 640 + i * 8);
const START = new Date("2026-09-28T18:05:20Z");

interface Case {
  scenario: string;
  /** How old the market data is when the page loads, in seconds. */
  initialAge: number;
  /** Ages to read the layout at, in seconds, ascending. */
  ages: number[];
}

const CASES: Case[] = [
  { scenario: "normal", initialAge: 2, ages: [9, 10, 59, 60, 599, 600] },
  { scenario: "stale", initialAge: 189, ages: [599, 600, 3599, 3600] },
];

async function layoutAt(page: Page, width: number) {
  await page.setViewportSize({ width, height: 900 });
  return page.evaluate(() => ({
    header: document.querySelector("header")!.getBoundingClientRect().height,
    mainTop: document.querySelector("main")!.getBoundingClientRect().top + window.scrollY,
  }));
}

/**
 * No trace: a case is 1,000 to 1,500 actions (a resize and a read at 121 widths for every age), and a
 * trace snapshots the page's DOM at every one, which tripled the run and took it past its budget on a
 * loaded runner. A failure names its width and age, which is what a trace would have shown.
 */
test.use({ trace: "off" });

for (const { scenario, initialAge, ages } of CASES) {
  test(`${scenario}: the header and main do not move as ages tick`, async ({ page }) => {
    test.setTimeout(90_000);
    await page.clock.install({ time: START });
    await page.clock.pauseAt(START);
    await page.goto(`/?scenario=${scenario}`);
    await page.waitForFunction(() => Object.keys(document.querySelector("header") ?? {}).some((k) => k.startsWith("__reactFiber")));
    await expect(page.locator("[data-slot=status-strip][data-degraded]")).toHaveCount(scenario === "normal" ? 0 : 1);

    let elapsed = 0;
    const baseline = new Map<number, Awaited<ReturnType<typeof layoutAt>>>();
    for (const age of ages) {
      const step = age - initialAge - elapsed;
      if (step > 1) await page.clock.fastForward((step - 1) * 1000);
      await page.clock.runFor(1000);
      elapsed = age - initialAge;
      for (const width of WIDTHS) {
        const now = await layoutAt(page, width);
        const first = baseline.get(width);
        if (!first) baseline.set(width, now);
        else expect(now, `${width} px at ${age} s old`).toEqual(first);
      }
    }
  });
}
