import { type Page, expect, test } from "@playwright/test";
import { APPROVAL_IDS } from "../src/fixtures/workspace";
import { SCREENS, SECTION_INDEX } from "../src/lib/screens";

/**
 * From 1024 px the dock carries the navigation: a floating glass bar centred at the bottom, with a
 * label on hover and on focus for every item, menus a keyboard can open, move through and close,
 * and every screen the sidebar reached one press or one menu away. It never covers a focused
 * control, an approval's choices, the header or Stop. Below 1024 px the tab bar carries it instead.
 */

const dock = (page: Page) => page.getByRole("navigation", { name: "Primary" });
const tooltip = (page: Page) => page.locator(".kumo-tooltip-popup");

async function open(page: Page, path: string, width = 1440, height = 900) {
  await page.setViewportSize({ width, height });
  await page.goto(path);
  await page.waitForLoadState("networkidle");
}

test("its shape: a rounded glass bar of 46 px items with 22 px icons", async ({ page }) => {
  await open(page, "/");
  const found = await dock(page).evaluate((el) => {
    const style = getComputedStyle(el);
    const item = el.querySelector("a")!;
    const icon = item.querySelector("svg")!.getBoundingClientRect();
    return {
      radius: parseFloat(style.borderTopLeftRadius),
      padding: parseFloat(style.paddingTop),
      shadow: style.boxShadow,
      height: el.getBoundingClientRect().height,
      item: [item.getBoundingClientRect().width, item.getBoundingClientRect().height],
      icon: [icon.width, icon.height],
    };
  });
  expect(found.radius).toBeGreaterThanOrEqual(20);
  expect(found.radius).toBeLessThanOrEqual(24);
  expect(found.padding).toBe(6);
  expect(found.item).toEqual([46, 46]);
  expect(found.icon).toEqual([22, 22]);
  expect(found.height, "the dock's height matches --dock-h").toBe(60);
  expect(found.shadow).not.toBe("none");
});

test("the current screen's item is a raised card tile", async ({ page }) => {
  await open(page, "/positions");
  const current = dock(page).getByRole("link", { name: "Positions" });
  await expect(current).toHaveAttribute("aria-current", "page");
  const [tile, idle, card] = await Promise.all([
    current.evaluate((el) => getComputedStyle(el).backgroundColor),
    dock(page).getByRole("link", { name: "Alerts" }).evaluate((el) => getComputedStyle(el).backgroundColor),
    page.evaluate(() => {
      const probe = document.createElement("div");
      probe.style.backgroundColor = "var(--card)";
      document.body.append(probe);
      const value = getComputedStyle(probe).backgroundColor;
      probe.remove();
      return value;
    }),
  ]);
  expect(tile).toBe(card);
  expect(idle).toMatch(/^rgba\(0, 0, 0, 0\)$|^transparent$/);
});

test("every item has a label on hover and on keyboard focus, and a visible focus ring", async ({ page }) => {
  await open(page, "/");
  const items = dock(page).locator("a, button");
  const names = await items.evaluateAll((els) => els.map((el) => el.getAttribute("aria-label") ?? el.textContent?.replace(/\d+ open$/, "")));
  expect(names).toEqual(["Home", "Approvals", "Alerts", "All agents", "Positions", "Connections", "Audit", "More screens"]);
  await items.nth(4).hover();
  await expect(tooltip(page)).toHaveText("Positions");
  await page.mouse.move(0, 450);
  await expect(tooltip(page)).toHaveCount(0);

  await page.locator("#main").focus();
  const first = items.first();
  await first.focus();
  await page.keyboard.press("Tab");
  const second = items.nth(1);
  await expect(second).toBeFocused();
  await expect(tooltip(page)).toHaveText("Approvals");
  expect(await second.evaluate((el) => getComputedStyle(el).boxShadow), "the focus ring").not.toBe("none");
});

