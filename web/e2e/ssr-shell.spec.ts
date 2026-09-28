import { type Page, type Route, expect, test } from "@playwright/test";
import { recordHref } from "../src/components/stop/commands";
import { AGENT_IDS, APPROVAL_IDS } from "../src/fixtures/workspace";
import { agentHref } from "../src/lib/screens";

/**
 * The shell is laid out by CSS from the first server render (brief §5, rule 13: Stop does not depend
 * on the dashboard having loaded). Kumo's Sidebar picks its desktop rail or its mobile sheet in JS,
 * and its server snapshot is "desktop", so until the client bundle runs a phone would get the rail in
 * flow, a squeezed content column, and Stop pushed off-screen. With JavaScript off, and with it on
 * but the bundle held back, a phone gets the phone shell: no rail, the full-width content column, the
 * tab bar, Stop in the header wholly on screen, and nothing wider than the viewport.
 */

const PHONES = [320, 360, 390, 430];
const DESKTOPS = [1024, 1440];
/** Tailwind's `lg`, and the breakpoint `AppShell` hands Kumo's Sidebar. */
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
    const header = document.querySelector("header");
    const stop = Array.from(header?.querySelectorAll("button") ?? []).find((b) => b.textContent?.trim() === "Stop") ?? null;
    const stopBox = box(stop);
    const hit = stopBox ? document.elementFromPoint(stopBox.left + stopBox.width / 2, stopBox.top + stopBox.height / 2) : null;
    const rail = document.querySelector<HTMLElement>('aside[data-sidebar="sidebar"]:not([data-mobile])');
    const railStyle = rail ? getComputedStyle(rail) : null;
    const sheet = document.querySelector('[data-sidebar="sidebar"][data-mobile]');
    const tabBar = Array.from(document.querySelectorAll("nav")).find((n) => n.getAttribute("aria-label") === "Main" && !n.hasAttribute("data-mobile")) ?? null;
    const strip = document.querySelector<HTMLElement>('[data-slot="status-strip"]');
    return {
      innerWidth: window.innerWidth,
      scrollWidth: document.documentElement.scrollWidth,
      stop: stopBox,
      stopInHeader: stop !== null,
      stopUncovered: stop !== null && hit !== null && (hit === stop || stop.contains(hit)),
      covering: stop && hit && hit !== stop && !stop.contains(hit) ? `<${hit.tagName.toLowerCase()} class="${(hit.getAttribute("class") ?? "").slice(0, 80)}">` : null,
      stopClipped: stop !== null && stop.scrollWidth > stop.clientWidth,
      rail: rail && railStyle ? { display: railStyle.display, position: railStyle.position, box: box(rail) } : null,
      sheet: box(sheet),
      content: box(document.getElementById("main")?.parentElement ?? null),
      tabBar: tabBar && getComputedStyle(tabBar.parentElement!).display !== "none" ? box(tabBar) : null,
      strip: strip ? { box: box(strip), scrollWidth: strip.scrollWidth, clientWidth: strip.clientWidth, overflowX: getComputedStyle(strip).overflowX } : null,
    };
  });
}

type Shell = Awaited<ReturnType<typeof measureShell>>;

function expectStopOnScreen(found: Shell, width: number, state: string) {
  expect(found.innerWidth, `${state}: the layout viewport is the device's width`).toBe(width);
  expect(found.scrollWidth, `${state}: the page does not scroll sideways`).toBeLessThanOrEqual(found.innerWidth);
  expect(found.stopInHeader, `${state}: Stop is in the header`).toBe(true);
  const stop = found.stop!;
  expect(stop.left, `${state}: Stop's left edge`).toBeGreaterThanOrEqual(0);
  expect(stop.top, `${state}: Stop's top edge`).toBeGreaterThanOrEqual(0);
  expect(stop.right, `${state}: Stop's right edge against a ${width} px viewport`).toBeLessThanOrEqual(width);
  expect(stop.bottom, `${state}: Stop's bottom edge`).toBeLessThanOrEqual(HEIGHT);
  expect(found.covering, `${state}: the element at Stop's centre`).toBeNull();
  expect(found.stopUncovered, `${state}: Stop is the element at its own centre`).toBe(true);
  expect(found.stopClipped, `${state}: Stop's label fits`).toBe(false);
}

