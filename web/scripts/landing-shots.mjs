// The product pictures on the long page under the desktop (DEC-907), taken from the example
// workspace at /demo in both themes, as PNG files under public/landing/. Each one is the app as it
// draws today over its fixture data; run this again after a change to a screen it shows.
//
// Usage: a server on http://127.0.0.1:4317 (`npm run dev` or `npm start`), then
// `node scripts/landing-shots.mjs`. LANDING_BASE=<url> points it at another server.

import { mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const BASE = process.env.LANDING_BASE ?? "http://127.0.0.1:4317";
const OUT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "public", "landing");

const AGENT = "agt_01JB3K8Y4N7QW2M6R9T5V0XZAC";
const SWING = "agt_01JB3K9P2H6SD4F8G1E3W7XYZB";
const APPROVAL = "apr_01JB3E28JT97KB6CQ643DZVMXX";

/**
 * Each picture: the links followed from Home inside the example app, the window it is taken at,
 * and the part of it kept. These sizes are the CSS pixels `Shot` declares; the files are twice that.
 */
export const LANDING_SHOTS = [
  { name: "agent", hops: [`/agents/${AGENT}`], width: 1280, height: 800 },
  { name: "thread", hops: ["/messages", `/messages/${SWING}`], width: 1280, height: 800 },
  {
    name: "gate",
    hops: [`/agents/${AGENT}/decisions/01JBWPQ5E6EYCNDY0YP57RCYBV`],
    width: 1024,
    height: 900,
    clip: { x: 0, y: 64, width: 1024, height: 720 },
  },
  { name: "request", hops: [`/approvals/${APPROVAL}`], width: 390, height: 844 },
  {
    name: "mandate",
    hops: [`/agents/${AGENT}`, `/agents/${AGENT}/mandate`],
    width: 1280,
    height: 1000,
    clip: { x: 0, y: 260, width: 1280, height: 600 },
  },
];

const browser = await chromium.launch();
await mkdir(OUT, { recursive: true });
for (const mode of ["light", "dark"]) {
  for (const shot of LANDING_SHOTS) {
    const context = await browser.newContext({ viewport: { width: shot.width, height: shot.height }, deviceScaleFactor: 2 });
    await context.addCookies([{ name: "owlhead-theme", value: mode, url: BASE }]);
    const page = await context.newPage();
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto(`${BASE}/demo`, { waitUntil: "networkidle" });
    for (const href of shot.hops) {
      await page.locator(`a[href="${href}"]:visible`).first().click();
      await page.waitForLoadState("networkidle");
    }
    await page.waitForTimeout(600);
    const file = path.join(OUT, `${shot.name}-${mode}.png`);
    await page.screenshot({ path: file, ...(shot.clip ? { clip: shot.clip } : {}) });
    console.log(path.relative(process.cwd(), file));
    await context.close();
  }
}
await browser.close();
