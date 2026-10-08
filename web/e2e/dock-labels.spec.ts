import { type Locator, type Page, expect, test } from "@playwright/test";
import { AGENT_IDS, APPROVAL_IDS } from "../src/fixtures/workspace";
import { SCREENS, SECTION_INDEX, type Screen } from "../src/lib/screens";

/**
 * The labelled dock: every item's name reads under its icon at every desktop width, the current
 * section is marked on every route, including inside an agent or an audit screen, the labels hold
 * 4.5:1 on the glass whatever scrolls underneath, nothing on a page ends up under the dock, and the
 * keyboard meets the items in the order they read.
 */

const WIDTHS = [1024, 1280, 1440, 1920] as const;
const LABELS = ["Home", "Messages", "Approvals", "Alerts", "Agents", "Positions", "More"];
const DOCKED: Record<string, string> = { home: "Home", messages: "Messages", approvals: "Approvals", alerts: "Alerts", agents: "Agents", positions: "Positions" };

const dock = (page: Page) => page.getByRole("navigation", { name: "Primary" });
const items = (page: Page) => dock(page).locator("a, button:not([data-slot=stop-control])");
/** The keyboard's order through the dock: every labelled item, then Stop at its end (DEC-452). */
const TAB_ORDER = [...LABELS, "Stop"];

async function open(page: Page, path: string, width = 1440, height = 900) {
  await page.setViewportSize({ width, height });
  await page.goto(path);
  await page.waitForLoadState("networkidle");
}

function sectionOf(s: Screen): string {
  return DOCKED[s.key] ?? "More";
}

/** Only a built screen has a door on the dock (DEC-504); one still to come marks nothing current. */
const ROUTES: [string, string][] = [
  ...SCREENS.filter((s) => s.built).map((s): [string, string] => [s.href, sectionOf(s)]),
  [SECTION_INDEX.audit.href, "More"],
  [SECTION_INDEX.workspace.href, "More"],
  [`/agents/${AGENT_IDS.btc}`, "Agents"],
  [`/agents/${AGENT_IDS.btc}/positions`, "Agents"],
  [`/agents/${AGENT_IDS.swing}/mandate`, "Agents"],
  [`/agents/${AGENT_IDS.btc}/kill-switch`, "Agents"],
  [`/approvals/${APPROVAL_IDS.swingXyz}`, "Approvals"],
  [`/messages/${AGENT_IDS.swing}`, "Messages"],
  [`/messages/${AGENT_IDS.swing}/desk`, "Messages"],
  ["/audit/decisions", "More"],
  ["/audit/timeline", "More"],
];

for (const width of WIDTHS) {
  test(`${width} px: every item shows its label in full, inside the viewport`, async ({ page }) => {
    await open(page, "/", width);
    const found = await items(page).evaluateAll((els) =>
      els.map((el) => {
        const label = el.querySelector<HTMLElement>("[data-slot=dock-label]")!;
        const r = label.getBoundingClientRect();
        const item = el.getBoundingClientRect();
        return {
          text: label.textContent,
          shown: label.checkVisibility({ opacityProperty: true, visibilityProperty: true }) && r.width > 0 && r.height > 0,
          whole: label.scrollWidth <= Math.ceil(r.width) && r.left >= item.left && r.right <= item.right,
          inView: r.left >= 0 && r.right <= innerWidth && r.bottom <= innerHeight,
        };
      }),
    );
    expect(found.map((f) => f.text)).toEqual(LABELS);
    for (const f of found) expect(f, f.text ?? "").toEqual({ text: f.text, shown: true, whole: true, inView: true });
  });
}

test("the dock does not shift when the current section changes", async ({ page }) => {
  const boxes: string[] = [];
  for (const path of ["/", "/connections", "/audit/decisions"]) {
    await open(page, path, 1280);
    boxes.push(JSON.stringify(await dock(page).boundingBox()));
  }
  expect(new Set(boxes).size).toBe(1);
});

test("the current section is marked on every top-level route and inside agents, approvals and audit", async ({ page }) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1280, height: 800 });
  for (const [path, section] of ROUTES) {
    await page.goto(path);
    await expect(dock(page).locator("[data-current]"), path).toHaveCount(1);
    const current = dock(page).locator("[data-current]");
    await expect(current.locator("[data-slot=dock-label]"), path).toHaveText(section);
    await expect(current, path).toHaveAttribute("aria-current", section === "More" ? "true" : "page");
    expect(await current.locator("[data-slot=dock-label]").evaluate((el) => getComputedStyle(el).fontWeight), path).toBe("600");
  }
});