function expectPhoneShell(found: Shell, width: number, state: string) {
  expectStopOnScreen(found, width, state);
  if (found.rail) {
    expect(found.rail.display, `${state}: the desktop rail takes no room`).toBe("none");
    expect(found.rail.box?.width, `${state}: the desktop rail's width`).toBe(0);
  }
  if (found.sheet) expect(found.sheet.right, `${state}: the closed sheet sits off-canvas`).toBeLessThanOrEqual(0);
  expect(found.content?.left, `${state}: the content column starts at the left edge`).toBe(0);
  expect(found.content?.width, `${state}: the content column takes the full width`).toBe(width);
  expect(found.tabBar, `${state}: the tab bar shows`).not.toBeNull();
  expect(found.tabBar!.bottom, `${state}: the tab bar sits at the bottom`).toBeLessThanOrEqual(HEIGHT);
  expect(found.tabBar!.width, `${state}: the tab bar spans the viewport`).toBe(width);
  const strip = found.strip!;
  expect(strip.box!.right, `${state}: the status strip ends inside the viewport`).toBeLessThanOrEqual(width);
  expect(strip.overflowX, `${state}: the status strip scrolls inside itself`).toBe("auto");
}

function expectDesktopShell(found: Shell, width: number, state: string) {
  expectStopOnScreen(found, width, state);
  expect(found.rail, `${state}: the desktop rail renders`).not.toBeNull();
  expect(found.rail!.display, `${state}: the desktop rail shows`).not.toBe("none");
  expect(found.rail!.position, `${state}: the desktop rail is pinned`).toBe("sticky");
  expect(found.rail!.box!.left, `${state}: the desktop rail's left edge`).toBe(0);
  expect(found.rail!.box!.width, `${state}: the desktop rail's width`).toBeGreaterThan(0);
  expect(found.rail!.box!.height, `${state}: the desktop rail fills the viewport's height`).toBe(HEIGHT);
  expect(found.content!.left, `${state}: the content column starts after the rail`).toBe(found.rail!.box!.right);
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
          const found = await measureShell(page);
          expect(found.sheet, "no script has run, so Kumo's sheet has not mounted").toBeNull();
          expectPhoneShell(found, width, "server markup");
        });
      }
    });
  }
});

test.describe("With JavaScript off, desktops get the pinned rail", () => {
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
          const held: Route[] = [];
          let release = false;
          await page.route(/\/_next\/static\/.*\.js(\?.*)?$/, (r) => (release ? r.continue() : void held.push(r)));

          // The bundle's scripts are async, so the whole server document parses while they are held.
          await page.goto(route.path, { waitUntil: "domcontentloaded" });
          await stylesLoaded(page);
          await page.evaluate(() => new Promise((frame) => requestAnimationFrame(() => requestAnimationFrame(frame))));
          await expect.poll(() => held.length, "the client bundle is held back").toBeGreaterThan(0);
          const before = await measureShell(page);
          expect(before.sheet, "before hydration Kumo's sheet has not mounted").toBeNull();
          expectPhoneShell(before, width, "before hydration");

          release = true;
          for (const r of held.splice(0)) await r.continue();
          await expect(page.locator('[data-sidebar="sidebar"][data-mobile]'), "React hydrated and Kumo mounted its sheet").toBeAttached();
          await page.waitForLoadState("networkidle");
          const after = await measureShell(page);
          expect(after.rail, "after hydration Kumo renders no desktop rail").toBeNull();
          expect(after.sheet, "after hydration the sheet is mounted").not.toBeNull();
          expectPhoneShell(after, width, "after hydration");
          expect(after.stop, "Stop does not move when React takes over").toEqual(before.stop);
          expect(errors.filter((e) => /hydrat|#418|#423|#425/i.test(e)), "hydration errors").toEqual([]);
        });
      }
    });
  }
});
