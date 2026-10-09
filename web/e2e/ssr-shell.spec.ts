import { type Page, type Route, expect, test } from "@playwright/test";
import { recordHref } from "../src/components/stop/commands";
import { AGENT_IDS, APPROVAL_IDS } from "../src/fixtures/workspace";
import { isRecordRoute } from "../src/lib/frozen";
import { agentHref } from "../src/lib/screens";

/**
 * The shell is laid out by CSS from the first server render (brief §5, rule 13: Stop does not depend
 * on the dashboard having loaded), and nothing in it picks a layout in JS, so there is nothing for a
 * phone to wait for. With JavaScript off, and with it on but the bundle held back, a phone gets the
 * phone shell (DEC-207): the full-width content column, the tab bar with Stop at its end wholly on
 * screen (DEC-452), the status strip only on a record screen, and nothing wider than the viewport. A
 * desktop gets the desktop shell from the same markup: the dock at the bottom, Stop at its end, and
 * the full-width column.
 */

const PHONES = [320, 360, 390, 430];
const DESKTOPS = [1024, 1440];
/** Tailwind's `lg`: below it the tab bar carries the navigation. */
const LG = 1024;
const HEIGHT = 844;

const ROUTES = [
  { name: "home", path: "/" },
  { name: "agents", path: "/agents" },
  { name: "an agent", path: agentHref(AGENT_IDS.btc, "overview") },
  { name: "approvals", path: "/approvals" },
  { name: "an approval request", path: `/approvals/${APPROVAL_IDS.swingXyz}` },
  { name: "positions", path: "/positions" },
  { name: "the kill-switch record", path: recordHref("kill", AGENT_IDS.btc) },
];

declare global {
  interface Window {
    /** How many view transitions are still running; see `countViewTransitions`. */
    __viewTransitionsRunning: number;
  }
}

/** Counts the document's running view transitions from before its first script, inline ones included. */
function countViewTransitions() {
  window.__viewTransitionsRunning = 0;
  const start = document.startViewTransition;
  if (!start) return;
  document.startViewTransition = function (this: Document, ...args: Parameters<typeof start>) {
    const transition = start.apply(this, args);
    window.__viewTransitionsRunning++;
    transition.finished.finally(() => window.__viewTransitionsRunning--);
    return transition;
  } as typeof start;
}

/**
 * The server document has settled: React's inline runtime has revealed every streamed Suspense boundary
 * (no hidden `S:` segment is left) and the view transition it reveals them in has ended. React holds a
 * reveal up to 300 ms and starts it in a view transition that waits for the fonts, and while a view
 * transition captures its snapshot every hit goes to the document element (`e2e/stop-visible.spec.ts`),
 * so sampling before then measures the transition, not the shell.
 */
async function serverDocumentSettled(page: Page) {
  await page.evaluate(async () => {
    while (document.querySelector('div[hidden][id^="S:"]') !== null || window.__viewTransitionsRunning > 0) {
      await new Promise((frame) => requestAnimationFrame(frame));
    }
  });
}

/** The route shows its server markup: every stylesheet has loaded, whether or not any script has. */
async function stylesLoaded(page: Page) {
  await page.waitForFunction(() =>
    Array.from(document.querySelectorAll<HTMLLinkElement>('link[rel="stylesheet"]')).every((link) => link.sheet !== null),
  );
}