/** A colour as 8-bit sRGB, painted opaque on a canvas so any CSS colour syntax converts. */
async function rgbOf(page: Page, color: string): Promise<number[]> {
  return page.evaluate((color) => {
    const ctx = document.createElement("canvas").getContext("2d")!;
    ctx.fillStyle = color;
    ctx.fillRect(0, 0, 1, 1);
    return [...ctx.getImageData(0, 0, 1, 1).data.slice(0, 3)];
  }, color);
}

/** The pixel the browser painted at a point, blur, saturation and all. */
async function pixelAt(page: Page, x: number, y: number): Promise<number[]> {
  const png = await page.screenshot({ clip: { x, y, width: 1, height: 1 }, animations: "disabled" });
  return page.evaluate(async (b64) => {
    const img = await createImageBitmap(await (await fetch(`data:image/png;base64,${b64}`)).blob());
    const ctx = document.createElement("canvas").getContext("2d")!;
    ctx.drawImage(img, 0, 0);
    return [...ctx.getImageData(0, 0, 1, 1).data.slice(0, 3)];
  }, png.toString("base64"));
}

function contrast(a: number[], b: number[]): number {
  const y = (c: number[]) => {
    const [r, g, bl] = c.map((v) => {
      const s = v / 255;
      return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * bl;
  };
  const [hi, lo] = [y(a), y(b)].sort((p, q) => q - p);
  return (hi + 0.05) / (lo + 0.05);
}

/** A point inside an item's pill, clear of its icon and label. */
async function padOf(item: Locator): Promise<[number, number]> {
  const box = (await item.boundingBox())!;
  return [Math.round(box.x + 5), Math.round(box.y + box.height / 2)];
}

test.describe("labels hold 4.5:1 on the dock's glass, whatever scrolls under it", () => {
  for (const under of ["none", "--foreground", "--ink", "--mandate-marker", "--gain", "--loss", "--lapis-line"]) {
    test(`over ${under === "none" ? "the page" : `solid ${under}`}`, async ({ page }) => {
      await open(page, "/positions", 1280, 800);
      if (under !== "none") {
        await page.evaluate((under) => {
          const holder = document.querySelector("nav[aria-label=Primary]")!.parentElement!;
          const layer = document.createElement("div");
          layer.style.cssText = `position:fixed;inset:auto 0 0 0;height:120px;z-index:29;pointer-events:none;background:var(${under})`;
          holder.before(layer);
        }, under);
      }
      await page.mouse.move(0, 0);
      const current = dock(page).getByRole("link", { name: "Positions" });
      const idle = dock(page).getByRole("link", { name: "Alerts" });
      const hovered = dock(page).getByRole("link", { name: "Agents" });
      await hovered.hover();
      await page.waitForTimeout(250);
      const colorOf = (item: Locator) => item.locator("[data-slot=dock-label]").evaluate((el) => getComputedStyle(el).color);
      for (const [item, what] of [
        [current, "the current label on its pill"],
        [idle, "an idle label on the glass"],
        [hovered, "a hovered label on its lighter pill"],
      ] as const) {
        const [x, y] = await padOf(item);
        const ratio = contrast(await rgbOf(page, await colorOf(item)), await pixelAt(page, x, y));
        expect(ratio, what).toBeGreaterThanOrEqual(4.5);
      }
      if (under !== "none") {
        const [cx, cy] = await padOf(current);
        const top = (await current.boundingBox())!.y;
        expect(contrast(await pixelAt(page, cx, cy), await pixelAt(page, cx, Math.round(top - 3))), "the pill stands off the glass around it").toBeGreaterThanOrEqual(1.25);
      }
    });
  }
});

for (const width of WIDTHS) {
  test(`${width} px: nothing on a long page ends under the dock`, async ({ page }) => {
    for (const path of ["/", "/agents", "/positions", "/audit/decisions", "/audit/timeline", `/agents/${AGENT_IDS.btc}/orders`]) {
      await open(page, path, width, 800);
      await page.evaluate(() => window.scrollTo({ top: document.documentElement.scrollHeight, behavior: "instant" }));
      await page.waitForFunction(() => Math.abs(window.scrollY + innerHeight - document.documentElement.scrollHeight) < 2);
      const bar = (await dock(page).boundingBox())!;
      const under = await page.evaluate((bar) => {
        const main = document.getElementById("main")!;
        return [...main.querySelectorAll<HTMLElement>("*")]
          .filter((el) => el.children.length === 0 || el.matches("a, button, input, select, textarea, img, svg, canvas, [role=row], tr"))
          .filter((el) => el.checkVisibility())
          .map((el) => ({ r: el.getBoundingClientRect(), label: `${el.tagName.toLowerCase()} ${el.textContent?.trim().slice(0, 30) ?? ""}` }))
          .filter(({ r }) => r.width > 0 && r.height > 0 && r.bottom > bar.y + 0.5 && r.top < bar.y + bar.height && r.right > bar.x && r.left < bar.x + bar.width)
          .map(({ label }) => label);
      }, bar);
      expect(under, path).toEqual([]);
    }
  });
}

for (const width of [1024, 1280] as const) {
  test(`${width} px: a toast rises clear of the dock`, async ({ page }) => {
    await open(page, "/", width, 800);
    await page.getByRole("button", { name: "Stop", exact: true }).click();
    const sheet = page.getByRole("dialog", { name: /^Stop/ });
    await sheet.getByRole("button", { name: /Pause all agents/ }).click();
    const toast = page.getByRole("region", { name: "Notifications" }).getByText("Recorded in the journal");
    await expect(toast).toBeVisible({ timeout: 15_000 });
    await page.keyboard.press("Escape");
    await expect(sheet).toBeHidden();
    const bar = (await dock(page).boundingBox())!;
    const bottom = await toast.evaluate((el) => {
      let node: Element = el;
      while (node.parentElement && node.parentElement.getAttribute("aria-label") !== "Notifications") node = node.parentElement;
      return node.getBoundingClientRect().bottom;
    });
    expect(bottom, "the toast ends above the dock").toBeLessThanOrEqual(bar.y);
  });

  test(`${width} px: the Stop sheet's footer is never under the dock`, async ({ page }) => {
    await open(page, "/", width, 700);
    await page.getByRole("button", { name: "Stop", exact: true }).click();
    const sheet = page.getByRole("dialog", { name: /^Stop/ });
    await expect(sheet).toBeVisible();
    const hidden = await sheet.evaluate((el) =>
      [...el.querySelectorAll<HTMLElement>("button, a")]
        .filter((b) => b.checkVisibility())
        .filter((b) => {
          const r = b.getBoundingClientRect();
          if (r.bottom <= 0 || r.top >= innerHeight) return false;
          const hit = document.elementFromPoint(r.left + r.width / 2, Math.min(r.top + r.height / 2, innerHeight - 1));
          return !!hit?.closest("nav[aria-label=Primary]");
        })
        .map((b) => b.textContent?.trim()),
    );
    expect(hidden).toEqual([]);
  });
}

test("the keyboard meets the dock after the page, in reading order, and Shift+Tab walks it back", async ({ page }) => {
  await open(page, "/", 1280, 800);
  const labelOfFocus = () =>
    page.evaluate(() => {
      const el = document.activeElement;
      if (el?.getAttribute("data-slot") === "stop-control") return el.textContent;
      return el?.querySelector("[data-slot=dock-label]")?.textContent ?? null;
    });
  await items(page).first().focus();
  const order = [await labelOfFocus()];
  for (let i = 1; i < TAB_ORDER.length; i++) {
    await page.keyboard.press("Tab");
    order.push(await labelOfFocus());
  }
  expect(order).toEqual(TAB_ORDER);
  const back: (string | null)[] = [];
  for (let i = 1; i < TAB_ORDER.length; i++) {
    await page.keyboard.press("Shift+Tab");
    back.push(await labelOfFocus());
  }
  expect(back).toEqual(TAB_ORDER.slice(0, -1).reverse());
  await page.keyboard.press("Shift+Tab");
  expect(await page.evaluate(() => !!document.activeElement?.closest("#main")), "before Home, the page's last control").toBe(true);
});
