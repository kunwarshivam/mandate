#!/usr/bin/env node
/**
 * Screenshots the app for the landing page (DEC-212): real screens from the production build, at 2x,
 * in light and in dark, written to `public/site/` as `<name>-light.png` and `<name>-dark.png`.
 *
 *   npm run build && npm run site-shots
 *
 * The script starts `next start` on port 4342 against the build in `.next` and stops it when done.
 * To shoot a server that is already running, set `SITE_SHOTS_URL` (for example
 * `SITE_SHOTS_URL=http://127.0.0.1:4341 npm run site-shots`). Commit the images, and update
 * `SHOTS` in `src/components/site/shots.ts` when a shot's size changes; `shots.test.ts` reads each
 * file's size and fails when the two disagree.
 *
 * The shots show the app's fixture data on paper. The landing page shows no performance (DEC-212),
 * so every P&L figure, the hero change pill, the key figures, the loss-today rail and the performance
 * symbols are hidden before the shutter. The activity wire and the fixture tag are hidden too: they
 * belong to the live app, and the page says the screens use example data. The Stop sheet's rounded
 * corner is squared, so the frame on the page is its only edge.
 */
import { spawn } from "node:child_process";
import { mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const WEB = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const OUT_DIR = path.join(WEB, "public/site");
export const PORT = 4342;
export const SCALE = 2;
export const MODES = ["light", "dark"];

const AGENT = "agt_01JB3K9P2H6SD4F8G1E3W7XYZB";
const APPROVAL = "apr_01JBM9S346Q3D25VT4F5V37E3S";

const DESKTOP = { width: 1100, height: 720 };
const PHONE = { width: 390, height: 844 };

/** Everything that states a gain, a loss or a return, and the live-only chrome. */
const HIDE_CSS = `
  [data-slot="wire"], [data-slot="fixture-tag"], [data-slot="phone-footer"],
  [data-slot="key-figures"], p:has(> [data-slot="hero-change"]),
  [data-placeholder="performance"] { display: none !important; }
  *, *::before, *::after { caret-color: transparent !important; }
`;

/** Rows that carry a signed figure or name P&L go too, so no number on a shot reads as a result. */
function hidePerformanceRows() {
  const PNL = /P&L|since deployed/;
  for (const el of document.querySelectorAll("[data-direction]")) {
    const row = el.closest("p, li, tr, [role=row]") ?? el;
    row.style.setProperty("display", "none", "important");
  }
  const named = [...document.querySelectorAll("p, span, div, dt, dd, td, th, li, figcaption")].filter((el) => PNL.test(el.textContent ?? ""));
  for (const el of named) {
    if (named.some((other) => other !== el && el.contains(other))) continue;
    (el.closest("p, li, tr, dl > div") ?? el).style.setProperty("display", "none", "important");
  }
  for (const rail of document.querySelectorAll('[data-slot="limit-rail"]')) {
    if (/^\s*Loss today/.test(rail.textContent ?? "")) rail.style.setProperty("display", "none", "important");
  }
}

const hideDock = `[data-slot="dock"], nav[aria-label="Primary"] { display: none !important; }`;
const hideTabBar = `nav[aria-label="Main"] { display: none !important; }`;

/**
 * Each shot: a route, a viewport, what to hide, and either the viewport or one element, cut at
 * `maxHeight` CSS pixels when given. The files are `SCALE` times the CSS size.
 */
export const SHOTS = [
  { name: "hero-desktop", route: `/agents/${AGENT}`, viewport: DESKTOP, css: hideDock },
  { name: "hero-phone", route: "/", viewport: PHONE },
  { name: "step-mandate", route: `/agents/${AGENT}`, viewport: { width: 1100, height: 1000 }, css: hideDock, element: '[data-slot="mandate-card"]' },
  { name: "step-gate", route: "/", viewport: { width: 1100, height: 1400 }, css: hideDock, element: 'section[aria-labelledby="section-recent-activity"]' },
  { name: "step-gate-phone", route: "/", viewport: PHONE, css: hideTabBar, element: 'section[aria-labelledby="section-recent-activity"]', maxHeight: 560 },
  { name: "step-approve", route: `/approvals/${APPROVAL}`, viewport: PHONE },
  {
    name: "step-stop",
    route: `/agents/${AGENT}`,
    viewport: { width: 1100, height: 1100 },
    css: `${hideDock} [data-slot="stop-sheet"] { border-radius: 0 !important; }`,
    element: '[data-slot="stop-sheet"]',
    maxHeight: 720,
    async before(page) {
      await page.getByRole("button", { name: "Stop", exact: true }).first().click();
      await page.locator('[data-slot="stop-sheet"]').waitFor();
    },
  },
];

async function waitForServer(url, ms = 60_000) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    try {
      const res = await fetch(url);
      if (res.ok) return;
    } catch {}
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(`No server at ${url} after ${ms / 1000}s. Run \`npm run build\` first.`);
}

/** `next start` in its own process group, so stopping it also stops the `next-server` it forks. */
function startServer() {
  const next = path.join(WEB, "node_modules/.bin/next");
  const child = spawn(next, ["start", "-p", String(PORT), "-H", "127.0.0.1"], { cwd: WEB, stdio: "ignore", detached: true });
  return { url: `http://127.0.0.1:${PORT}`, stop: () => process.kill(-child.pid, "SIGTERM") };
}

async function shoot(browser, base, shot, mode) {
  const context = await browser.newContext({ viewport: shot.viewport, deviceScaleFactor: SCALE, colorScheme: mode, reducedMotion: "reduce" });
  const page = await context.newPage();
  await page.goto(base + shot.route, { waitUntil: "networkidle" });
  await page.addStyleTag({ content: HIDE_CSS + (shot.css ?? "") });
  await page.evaluate(hidePerformanceRows);
  if (shot.before) await shot.before(page);
  await page.evaluate(hidePerformanceRows);
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(400);
  const file = path.join(OUT_DIR, `${shot.name}-${mode}.png`);
  if (shot.element && shot.maxHeight) {
    const box = await page.locator(shot.element).first().boundingBox();
    const scrollY = await page.evaluate(() => window.scrollY);
    const clip = { x: box.x, y: box.y + scrollY, width: box.width, height: Math.min(box.height, shot.maxHeight) };
    await page.screenshot({ path: file, clip, fullPage: true, animations: "disabled" });
  } else if (shot.element) {
    await page.locator(shot.element).first().screenshot({ path: file, animations: "disabled" });
  } else {
    await page.screenshot({ path: file, animations: "disabled" });
  }
  await context.close();
  return file;
}

async function main() {
  const given = process.env.SITE_SHOTS_URL;
  const server = given ? { url: given.replace(/\/$/, ""), stop: () => {} } : startServer();
  try {
    await waitForServer(server.url);
    await mkdir(OUT_DIR, { recursive: true });
    const only = process.argv.slice(2);
    const browser = await chromium.launch();
    try {
      for (const shot of SHOTS.filter((s) => only.length === 0 || only.includes(s.name))) {
        for (const mode of MODES) console.log(path.relative(WEB, await shoot(browser, server.url, shot, mode)));
      }
    } finally {
      await browser.close();
    }
  } finally {
    server.stop();
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) await main();
