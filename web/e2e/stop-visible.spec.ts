import { type Page, expect, test } from "@playwright/test";
import { recordHref } from "../src/components/stop/commands";
import { AGENT_IDS, APPROVAL_IDS } from "../src/fixtures/workspace";
import { agentHref } from "../src/lib/screens";

/**
 * Stop is always one press away (brief §5, rule 13): at every width, on every main route, with the
 * sidebar open or collapsed, the Stop button sits wholly inside the viewport, nothing covers it,
 * and its label is never clipped. On touch widths it is at least 44 by 44 px.
 */

const WIDTHS = [320, 360, 375, 390, 414, 430, 480, 600, 640, 768, 820, 1024, 1180, 1280, 1440, 1920];
/** `AppShell` hands Kumo's Sidebar a 1024 px mobile breakpoint: from here up the sidebar collapses in place. */
const DESKTOP_SIDEBAR = 1024;
/** Phones and tablets, landscape included. */
const TOUCH_MAX = 1180;
const MIN_TARGET = 44;

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
  const found = await stop.evaluate((el) => {
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
        if (width < DESKTOP_SIDEBAR) {
          await expectStopVisible(page, width, "sidebar closed");
          return;
        }
        const wrapper = page.locator("[data-sidebar-wrapper]");
        await expect(wrapper).toHaveAttribute("data-state", "expanded");
        await expectStopVisible(page, width, "sidebar open");
        await expect(async () => {
          await page.getByRole("button", { name: "Collapse sidebar" }).click();
          await expect(wrapper).toHaveAttribute("data-state", "collapsed", { timeout: 1000 });
        }).toPass({ timeout: 15_000 });
        await expectStopVisible(page, width, "sidebar collapsed");
      });
    }
  }
});
