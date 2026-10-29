import { writeFileSync } from "node:fs";
import { expect, test } from "@playwright/test";

/**
 * The cell demo's warm-up (E11-9w): `next dev`, started by this config as every cell gesture starts
 * it, compiles each page the cell's specs open, so the trade loop's own run finds them compiled and
 * its 60 s `webServer` start is never spent on a first compile. The driver (`cargo xtask cell demo`)
 * runs it while the gestures' Job runs, before the first gesture from web/, and names the pages in
 * `OWLHEAD_CELL_WARM_PATHS`, comma separated. It checks nothing: each page's status, or why it did
 * not load, is written to `OWLHEAD_CELL_RESULT`, and a page that does not load stops nothing. Paper
 * only: nothing here reaches a broker, and no gesture is made.
 */
const RESULT = process.env.OWLHEAD_CELL_RESULT ?? "";
const PATHS = (process.env.OWLHEAD_CELL_WARM_PATHS ?? "").split(",").filter((path) => path !== "");
/** One page's bound: its first compile. */
const PAGE_MS = 30_000;

test("every page the cell's specs open, compiled once", async ({ page }) => {
  test.setTimeout(PATHS.length * PAGE_MS + 10_000);
  expect(RESULT, "the driver names RESULT").not.toBe("");
  expect(PATHS.length, "the driver names the pages").toBeGreaterThan(0);
  const loaded: Record<string, string> = {};
  for (const path of PATHS) {
    const response = await page.goto(path, { timeout: PAGE_MS }).catch((error: Error) => error);
    loaded[path] = response instanceof Error ? `not loaded: ${response.message.split("\n")[0]}` : String(response?.status() ?? "no response");
    writeFileSync(RESULT, JSON.stringify(loaded));
  }
});
