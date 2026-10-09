import { type Page, expect, test } from "@playwright/test";
import { recordHref } from "../src/components/stop/commands";
import { AGENT_IDS, APPROVAL_IDS } from "../src/fixtures/workspace";
import { agentHref } from "../src/lib/screens";

/**
 * Stop is always one press away (brief §5, rule 13): at every width, on every main route, with the
 * page at its top or scrolled to its end, the Stop button at the end of the dock or the phone tab bar
 * (DEC-452) sits wholly inside the viewport, nothing covers it, and its label is never clipped. On
 * touch widths it is at least 44 by 44 px.
 */

const WIDTHS = [320, 360, 375, 390, 414, 430, 480, 600, 640, 768, 820, 1024, 1180, 1280, 1440, 1920];
/** Tailwind's `lg`: from here up the dock carries the nav, and below it the tab bar (DEC-207). */
const DESKTOP = 1024;
/** Phones and tablets, landscape included. */
const TOUCH_MAX = 1180;
const MIN_TARGET = 44;

interface ViewTransitionLog {
  started: number;
  running: number;
}

declare global {
  interface Window {
    __viewTransitions: ViewTransitionLog;
  }
}

/** Counts the document's view transitions, so a check can wait for none and a test can tell some ran. */
function logViewTransitions() {
  const log: ViewTransitionLog = { started: 0, running: 0 };
  window.__viewTransitions = log;
  const start = document.startViewTransition;
  if (!start) return;
  document.startViewTransition = function (this: Document, ...args: Parameters<typeof start>) {
    const transition = start.apply(this, args);
    log.started++;
    log.running++;
    transition.finished.finally(() => log.running--);
    return transition;
  } as typeof start;
}

test.beforeEach(async ({ page }) => {
  await page.addInitScript(logViewTransitions);
});

const ROUTES = [
  { name: "home", path: "/" },
  { name: "an agent", path: agentHref(AGENT_IDS.btc, "overview") },
  { name: "a position", path: agentHref(AGENT_IDS.swing, "positions") },
  { name: "approvals", path: "/approvals" },
  { name: "an approval request", path: `/approvals/${APPROVAL_IDS.swingXyz}` },
  { name: "the kill-switch record", path: recordHref("kill", AGENT_IDS.btc) },
];

async function expectStopVisible(page: Page, width: number, state: string) {
  const stop = page.getByRole("button", { name: "Stop", exact: true });
  await expect(stop, state).toBeVisible();
  const viewport = page.viewportSize()!;
  const found = await stop.evaluate(async (el) => {
    while (window.__viewTransitions.running > 0) await new Promise((frame) => requestAnimationFrame(frame));
    const r = el.getBoundingClientRect();
    const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    return {
      box: { left: r.left, top: r.top, right: r.right, bottom: r.bottom, width: r.width, height: r.height },
      uncovered: hit !== null && (hit === el || el.contains(hit)),
      covering: hit && hit !== el && !el.contains(hit) ? `<${hit.tagName.toLowerCase()} class="${(hit.getAttribute("class") ?? "").slice(0, 80)}">` : null,
      clipped: el.scrollWidth > el.clientWidth,
      documentWidth: document.documentElement.scrollWidth,
    };
  });
  const { box } = found;
  expect(box.left, `${state}: Stop's left edge`).toBeGreaterThanOrEqual(0);
  expect(box.top, `${state}: Stop's top edge`).toBeGreaterThanOrEqual(0);
  expect(box.right, `${state}: Stop's right edge against a ${viewport.width} px viewport`).toBeLessThanOrEqual(viewport.width);
  expect(box.bottom, `${state}: Stop's bottom edge`).toBeLessThanOrEqual(viewport.height);
  expect(found.covering, `${state}: the element at Stop's centre`).toBeNull();
  expect(found.uncovered, `${state}: Stop is the element at its own centre`).toBe(true);
  expect(found.clipped, `${state}: Stop's label fits`).toBe(false);
  expect(found.documentWidth, `${state}: the page does not scroll sideways`).toBeLessThanOrEqual(viewport.width);
  if (width <= TOUCH_MAX) {
    expect(box.width, `${state}: Stop's width`).toBeGreaterThanOrEqual(MIN_TARGET);
    expect(box.height, `${state}: Stop's height`).toBeGreaterThanOrEqual(MIN_TARGET);
  }
}

