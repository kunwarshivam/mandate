import { type Locator, type Page, expect, test } from "@playwright/test";
import { AGENT_IDS } from "../src/fixtures/workspace";
import { agentHref } from "../src/lib/screens";

/**
 * The desktop sidebar stays put: however far a long page scrolls, the panel fills the viewport from
 * top to bottom and its nav items stay on screen, expanded and collapsed to icons. Below the 1024 px
 * mobile breakpoint the tab bar and the slide-out sheet carry the nav instead.
 */

const WIDTHS = [1280, 1920];
const HEIGHT = 800;
const ROUTE = agentHref(AGENT_IDS.swing, "overview");

async function scrollToBottom(page: Page) {
  const { scrollHeight, innerHeight } = await page.evaluate(() => ({
    scrollHeight: document.documentElement.scrollHeight,
    innerHeight: window.innerHeight,
  }));
  expect(scrollHeight, "the page is long enough to scroll well past the sidebar").toBeGreaterThan(innerHeight * 1.5);
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await page.waitForFunction(() => Math.ceil(window.scrollY + window.innerHeight) >= document.documentElement.scrollHeight - 1);
}

async function computedScroll(locator: Locator) {
  return locator.evaluate((el) => {
    const style = getComputedStyle(el);
    return { overflowY: style.overflowY, overscrollBehaviorY: style.overscrollBehaviorY };
  });
}

async function expectPinned(page: Page, state: string) {
  const sidebar = page.locator("[data-sidebar-wrapper] aside[data-sidebar=sidebar]");
  const panel = sidebar.locator("[data-sidebar=content-container]");
  const found = await panel.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const box = { top: r.top, bottom: r.bottom, left: r.left, right: r.right };
    const probe = (y: number) => {
      const hit = document.elementFromPoint(box.left + 4, y);
      return hit !== null && el.contains(hit);
    };
    return {
      box,
      scrollY: window.scrollY,
      background: getComputedStyle(el).backgroundColor,
      paintsTop: probe(box.top + 2),
      paintsBottom: probe(window.innerHeight - 2),
    };
  });
  const { box } = found;
  expect(found.scrollY, `${state}: the page scrolled`).toBeGreaterThan(0);
  // Scrolled to the end, the rail stops at its wrapper's bottom edge; the wrapper's height is fractional
  // and the scroll range whole pixels, so the rail can sit up to half a pixel above the viewport's top.
  expect(box.top, `${state}: the sidebar's top edge`).toBeGreaterThanOrEqual(-0.5);
  expect(box.top, `${state}: the sidebar's top edge`).toBeLessThanOrEqual(1);
  expect(box.bottom, `${state}: the sidebar's bottom edge`).toBeLessThanOrEqual(HEIGHT);
  expect(box.bottom, `${state}: the sidebar's bottom edge`).toBeGreaterThanOrEqual(HEIGHT - 1);
  expect(found.background, `${state}: the sidebar paints its own background`).not.toMatch(/^(transparent|rgba\(0, 0, 0, 0\))$/);
  expect(found.paintsTop, `${state}: the sidebar is the element at the top of its column`).toBe(true);
  expect(found.paintsBottom, `${state}: the sidebar is the element at the bottom of its column`).toBe(true);

  const items = sidebar.locator("[data-sidebar=sliding-view][aria-hidden=false] a[href]");
  expect(await items.count(), `${state}: nav items`).toBeGreaterThan(0);
  await expect(items.first(), `${state}: the first nav item`).toBeInViewport();
  await expect(items.and(page.locator("[aria-current=page]")), `${state}: the current page's nav item`).toBeInViewport();
  await expect(sidebar.locator("a[href]").first(), `${state}: the brand link`).toBeInViewport();
  await expect(sidebar.getByRole("button", { name: /(Collapse|Expand) sidebar/ }), `${state}: the collapse toggle`).toBeInViewport();

  const scroller = await computedScroll(sidebar.locator("[data-sidebar=viewport]"));
  expect(scroller, `${state}: the nav scrolls on its own`).toEqual({ overflowY: "auto", overscrollBehaviorY: "contain" });

  const header = await page.locator("header").first().boundingBox();
  expect(header?.y, `${state}: the header stays at the top`).toBe(0);
  await expect(page.getByRole("button", { name: "Stop", exact: true }), `${state}: Stop`).toBeInViewport({ ratio: 1 });
}

test.describe("The desktop sidebar stays pinned while a long page scrolls", () => {
  for (const width of WIDTHS) {
    for (const state of ["expanded", "collapsed"] as const) {
      test(`${width} px, ${state}`, async ({ page }) => {
        await page.setViewportSize({ width, height: HEIGHT });
        await page.goto(ROUTE);
        await page.waitForLoadState("networkidle");
        const wrapper = page.locator("[data-sidebar-wrapper]");
        await expect(wrapper).toHaveAttribute("data-state", "expanded");
        if (state === "collapsed") {
          await expect(async () => {
            await page.getByRole("button", { name: "Collapse sidebar" }).click();
            await expect(wrapper).toHaveAttribute("data-state", "collapsed", { timeout: 1000 });
          }).toPass({ timeout: 15_000 });
          await page.waitForFunction(() => document.getAnimations().every((a) => a.playState !== "running"));
        }
        await scrollToBottom(page);
        await expectPinned(page, state);
      });
    }
  }
});
