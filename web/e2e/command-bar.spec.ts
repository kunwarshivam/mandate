import { type JSHandle, type Page, expect, test } from "@playwright/test";
import { AGENT_IDS } from "../src/fixtures/workspace";
import { agentHref } from "../src/lib/screens";

/**
 * From 1024 px the header carries a wide command bar between its two sides: a search icon, the
 * prompt and a ⌘K key on a muted fill with a hairline. It opens the command palette, like ⌘K. The
 * header covers none of the page (its height is look, DEC-739 item 1), and the bar never overlaps
 * the paper badge, the theme menu, the approvals and alerts links, the account menus, the brand or
 * the trail. Below 1024 px it is the compact icon beside the theme menu.
 */

const bar = (page: Page) => page.locator("[data-slot=command-bar]");

/** The command palette, by its role and its accessible name. */
const commandPalette = (page: Page) => page.getByRole("dialog", { name: "Command palette", exact: true });

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
      main: rect(document.getElementById("main")!),
      bar: rect(document.querySelector("[data-slot=command-bar]")!),
      others: others.map((el) => ({ name: el.getAttribute("aria-label") ?? el.textContent?.trim().slice(0, 24) ?? el.tagName, ...rect(el) })),
    };
  });
}

for (const width of [1024, 1280, 1440]) {
  for (const route of ROUTES) {
    test(`${width} px, ${route}: a wide bar that overlaps nothing in a header that covers none of the page`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(route);
      await page.waitForLoadState("networkidle");
      await expect(bar(page)).toBeVisible();
      const found = await boxes(page);
      expect(found.main.top, "the header covers none of the page").toBeGreaterThanOrEqual(found.header.bottom);
      expect(found.bar.top, "the bar sits inside the header").toBeGreaterThanOrEqual(found.header.top);
      expect(found.bar.bottom, "the bar sits inside the header").toBeLessThanOrEqual(found.header.bottom);
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
  const palette = commandPalette(page);
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
  test(`${width} px: the bar gives way to Search in the More sheet, and the header covers none of the page`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/");
    await page.waitForLoadState("networkidle");
    await expect(bar(page)).toBeHidden();
    await expect(page.getByRole("banner").getByRole("button", { name: "Go to…", exact: true })).toHaveCount(0);
    const top = (await page.locator("header").boundingBox())!;
    expect((await page.locator("#main").boundingBox())!.y, "the header covers none of the page").toBeGreaterThanOrEqual(top.y + top.height);
    await page.getByRole("navigation", { name: "Main" }).getByRole("button", { name: "More" }).click();
    await page.getByRole("dialog", { name: "More" }).getByRole("button", { name: /^Search/ }).click();
    const palette = commandPalette(page);
    await expect(palette).toBeVisible();
    await expect(palette.getByRole("combobox", { name: "Command" })).toBeFocused();
  });
}

/** The frame's Stop: at the end of the dock from 1024 px (DEC-207), of the tab bar below it. */
const frameStop = (page: Page, width: number) =>
  page.getByRole("navigation", { name: width < 1024 ? "Main" : "Primary" }).getByRole("button", { name: "Stop", exact: true });

/** Whether the focused element is the one a handle holds. */
async function focusIs(page: Page, element: JSHandle) {
  return page.evaluate((el) => document.activeElement === el, element);
}

/**
 * The palette is not modal (C-21), so it keeps focus itself: Tab and Shift+Tab wrap inside it, and
 * when Escape closes it, focus goes back to the element that had it before it opened. The palette's
 * tab order is walked here with the keyboard, not computed from the component's own rule.
 */
test.describe("the palette keeps focus inside and gives it back (C-21)", () => {
  for (const width of [390, 1280]) {
    test(`${width} px: Tab from the last element wraps to the first, and Shift+Tab from the first to the last`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("/");
      await page.waitForLoadState("networkidle");
      await page.keyboard.press("ControlOrMeta+k");
      const palette = commandPalette(page);
      const first = palette.getByRole("combobox", { name: "Command" });
      await expect(first, "the palette opens on its first element").toBeFocused();
      let last: JSHandle = (await first.elementHandle())!;
      let wrapped = false;
      for (let step = 0; step < 50 && !wrapped; step++) {
        await page.keyboard.press("Tab");
        await expect(palette.locator(":focus"), `Tab ${step + 1} keeps focus in the palette`).toHaveCount(1);
        wrapped = await first.evaluate((el) => el === document.activeElement);
        if (!wrapped) last = await page.evaluateHandle(() => document.activeElement!);
      }
      expect(wrapped, "Tab from the last element comes back to the first").toBe(true);
      await page.keyboard.press("Shift+Tab");
      await expect(palette.locator(":focus"), "Shift+Tab from the first keeps focus in the palette").toHaveCount(1);
      expect(await focusIs(page, last), "Shift+Tab from the first goes to the last").toBe(true);
      await page.keyboard.press("Tab");
      await expect(first, "Tab from the last goes to the first").toBeFocused();
    });

    test(`${width} px, ⌘K: Escape closes the palette and focus goes back to where it was`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("/");
      await page.waitForLoadState("networkidle");
      const stop = frameStop(page, width);
      await stop.focus();
      await expect(stop).toBeFocused();
      await page.keyboard.press("ControlOrMeta+k");
      const palette = commandPalette(page);
      await expect(palette.getByRole("combobox", { name: "Command" })).toBeFocused();
      await page.keyboard.press("Escape");
      await expect(palette, "Escape closes the palette").toBeHidden();
      await expect(stop, "focus goes back to where it was before ⌘K").toBeFocused();
    });
  }

  for (const how of ["Enter", "a press"]) {
    test(`1280 px, the bar opened with ${how}: Escape closes the palette and focus goes back to the bar`, async ({ page }) => {
      await page.setViewportSize({ width: 1280, height: 900 });
      await page.goto("/");
      await page.waitForLoadState("networkidle");
      const palette = commandPalette(page);
      if (how === "Enter") {
        await bar(page).focus();
        await page.keyboard.press("Enter");
      } else {
        await bar(page).click();
      }
      await expect(palette.getByRole("combobox", { name: "Command" })).toBeFocused();
      await page.keyboard.press("Escape");
      await expect(palette, "Escape closes the palette").toBeHidden();
      await expect(bar(page), "focus goes back to the bar that opened the palette").toBeFocused();
    });
  }

  test("390 px, Search in the More sheet: Escape closes the palette and focus goes back to More", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 900 });
    await page.goto("/");
    await page.waitForLoadState("networkidle");
    const more = page.getByRole("navigation", { name: "Main" }).getByRole("button", { name: "More" });
    await more.click();
    await page.getByRole("dialog", { name: "More" }).getByRole("button", { name: /^Search/ }).click();
    const palette = commandPalette(page);
    await expect(palette.getByRole("combobox", { name: "Command" })).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(palette, "Escape closes the palette").toBeHidden();
    await expect(more, "Search went with its sheet, so focus goes back to More, which opened the sheet").toBeFocused();
  });

  for (const width of [390, 1280]) {
    test(`${width} px, the palette's Stop…: the Stop sheet keeps focus, and the palette does not take it back`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("/");
      await page.waitForLoadState("networkidle");
      await frameStop(page, width).focus();
      await page.keyboard.press("ControlOrMeta+k");
      const palette = commandPalette(page);
      await expect(palette.getByRole("combobox", { name: "Command" })).toBeFocused();
      await page.keyboard.type("Stop");
      await expect(page.getByRole("option", { name: /^Stop…/ })).toBeVisible();
      await page.keyboard.press("Enter");
      const sheet = page.getByRole("dialog", { name: /^Stop/ });
      await expect(sheet).toBeVisible();
      await expect(palette).toHaveCount(0);
      await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
      expect(await sheet.evaluate((el) => el.contains(document.activeElement)), "focus stays in the Stop sheet once the palette has gone").toBe(true);
    });
  }
});

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