test.describe("Stop is fully visible at every width (brief §5, rule 13)", () => {
  for (const route of ROUTES) {
    for (const width of WIDTHS) {
      test(`${width} px, ${route.name}`, async ({ page }) => {
        await page.setViewportSize({ width, height: 800 });
        await page.goto(route.path);
        await page.waitForLoadState("networkidle");
        if (width < DESKTOP) {
          await expectStopVisible(page, width, "at the top");
          await page.getByRole("navigation", { name: "Main" }).getByRole("button", { name: "More" }).click();
          await expect(page.getByRole("dialog", { name: "More" })).toBeVisible();
          await expectStopVisible(page, width, "with More open");
          await page.getByRole("navigation", { name: "Main" }).getByRole("button", { name: "Stop", exact: true }).click();
          await expect(page.getByRole("dialog", { name: /^Stop/ }), "Stop opens its sheet over More").toBeVisible();
          await expect(page.getByRole("dialog", { name: "More" })).toBeHidden();
          return;
        }
        await expect(page.getByRole("navigation", { name: "Primary" })).toBeVisible();
        await expectStopVisible(page, width, "at the top");
        await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
        await expectStopVisible(page, width, "scrolled to the end");
      });
    }
  }
});

/**
 * While the browser captures a view transition's snapshot it sends every hit to the document element
 * (CSS View Transitions 1, rendering suppression). That lasts a frame or two; the cross-fade after it
 * must not hold Stop, so no more than that many frames per navigation may miss it.
 */
const CAPTURE_FRAMES = 2;
const HOPS = ["/messages", "/", "/messages", "/"];

test.describe("Stop answers a press while the page cross-fades (brief §5, rule 13)", () => {
  for (const width of [390, 1280]) {
    test(`${width} px, navigating between home and messages`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.goto("/");
      await page.waitForLoadState("networkidle");
      const stop = page.getByRole("button", { name: "Stop", exact: true });
      const before = await page.evaluate(() => window.__viewTransitions.started);
      const perHop: string[] = [];
      let from = new URL(page.url()).pathname;
      let hopStart = before;
      for (const href of HOPS) {
        const sampled = stop.evaluate(
          (el) =>
            new Promise<{ frames: number; missed: number; covering: string[] }>((done) => {
              const r = el.getBoundingClientRect();
              const result = { frames: 0, missed: 0, covering: [] as string[] };
              const end = performance.now() + 1200;
              const tick = () => {
                const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
                result.frames++;
                if (hit === document.documentElement) result.missed++;
                else if (!hit || (hit !== el && !el.contains(hit))) result.covering.push(hit ? hit.tagName.toLowerCase() : "nothing");
                if (performance.now() < end) requestAnimationFrame(tick);
                else done(result);
              };
              requestAnimationFrame(tick);
            }),
        );
        const nav = page.getByRole("navigation", { name: width < DESKTOP ? "Main" : "Primary" });
        await nav.locator(`a[href="${href}"]`).first().click();
        const { frames, missed, covering } = await sampled;
        await expect(page).toHaveURL(href);
        expect(covering, `${href}: elements over Stop's centre`).toEqual([]);
        expect(missed, `${href}: frames of ${frames} in which Stop took no press`).toBeLessThanOrEqual(CAPTURE_FRAMES);
        const hopEnd = await page.evaluate(() => window.__viewTransitions.started);
        const hopStarted = hopEnd - hopStart;
        perHop.push(`${from}→${href}: ${hopStarted}`);
        expect(
          hopStarted,
          `${from}→${href}: view transitions started by this hop (${hopStarted}); a hop without a cross-fade proves nothing for rule 13; per hop so far (route→route: started) ${perHop.join(", ")}`,
        ).toBeGreaterThanOrEqual(1);
        from = href;
        hopStart = hopEnd;
      }
      const ran = (await page.evaluate(() => window.__viewTransitions.started)) - before;
      expect(
        ran,
        `view transitions during the navigations, per hop (route→route: started) ${perHop.join(", ")}`,
      ).toBeGreaterThanOrEqual(HOPS.length);
    });
  }
});
