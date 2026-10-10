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

/** The box around every element the locators find, grown by `x` at the sides and `y` above and below, in whole CSS pixels. */
async function around(locators, { x = 0, y = 0 } = {}) {
  const boxes = [];
  for (const locator of locators) {
    const box = await locator.boundingBox();
    if (!box) throw new Error(`nothing to crop to: ${locator}`);
    boxes.push(box);
  }
  const left = Math.floor(Math.min(...boxes.map((b) => b.x)) - x);
  const top = Math.floor(Math.min(...boxes.map((b) => b.y)) - y);
  const right = Math.ceil(Math.max(...boxes.map((b) => b.x + b.width)) + x);
  const bottom = Math.ceil(Math.max(...boxes.map((b) => b.y + b.height)) + y);
  return { x: left, y: top, width: right - left, height: bottom - top };
}

/** `box` cut to the left and right edges of `edges`. */
const between = (box, edges) => ({ ...box, x: edges.x, width: edges.width });

/** A gate check by its name. */
const check = (page, name) => page.locator("[data-slot=gate-checks] > li").filter({ hasText: name });

/**
 * Each picture: the links followed from Home inside the example app, the window it is taken at, and
 * the part of it kept, either the whole window or the few elements that make the part's point, so
 * each one reads at the size the page draws it. `width` and `height` are the CSS pixels `Shot`
 * declares; the files are twice that.
 */
export const LANDING_SHOTS = [
  { name: "agent", hops: [`/agents/${AGENT}`], viewport: { width: 1280, height: 800 } },
  {
    name: "verdict",
    hops: [`/agents/${AGENT}/decisions/01JBWPQ5E6EYCNDY0YP57RCYBV`],
    viewport: { width: 1024, height: 1400 },
    crop: async (page) => {
      const verdict = page.locator("section").filter({ has: page.locator("[data-slot=verdict]") });
      return between(await around([verdict], { y: 20 }), await around([page.locator("[data-slot=gate-checks]")], { x: 20 }));
    },
  },
  {
    name: "check",
    hops: [`/agents/${AGENT}/decisions/01JBWPQ5E6EYCNDY0YP57RCYBV`],
    viewport: { width: 1024, height: 1400 },
    crop: (page) => around([check(page, "Re-entry cooldown"), check(page, "Order size")], { x: 20 }),
  },
  {
    name: "ask",
    hops: ["/messages", `/messages/${SWING}`],
    viewport: { width: 1280, height: 800 },
    crop: (page) => around([page.locator("[data-slot=request-card]").last(), page.locator("[data-slot=composer]")], { x: 16, y: 8 }),
  },
  { name: "request", hops: [`/approvals/${APPROVAL}`], viewport: { width: 390, height: 844 } },
  {
    name: "ladder",
    hops: [`/agents/${AGENT}`, `/agents/${AGENT}/mandate`],
    viewport: { width: 1280, height: 1000 },
    crop: async (page) => {
      const envelope = page.locator("[data-slot=envelope]");
      const ladder = envelope.locator("ol").first();
      return between(await around([envelope.locator("h2"), ladder], { y: 16 }), await around([ladder], { x: 20 }));
    },
  },
];

const browser = await chromium.launch();
await mkdir(OUT, { recursive: true });
for (const mode of ["light", "dark"]) {
  for (const shot of LANDING_SHOTS) {
    const context = await browser.newContext({ viewport: shot.viewport, deviceScaleFactor: 2 });
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
    const clip = shot.crop ? await shot.crop(page) : undefined;
    await page.screenshot({ path: file, ...(clip ? { clip } : {}) });
    const { width, height } = clip ?? shot.viewport;
    console.log(`${path.relative(process.cwd(), file)} ${width}x${height}`);
    await context.close();
  }
}
await browser.close();
