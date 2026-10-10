// The app's own screens for the landing page's browser tab (DEC-905): Home with a request waiting,
// the request, an agent, its record and its mandate, on the fixture workspace at 1440 by 900 in
// both themes, as JPEG files under public/app/. They are the product as it renders, not mock-ups;
// run this again after a change to any of those screens.
//
// Usage: a dev server that answers `?scenario=` (`npm run dev`), then
// `SHOTS_BASE=http://127.0.0.1:4318 node scripts/app-screens.mjs`.

import { mkdir } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright";
import { HIDE_PANEL_CSS, chartsDrawn } from "./shots.mjs";

const BASE = process.env.SHOTS_BASE ?? "http://127.0.0.1:4317";
const OUT = path.resolve("public/app");

const AGENT = "agt_01JB3K8Y4N7QW2M6R9T5V0XZAC";
const APPROVAL = "apr_01JBM9S346Q3D25VT4F5V37E3S";

/** Names match `APP_SCREENS` in `src/components/site/app-tab.tsx`. */
const SCREENS = [
  ["home", "/?scenario=approvals"],
  ["request", `/approvals/${APPROVAL}?scenario=approvals`],
  ["agent", `/agents/${AGENT}?scenario=normal`],
  ["record", `/agents/${AGENT}/decisions?scenario=normal`],
  ["mandate", `/agents/${AGENT}/mandate?scenario=normal`],
];

async function main() {
  await mkdir(OUT, { recursive: true });
  const browser = await chromium.launch(process.env.SHOTS_CHROMIUM ? { executablePath: process.env.SHOTS_CHROMIUM } : {});
  for (const theme of ["light", "dark"]) {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: theme, deviceScaleFactor: 1 });
    const page = await context.newPage();
    for (const [name, url] of SCREENS) {
      const file = path.join(OUT, `${name}-${theme}.jpg`);
      await page.goto(`${BASE}${url}`, { waitUntil: "networkidle", timeout: 60_000 });
      await page.addStyleTag({ content: HIDE_PANEL_CSS });
      await chartsDrawn(page, path.basename(file));
      await page.waitForTimeout(900);
      await page.screenshot({ path: file, type: "jpeg", quality: 82 });
      process.stdout.write(`${file}\n`);
    }
    await context.close();
  }
  await browser.close();
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
