import { type Page, expect, test } from "@playwright/test";
import { AGENT_IDS } from "../src/fixtures/workspace";
import { agentHref } from "../src/lib/screens";

/**
 * From 1024 px the header carries a wide command bar between its two sides: a search icon, the
 * prompt and a ⌘K key on a muted fill with a hairline. It opens the command palette, like ⌘K. The
 * header keeps its height, and the bar never overlaps the paper badge, the theme menu, the approvals
 * and alerts links, the account menus, the brand or the trail. Below 1024 px it is the compact icon
 * beside the theme menu.
 */

const bar = (page: Page) => page.locator("[data-slot=command-bar]");

const ROUTES = ["/", agentHref(AGENT_IDS.swing, "positions"), "/audit/trace"];

async function boxes(page: Page) {
  return page.evaluate(() => {
    const header = document.querySelector("header")!;
    const rect = (el: Element) => {
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height };
    };
    const shown = (el: Element) => el.checkVisibility() && el.getBoundingClientRect().width > 0;
    const others = [
      ...header.querySelectorAll("button, a, [data-slot=environment-badge], nav[aria-label=breadcrumb]"),
    ].filter((el) => el.getAttribute("data-slot") !== "command-bar" && !el.closest("[data-slot=command-bar]") && shown(el));
    return {
      header: rect(header),
      bar: rect(document.querySelector("[data-slot=command-bar]")!),
      others: others.map((el) => ({ name: el.getAttribute("aria-label") ?? el.textContent?.trim().slice(0, 24) ?? el.tagName, ...rect(el) })),
    };
  });
}

for (const width of [1024, 1280, 1440]) {
  for (const route of ROUTES) {
    test(`${width} px, ${route}: a wide bar that overlaps nothing in a header of fixed height`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(route);
      await page.waitForLoadState("networkidle");
      await expect(bar(page)).toBeVisible();
      const found = await boxes(page);
      expect(found.header.height, "the header keeps its height").toBe(65);
      expect(found.bar.height).toBe(40);
      expect(found.bar.width).toBeLessThanOrEqual(460);
      expect(found.bar.width, "wide enough to read the prompt").toBeGreaterThanOrEqual(width >= 1280 ? 380 : 300);
      for (const other of found.others) {
        const apart = other.right <= found.bar.left || other.left >= found.bar.right;
        expect(apart, `${other.name} [${other.left}, ${other.right}] against the bar [${found.bar.left}, ${found.bar.right}]`).toBe(true);
      }
      await expect(page.getByRole("banner").getByRole("button", { name: "Theme", exact: true })).toBeInViewport({ ratio: 1 });
      await expect(page.getByRole("banner").getByRole("link", { name: "Approvals", exact: true }), "the dock carries Approvals and its count").toHaveCount(0);
      const badge = page.getByRole("banner").locator("[data-slot=environment-badge]");
      await expect(badge).toBeInViewport({ ratio: 1 });
      await expect(badge.getByText("PAPER", { exact: true })).toBeVisible();
      if (width >= 1440) expect(Math.abs((found.bar.left + found.bar.right) / 2 - width / 2), "centred in the header").toBeLessThanOrEqual(1);
    });
  }
}

test("its look: a muted fill, a hairline, no image, the prompt and the ⌘K key", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/");
  await expect(bar(page)).toHaveAccessibleName("Jump to an agent or screen… ⌘K");
  const style = await bar(page).evaluate((el) => {
    const s = getComputedStyle(el);
    const probe = document.createElement("div");
    probe.style.backgroundColor = "var(--muted)";
    document.body.append(probe);
    const muted = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return { background: s.backgroundColor, muted, border: s.borderTopWidth, image: s.backgroundImage };
  });
  expect(style.background).toBe(style.muted);
  expect(style).toMatchObject({ border: "1px", image: "none" });
  await expect(bar(page).locator("kbd")).toHaveText("⌘K");
  await expect(bar(page).locator("svg")).toBeVisible();
});

test("a press opens the command palette, and ⌘K does the same", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/");
  await page.waitForLoadState("networkidle");
  const palette = page.getByRole("dialog");
  await expect(async () => {
    await bar(page).click();
    await expect(palette).toBeVisible({ timeout: 1000 });
  }).toPass({ timeout: 15_000 });
  await expect(palette.getByRole("combobox", { name: "Command" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(palette).toBeHidden();
  await page.keyboard.press("ControlOrMeta+k");
  await expect(palette).toBeVisible();
});

for (const width of [390, 768, 1023]) {
  test(`${width} px: the bar gives way to Search in the More sheet, and the header keeps its height`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/");
    await page.waitForLoadState("networkidle");
    await expect(bar(page)).toBeHidden();
    await expect(page.getByRole("banner").getByRole("button", { name: "Go to…", exact: true })).toHaveCount(0);
    expect((await page.locator("header").boundingBox())!.height).toBe(65);
    await page.getByRole("navigation", { name: "Main" }).getByRole("button", { name: "More" }).click();
    await page.getByRole("dialog", { name: "More" }).getByRole("button", { name: /^Search/ }).click();
    const palette = page.getByRole("dialog").filter({ has: page.getByRole("combobox", { name: "Command" }) });
    await expect(palette).toBeVisible();
    await expect(palette.getByRole("combobox", { name: "Command" })).toBeFocused();
  });
}

const TRAILS: Array<[string, string, string[]]> = [
  ["/", "Home", []],
  [`/agents/${AGENT_IDS.btc}`, "Agent 1", ["Home", "Agents"]],
  [agentHref(AGENT_IDS.swing, "positions"), "Positions", ["Agents", "Agent 2"]],
  ["/audit/decisions", "Gate decisions", ["Home", "Audit"]],
];

for (const width of [1280, 1440]) {
  for (const [route, current, earlier] of TRAILS) {
    test(`${width} px, ${route}: the current page's crumb reads in full, and earlier crumbs fold into a menu`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(route);
      await page.waitForLoadState("networkidle");
      const banner = page.getByRole("banner");
      const here = banner.locator("nav[aria-label=breadcrumb] [aria-current=page]").locator("visible=true");
      await expect(here).toHaveText(current);
      const clipped = await here.evaluate((el) => {
        const text = el.querySelector("span:last-child") as HTMLElement;
        const nav = el.closest("nav")!.getBoundingClientRect();
        const box = el.getBoundingClientRect();
        return { truncated: text.scrollWidth > text.clientWidth, outside: box.left < nav.left - 0.5 || box.right > nav.right + 0.5 };
      });
      expect(clipped).toEqual({ truncated: false, outside: false });
      const found = await boxes(page);
      const box = (await here.boundingBox())!;
      expect(box.x + box.width <= found.bar.left || box.x >= found.bar.right, "clear of the command bar").toBe(true);
      const fold = banner.getByRole("button", { name: "Earlier pages" });
      if (earlier.length === 0) {
        await expect(fold).toBeHidden();
        return;
      }
      const shownLinks = banner.locator("nav[aria-label=breadcrumb] a").locator("visible=true");
      if (await fold.isVisible()) {
        await expect(shownLinks).toHaveCount(0);
        await fold.click();
        await expect(page.getByRole("menuitem")).toHaveText(earlier);
        await page.keyboard.press("Escape");
      } else {
        await expect(shownLinks).toHaveText(earlier);
      }
    });
  }
}