test("the menus open, move and close from the keyboard, and focus comes back to the button", async ({ page }) => {
  await open(page, "/");
  const more = dock(page).getByRole("button", { name: "More screens" });
  await more.focus();
  await page.keyboard.press("Enter");
  const menu = page.getByRole("menu");
  await expect(menu).toBeVisible();
  await expect(more).toHaveAttribute("aria-expanded", "true");
  await expect(menu).toContainText("Account");
  await expect(menu).toContainText("Alpaca paper");
  await page.keyboard.press("ArrowDown");
  await expect(menu.locator(":focus")).toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await expect(more).toBeFocused();

  const audit = dock(page).getByRole("button", { name: "Audit" });
  await audit.focus();
  await page.keyboard.press("ArrowUp");
  await expect(page.getByRole("menu")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(audit).toBeFocused();
});

test("every screen the sidebar reached is one press or one menu away", async ({ page }) => {
  await open(page, "/");
  const reached = new Set(await dock(page).locator(":scope > a").evaluateAll((els) => els.map((el) => el.getAttribute("href"))));
  for (const menu of ["Audit", "More screens"]) {
    await dock(page).getByRole("button", { name: menu }).click();
    const list = page.getByRole("menu");
    await expect(list).toBeVisible();
    for (const href of await list.getByRole("menuitem").evaluateAll((els) => els.map((el) => el.getAttribute("href")))) if (href) reached.add(href);
    await page.keyboard.press("Escape");
    await expect(list).toBeHidden();
  }
  for (const href of [...SCREENS.map((s) => s.href), SECTION_INDEX.audit.href, SECTION_INDEX.workspace.href]) expect(reached, href).toContain(href);
});

test("a menu item navigates, and the menu's button shows where you are", async ({ page }) => {
  await open(page, "/");
  await dock(page).getByRole("button", { name: "Audit" }).click();
  await page.getByRole("menuitem", { name: "Trace" }).click();
  await expect(page).toHaveURL("/audit/trace");
  const audit = dock(page).getByRole("button", { name: "Audit" });
  await expect(audit).toHaveAttribute("aria-current", "true");
  await expect(async () => {
    if ((await audit.getAttribute("aria-expanded")) !== "true") await audit.click();
    await expect(page.getByRole("menu")).toBeVisible({ timeout: 1000 });
  }).toPass({ timeout: 15_000 });
  await expect(page.getByRole("menuitem", { name: "Trace" })).toHaveAttribute("aria-current", "page");
});

for (const [width, height] of [
  [1024, 768],
  [1440, 900],
] as const) {
  test(`${width} x ${height}: tabbing through a long page, no focused control sits under the dock`, async ({ page }) => {
    await open(page, "/agents", width, height);
    const bar = (await dock(page).boundingBox())!;
    await page.locator("#main").focus();
    const covered: string[] = [];
    for (let i = 0; i < 60; i++) {
      await page.keyboard.press("Tab");
      const hit = await page.evaluate(() => {
        const el = document.activeElement as HTMLElement | null;
        if (!el || !document.getElementById("main")?.contains(el)) return null;
        const r = el.getBoundingClientRect();
        return { bottom: r.bottom, left: r.left, right: r.right, label: `${el.tagName} ${el.textContent?.trim().slice(0, 40)}` };
      });
      if (hit && hit.bottom > bar.y && hit.right > bar.x && hit.left < bar.x + bar.width) covered.push(hit.label);
    }
    expect(covered).toEqual([]);
  });
}

test("an approval's choices stay above the dock", async ({ page }) => {
  await open(page, `/approvals/${APPROVAL_IDS.swingXyz}`, 1280, 800);
  const choices = page.locator("[data-slot=approval-choices]");
  for (const y of [0, 10_000]) {
    await page.evaluate((y) => window.scrollTo(0, y), y);
    const bar = (await dock(page).boundingBox())!;
    const box = (await choices.boundingBox())!;
    expect(box.y + box.height, `scrolled to ${y}`).toBeLessThanOrEqual(bar.y);
    await expect(choices.getByRole("button", { name: /Approve/ })).toBeInViewport({ ratio: 1 });
  }
});

test("below 1024 px the tab bar carries the navigation, not the dock", async ({ page }) => {
  await open(page, "/", 1023, 800);
  await expect(dock(page)).toBeHidden();
  await expect(page.locator("nav[aria-label=Main].grid")).toBeVisible();
});