async function measureShell(page: Page) {
  return page.evaluate(() => {
    const box = (el: Element | null) => {
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { left: r.left, top: r.top, right: r.right, bottom: r.bottom, width: r.width, height: r.height };
    };
    const stop = Array.from(document.querySelectorAll<HTMLElement>('[data-slot="stop-control"]')).find((b) => b.getClientRects().length > 0) ?? null;
    const stopBox = box(stop);
    const hit = stopBox ? document.elementFromPoint(stopBox.left + stopBox.width / 2, stopBox.top + stopBox.height / 2) : null;
    const sidebar = document.querySelector("[data-sidebar]");
    const tabBar = Array.from(document.querySelectorAll("nav")).find((n) => n.getAttribute("aria-label") === "Main") ?? null;
    const shown = (el: Element | null) => (el && el.getClientRects().length > 0 ? el : null);
    const strip = shown(document.querySelector<HTMLElement>('[data-slot="status-strip"]')) as HTMLElement | null;
    const dock = document.querySelector<HTMLElement>('nav[data-slot="dock"]');
    return {
      innerWidth: window.innerWidth,
      scrollWidth: document.documentElement.scrollWidth,
      stop: stopBox,
      stopPlace: stop?.dataset.place ?? null,
      stopUncovered: stop !== null && hit !== null && (hit === stop || stop.contains(hit)),
      covering: stop && hit && hit !== stop && !stop.contains(hit) ? `<${hit.tagName.toLowerCase()} class="${(hit.getAttribute("class") ?? "").slice(0, 80)}">` : null,
      stopClipped: stop !== null && stop.scrollWidth > stop.clientWidth,
      sidebar: sidebar !== null,
      content: box(document.getElementById("main")?.parentElement ?? null),
      tabBar: tabBar && getComputedStyle(tabBar.parentElement!).display !== "none" ? box(tabBar) : null,
      dock: dock && getComputedStyle(dock.parentElement!).display !== "none" ? { box: box(dock), position: getComputedStyle(dock.parentElement!).position } : null,
      main: box(document.getElementById("main")),
      strip: strip ? { box: box(strip), scrollWidth: strip.scrollWidth, clientWidth: strip.clientWidth, overflowX: getComputedStyle(strip).overflowX } : null,
    };
  });
}

type Shell = Awaited<ReturnType<typeof measureShell>>;

function expectStopOnScreen(found: Shell, width: number, state: string, place: "dock" | "tab") {
  expect(found.innerWidth, `${state}: the layout viewport is the device's width`).toBe(width);
  expect(found.scrollWidth, `${state}: the page does not scroll sideways`).toBeLessThanOrEqual(found.innerWidth);
  expect(found.stopPlace, `${state}: the one Stop on screen ends the ${place === "dock" ? "dock" : "tab bar"}`).toBe(place);
  const stop = found.stop!;
  expect(stop.left, `${state}: Stop's left edge`).toBeGreaterThanOrEqual(0);
  expect(stop.top, `${state}: Stop's top edge`).toBeGreaterThanOrEqual(0);
  expect(stop.right, `${state}: Stop's right edge against a ${width} px viewport`).toBeLessThanOrEqual(width);
  expect(stop.bottom, `${state}: Stop's bottom edge`).toBeLessThanOrEqual(HEIGHT);
  expect(found.covering, `${state}: the element at Stop's centre`).toBeNull();
  expect(found.stopUncovered, `${state}: Stop is the element at its own centre`).toBe(true);
  expect(found.stopClipped, `${state}: Stop's label fits`).toBe(false);
}

function expectPhoneShell(found: Shell, width: number, state: string, record: boolean) {
  expectStopOnScreen(found, width, state, "tab");
  expect(found.sidebar, `${state}: no sidebar`).toBe(false);
  expect(found.content?.left, `${state}: the content column starts at the left edge`).toBe(0);
  expect(found.content?.width, `${state}: the content column takes the full width`).toBe(width);
  expect(found.dock, `${state}: no dock`).toBeNull();
  expect(found.tabBar, `${state}: the tab bar shows`).not.toBeNull();
  expect(found.tabBar!.bottom, `${state}: the tab bar sits at the bottom`).toBeLessThanOrEqual(HEIGHT);
  expect(found.tabBar!.width, `${state}: the tab bar spans the viewport`).toBe(width);
  if (!record) {
    expect(found.strip, `${state}: no status strip while every feed answers`).toBeNull();
    return;
  }
  const strip = found.strip!;
  expect(strip, `${state}: a record screen keeps the status strip`).not.toBeNull();
  expect(strip.box!.right, `${state}: the status strip ends inside the viewport`).toBeLessThanOrEqual(width);
  expect(strip.overflowX, `${state}: the status strip scrolls inside itself`).toBe("auto");
}

