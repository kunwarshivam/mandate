import { type Page, expect, test } from "@playwright/test";
import { APPROVAL_IDS } from "../src/fixtures/workspace";
import { SCREENS } from "../src/lib/screens";

/**
 * From 1024 px the dock carries the navigation: a floating glass bar centred at the bottom, every
 * item an icon over its name, the current section on a pill, and every screen one press away: Home,
 * New agent and Settings (DEC-2293). It never covers a
 * focused control, an approval's choices or the header, and Stop ends it (DEC-452). Below 1024 px the
 * tab bar carries it instead.
 */

const dock = (page: Page) => page.getByRole("navigation", { name: "Primary" });

async function open(page: Page, path: string, width = 1440, height = 900) {
  await page.setViewportSize({ width, height });
  await page.goto(path);
  await page.waitForLoadState("networkidle");
}

async function tokenColor(page: Page, token: string): Promise<string> {
  return page.evaluate((token) => {
    const probe = document.createElement("div");
    probe.style.backgroundColor = `var(${token})`;
    document.body.append(probe);
    const value = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return value;
  }, token);
}

test("its shape: a 64 px rounded glass bar of labelled items at least 64 px wide, with 24 px pixel icons over their labels", async ({ page }, testInfo) => {
  await open(page, "/");
  const found = await dock(page).evaluate((el) => {
    const style = getComputedStyle(el);
    const probe = document.createElement("div");
    probe.style.borderRadius = "var(--radius-3xl)";
    document.body.append(probe);
    const radius3xl = parseFloat(getComputedStyle(probe).borderTopLeftRadius);
    probe.remove();
    return {
      radius: parseFloat(style.borderTopLeftRadius),
      radius3xl,
      shadow: style.boxShadow,
      height: el.getBoundingClientRect().height,
      stop: el.querySelector<HTMLElement>("[data-slot=stop-control]")!.getBoundingClientRect().height,
      items: [...el.querySelectorAll<HTMLElement>("a, button:not([data-slot=stop-control])")].map((item) => {
        const icon = item.querySelector("svg")!.getBoundingClientRect();
        return { width: item.getBoundingClientRect().width, height: item.getBoundingClientRect().height, icon: [icon.width, icon.height] };
      }),
    };
  });
  expect(found.radius3xl).toBeGreaterThan(0);
  expect(found.radius, "the dock takes the 3xl corner DESIGN.md gives it").toBe(found.radius3xl);
  expect(found.height, "the dock's height matches --dock-h").toBe(64);
  expect(found.items, "Home, New agent and Settings, Stop aside (DEC-2293)").toHaveLength(3);
  expect(found.stop, "Stop, at the dock's end, is as tall as its items").toBe(50);
  for (const item of found.items) {
    expect(item.width).toBeGreaterThanOrEqual(64);
    expect(item.height).toBe(50);
    expect(item.icon, "pixel icons stay on their 24px grid (DEC-478)").toEqual([24, 24]);
  }
  if (testInfo.project.name.includes("dark")) expect(found.shadow, "flat in dark").toMatch(/^(none|rgba\(0, 0, 0, 0\) 0px 0px 0px 0px(, rgba\(0, 0, 0, 0\) 0px 0px 0px 0px)*)$/);
});

test("the current section sits on a tinted pill with a semibold label; hover is a lighter pill", async ({ page }) => {
  await open(page, "/settings");
  const current = dock(page).getByRole("link", { name: "Settings" });
  const idle = dock(page).getByRole("link", { name: "New" });
  await expect(current).toHaveAttribute("aria-current", "page");
  const paint = (el: HTMLElement) => ({ bg: getComputedStyle(el).backgroundColor, weight: getComputedStyle(el.querySelector("[data-slot=dock-label]")!).fontWeight });
  const [pill, rest, tint, hoverTint, ink, mandate] = await Promise.all([
    current.evaluate(paint),
    idle.evaluate(paint),
    tokenColor(page, "--dock-current"),
    tokenColor(page, "--dock-hover"),
    tokenColor(page, "--foreground"),
    tokenColor(page, "--mandate"),
  ]);
  expect(pill.bg).toBe(tint);
  expect(pill.bg, "not a solid ink fill").not.toBe(ink);
  expect(pill.bg, "not the mandate's colour").not.toBe(mandate);
  expect(pill.weight).toBe("600");
  expect(rest.bg).toMatch(/^rgba\(0, 0, 0, 0\)$|^transparent$/);
  expect(Number(rest.weight), "an idle label is lighter than the current one").toBeLessThan(Number(pill.weight));
  await idle.hover();
  await expect.poll(() => idle.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe(hoverTint);
  expect(hoverTint).not.toBe(tint);
});

test("the labels name every item: no tooltip on hover or focus, and a visible focus ring", async ({ page }) => {
  await open(page, "/");
  const items = dock(page).locator("a, button:not([data-slot=stop-control])");
  const names = await items.evaluateAll((els) => els.map((el) => el.querySelector("[data-slot=dock-label]")?.textContent));
  expect(names).toEqual(["Home", "New", "Settings"]);
  await items.nth(1).hover();
  await page.waitForTimeout(600);
  await expect(page.locator(".kumo-tooltip-popup")).toHaveCount(0);

  await page.locator("#main").focus();
  await items.first().focus();
  await page.keyboard.press("Tab");
  const second = items.nth(1);
  await expect(second).toBeFocused();
  await page.waitForTimeout(600);
  await expect(page.locator(".kumo-tooltip-popup")).toHaveCount(0);
  expect(await second.evaluate((el) => getComputedStyle(el).boxShadow), "the focus ring").not.toBe("none");
});

test("every screen is one press away", async ({ page }) => {
  await open(page, "/");
  const reached = await dock(page).locator(":scope > a").evaluateAll((els) => els.map((el) => el.getAttribute("href")));
  expect(reached).toEqual(SCREENS.filter((s) => s.built).map((s) => s.href));
});

for (const [width, height] of [
  [1024, 768],
  [1440, 900],
] as const) {
  test(`${width} x ${height}: tabbing through a long page, no focused control sits under the dock`, async ({ page }) => {
    await open(page, "/settings", width, height);
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