function expectDesktopShell(found: Shell, width: number, state: string) {
  expectStopOnScreen(found, width, state, "dock");
  expect(found.sidebar, `${state}: no sidebar`).toBe(false);
  expect(found.content!.left, `${state}: the content column starts at the left edge`).toBe(0);
  expect(found.content!.width, `${state}: the content column takes the full width`).toBe(width);
  expect(Math.abs((found.main!.left + found.main!.right) / 2 - width / 2), `${state}: the page is centred in it`).toBeLessThanOrEqual(1);
  expect(found.dock, `${state}: the dock shows`).not.toBeNull();
  expect(found.dock!.position, `${state}: the dock is fixed`).toBe("fixed");
  expect(found.dock!.box!.bottom, `${state}: the dock floats above the bottom edge`).toBeLessThan(HEIGHT);
  expect(Math.abs((found.dock!.box!.left + found.dock!.box!.right) / 2 - width / 2), `${state}: the dock is centred`).toBeLessThanOrEqual(1);
  expect(found.tabBar, `${state}: no tab bar`).toBeNull();
}

test.describe("With JavaScript off, phones get the phone shell (brief §5, rule 13)", () => {
  for (const width of PHONES) {
    test.describe(`${width} px`, () => {
      test.use({ javaScriptEnabled: false, isMobile: true, hasTouch: true, viewport: { width, height: HEIGHT } });
      for (const route of ROUTES) {
        test(route.name, async ({ page }) => {
          await page.goto(route.path);
          await stylesLoaded(page);
          expectPhoneShell(await measureShell(page), width, "server markup", isRecordRoute(route.path));
        });
      }
    });
  }
});

test.describe("With JavaScript off, desktops get the dock", () => {
  for (const width of DESKTOPS) {
    test.describe(`${width} px`, () => {
      test.use({ javaScriptEnabled: false, viewport: { width, height: HEIGHT } });
      for (const route of ROUTES) {
        test(route.name, async ({ page }) => {
          await page.goto(route.path);
          await stylesLoaded(page);
          expectDesktopShell(await measureShell(page), width, "server markup");
        });
      }
    });
  }
});

/**
 * A slow link: the page and its styles arrive, the client bundle waits. The shell measured then is
 * what a phone paints until hydration, and it must already be the phone shell, the same one it
 * shows once React has taken over, with Stop in the same place and no hydration mismatch.
 */
test.describe("With JavaScript on, phones show no desktop flash before hydration", () => {
  for (const width of PHONES) {
    test.describe(`${width} px`, () => {
      test.use({ isMobile: true, hasTouch: true, viewport: { width, height: HEIGHT } });
      for (const route of ROUTES) {
        test(route.name, async ({ page }) => {
          expect(width).toBeLessThan(LG);
          const errors: string[] = [];
          page.on("pageerror", (e) => errors.push(e.message));
          page.on("console", (m) => {
            if (m.type() === "error") errors.push(m.text());
          });
          await page.addInitScript(countViewTransitions);
          const held: Route[] = [];
          let release = false;
          await page.route(/\/_next\/static\/.*\.js(\?.*)?$/, (r) => (release ? r.continue() : void held.push(r)));

          // The bundle's scripts are async, so the whole server document parses while they are held.
          await page.goto(route.path, { waitUntil: "domcontentloaded" });
          await stylesLoaded(page);
          await page.evaluate(() => new Promise((frame) => requestAnimationFrame(() => requestAnimationFrame(frame))));
          await serverDocumentSettled(page);
          await expect.poll(() => held.length, "the client bundle is held back").toBeGreaterThan(0);
          const record = isRecordRoute(route.path);
          const before = await measureShell(page);
          expectPhoneShell(before, width, "before hydration", record);

          release = true;
          for (const r of held.splice(0)) await r.continue();
          const more = page.getByRole("navigation", { name: "Main" }).getByRole("button", { name: "More" });
          const sheet = page.getByRole("dialog", { name: "More" });
          await expect(async () => {
            await more.click();
            await expect(sheet, "React hydrated: More opens its sheet").toBeVisible({ timeout: 1000 });
          }).toPass();
          await page.keyboard.press("Escape");
          await expect(sheet).toBeHidden();
          await page.waitForLoadState("networkidle");
          const after = await measureShell(page);
          expectPhoneShell(after, width, "after hydration", record);
          expect(after.stop, "Stop does not move when React takes over").toEqual(before.stop);
          expect(errors.filter((e) => /hydrat|#418|#423|#425/i.test(e)), "hydration errors").toEqual([]);
        });
      }
    });
  }
});
