import { type Locator, type Page, expect, test } from "@playwright/test";
import { APPROVAL_IDS } from "../src/fixtures/workspace";
import { SCREENS } from "../src/lib/screens";

/**
 * The phone is a remote control (DEC-207): two things in the header, three tabs and Stop (DEC-452), one navigation,
 * a banner only when a feed is failing, and everything else one press away under More. Checked at
 * the four common phone widths, in the light and dark projects, and with reduced motion.
 */

const PHONES = [320, 375, 390, 430] as const;
const HEIGHT = 844;
const MIN_TARGET = 44;

const header = (page: Page) => page.getByRole("banner");
const tabBar = (page: Page) => page.getByRole("navigation", { name: "Main" });
const moreTab = (page: Page) => tabBar(page).getByRole("button", { name: "More" });
const moreSheet = (page: Page) => page.getByRole("dialog", { name: "More" });
const tabStop = (page: Page) => tabBar(page).getByRole("button", { name: "Stop", exact: true });

async function open(page: Page, path: string, width: number, height = HEIGHT) {
  await page.setViewportSize({ width, height });
  await page.goto(path);
  await page.waitForLoadState("networkidle");
}

async function openMore(page: Page) {
  await expect(async () => {
    if ((await moreTab(page).getAttribute("aria-expanded")) !== "true") await moreTab(page).click();
    await expect(moreSheet(page)).toBeVisible({ timeout: 1000 });
  }).toPass({ timeout: 15_000 });
  return moreSheet(page);
}

async function labels(items: Locator) {
  return items.evaluateAll((els) => els.map((el) => el.getAttribute("aria-label") ?? el.textContent?.trim() ?? ""));
}

/** Every visible control inside a box that is smaller than a 44px touch target. */
async function smallTargets(root: Locator) {
  return root.evaluate((el, min) => {
    const small: string[] = [];
    for (const c of el.querySelectorAll<HTMLElement>("a[href], button, [role=button], input, select, summary")) {
      if (!c.checkVisibility()) continue;
      // The disclosure symbol's target is a pseudo-element that grows to 44px only under a coarse pointer; disclosure.spec.ts measures it with touch.
      if (c.dataset.slot === "disclosure-trigger") continue;
      // A stretched link's ::after fills its positioned row, which is the target a finger meets.
      const stretched = getComputedStyle(c, "::after").position === "absolute";
      const r = (stretched ? (c.offsetParent ?? c) : c).getBoundingClientRect();
      if (r.width < min - 0.5 || r.height < min - 0.5) small.push(`${c.tagName} ${(c.getAttribute("aria-label") ?? c.textContent ?? "").trim().slice(0, 40)} ${Math.round(r.width)}x${Math.round(r.height)}`);
    }
    return small;
  }, MIN_TARGET);
}

for (const width of PHONES) {
  test.describe(`${width} px`, () => {
    test("the header holds two things: the mark and the paper badge", async ({ page }) => {
      await open(page, "/", width);
      const controls = header(page).locator("a[href], button, [role=button], input").locator("visible=true");
      expect(await labels(controls)).toEqual(["Owlhead, home"]);
      await expect(header(page).locator("[data-slot=environment-badge]")).toBeVisible();
      await expect(header(page).locator("[data-slot=environment-badge]")).toContainText("PAPER");
      expect(await smallTargets(header(page))).toEqual([]);
    });

    test("four tabs and then Stop sit at the bottom, 44 px or more, frosted, and the page never scrolls sideways", async ({ page }) => {
      await open(page, "/", width);
      const items = tabBar(page).locator(":scope > a, :scope > button");
      expect((await labels(items)).map((l) => l.replace(/\d+ open/, "").trim())).toEqual(["Home", "Portfolio", "New agent", "More", "Stop"]);
      await expect(tabStop(page)).toBeEnabled();
      await expect(tabStop(page)).toBeInViewport({ ratio: 1 });
      const bar = (await tabBar(page).boundingBox())!;
      expect(bar.y + bar.height).toBeCloseTo(HEIGHT, 0);
      expect(bar.width).toBe(width);
      expect(await smallTargets(tabBar(page))).toEqual([]);
      expect(await tabBar(page).evaluate((el) => getComputedStyle(el).backdropFilter)).not.toBe("none");
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    });

    test("there is one navigation: no sidebar, no menu button, no search button", async ({ page }) => {
      await open(page, "/settings", width);
      await expect(page.locator("[data-sidebar]")).toHaveCount(0);
      expect(await page.getByRole("navigation").locator("visible=true").evaluateAll((els) => els.map((el) => el.getAttribute("aria-label")))).toEqual(["Main"]);
      await expect(header(page).getByRole("button", { name: /sidebar|menu|Go to/i })).toHaveCount(0);
    });

    test("More reaches every screen not on the tabs, with the account and Search", async ({ page }) => {
      await open(page, "/", width);
      const reached = new Set(await tabBar(page).locator(":scope > a").evaluateAll((els) => els.map((el) => el.getAttribute("href"))));
      const sheet = await openMore(page);
      await expect(moreTab(page)).toHaveAttribute("aria-expanded", "true");
      const account = sheet.getByRole("region", { name: "Account and workspace" });
      await expect(account).toBeVisible();
      await expect(account.getByRole("button", { name: /^Workspace: / })).toBeVisible();
      for (const href of await sheet.getByRole("link").evaluateAll((els) => els.map((el) => el.getAttribute("href")))) if (href) reached.add(href);
      for (const s of SCREENS) expect(reached.has(s.href), s.href).toBe(s.built);
      await expect(sheet.getByRole("button", { name: /^Search/ })).toBeVisible();
      expect(await smallTargets(sheet)).toEqual([]);
      expect(await sheet.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
      const box = (await sheet.boundingBox())!;
      expect(box.y, "the sheet stops below the header").toBeGreaterThanOrEqual(65);
      const backdrop = (await page.locator("[data-slot=sheet-backdrop]").boundingBox())!;
      expect(backdrop.y, "so does its backdrop").toBeGreaterThanOrEqual(65);
      const stop = tabStop(page);
      await expect(stop, "Stop stays in view and in the accessibility tree").toBeInViewport({ ratio: 1 });
      const uncovered = await stop.evaluate((el) => {
        const r = el.getBoundingClientRect();
        const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
        return hit === el || el.contains(hit);
      });
      expect(uncovered, "Stop is the element at its own centre").toBe(true);
    });

    test("a screen chosen under More opens, closes the sheet and marks More current", async ({ page }) => {
      await open(page, "/", width);
      await (await openMore(page)).getByRole("link", { name: "Settings" }).click();
      await expect(page).toHaveURL("/settings");
      await expect(moreSheet(page)).toBeHidden();
      await expect(moreTab(page)).toHaveAttribute("aria-current", "page");
    });

    test("no strip and no banner while every feed answers", async ({ page }) => {
      await open(page, "/?scenario=normal", width);
      await expect(page.locator("[data-slot=status-strip]")).toBeHidden();
      await expect(page.locator("[data-slot=feed-banner]")).toHaveCount(0);
    });

    test("a one-line banner under the header when a feed is stale, in the strip's words", async ({ page }) => {
      await open(page, "/?scenario=stale", width);
      const banner = page.getByRole("region", { name: "Feed warning" });
      await expect(banner).toBeVisible();
      await expect(banner).toContainText("Market data stale");
      await expect(banner).toContainText("2 degraded");
      expect((await banner.boundingBox())!.height).toBe(34);
      const head = (await header(page).boundingBox())!;
      expect((await banner.boundingBox())!.y).toBeCloseTo(head.y + head.height, 0);
      await expect(page.locator("[data-slot=status-strip]")).toBeHidden();
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    });

    test("Stop on the tab bar is quiet on a calm screen and loud on risk, 44 px either way", async ({ page }) => {
      for (const [scenario, tone] of [
        ["normal", "quiet"],
        ["stale", "loud"],
      ] as const) {
        await open(page, `/?scenario=${scenario}`, width);
        const stop = tabStop(page);
        await expect(stop).toHaveAttribute("data-tone", tone);
        await expect(stop).toBeEnabled();
        const pill = (await stop.locator("[data-slot=stop-pill]").boundingBox())!;
        expect(pill.height, `${scenario}: the visible pill`).toBeGreaterThanOrEqual(MIN_TARGET);
      }
    });

    test("the stale banner and a loud Stop stand together, and the open More sheet leaves Stop pressable", async ({ page }) => {
      await open(page, "/?scenario=stale", width);
      const stop = tabStop(page);
      const banner = page.getByRole("region", { name: "Feed warning" });
      await expect(stop).toHaveAttribute("data-tone", "loud");
      await expect(banner).toBeVisible();
      await expect(stop).toBeInViewport({ ratio: 1 });
      const [s, b] = [(await stop.boundingBox())!, (await banner.boundingBox())!];
      expect(s.y, "the banner sits under the header, far from Stop on the tab bar").toBeGreaterThanOrEqual(b.y + b.height - 0.5);
      await openMore(page);
      await expect(stop, "More is not modal: Stop stays in the accessibility tree").toBeVisible();
      await expect(stop).toHaveAttribute("data-tone", "loud");
      const uncovered = await stop.evaluate((el) => {
        const r = el.getBoundingClientRect();
        const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
        return hit === el || el.contains(hit);
      });
      expect(uncovered, "Stop is the element at its own centre with More open").toBe(true);
      await stop.click();
      await expect(page.getByRole("dialog", { name: /^Stop/ })).toBeVisible();
    });
  });
}

for (const width of PHONES) {
  test.describe(`${width} px, approvals`, () => {
    test("a request fills the screen, with Approve and Skip equal and pinned above the tab bar", async ({ page }) => {
      await open(page, `/approvals/${APPROVAL_IDS.swingXyz}?scenario=approvals`, width);
      const approve = page.getByRole("button", { name: "Approve", exact: true });
      const skip = page.getByRole("button", { name: "Skip", exact: true });
      await expect(approve).toBeVisible();
      const bar = (await tabBar(page).boundingBox())!;
      for (const scroll of ["top", "bottom"] as const) {
        await page.evaluate((to) => window.scrollTo(0, to === "top" ? 0 : document.documentElement.scrollHeight), scroll);
        await page.waitForTimeout(150);
        const a = (await approve.boundingBox())!;
        const s = (await skip.boundingBox())!;
        expect(a.width, `equal width at the ${scroll}`).toBeCloseTo(s.width, 0);
        expect(a.height).toBeCloseTo(s.height, 0);
        expect(a.y).toBeCloseTo(s.y, 0);
        expect(a.height).toBeGreaterThanOrEqual(MIN_TARGET);
        expect(a.y + a.height, `the choices sit above the tab bar at the ${scroll}`).toBeLessThanOrEqual(bar.y);
        expect(a.y + a.height, `and are pinned on it at the ${scroll}`).toBeGreaterThan(bar.y - 24);
      }
      const styles = await Promise.all([approve, skip].map((b) => b.evaluate((el) => { const cs = getComputedStyle(el); return [cs.backgroundColor, cs.color, cs.fontWeight, cs.fontSize, cs.borderRadius].join(" "); })));
      expect(styles[0]).toBe(styles[1]);
      const response = page.getByRole("region", { name: "Your response" });
      await expect(response.locator("time")).toHaveText(/^\d\d:\d\d:\d\d [A-Z]+$/);
      await expect(response.locator("[data-slot=remaining]"), "the request states the minutes left, as the brief asks").toHaveText(/^\(\d+ min left\)$/);
      await expect(tabStop(page)).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    });
  });
}

test("the banner says the deployment is unreachable", async ({ page }) => {
  await open(page, "/?scenario=unreachable", 390);
  await expect(page.getByRole("region", { name: "Feed warning" })).toContainText("Deployment unreachable since");
});

test("the banner holds its height as the market age ticks", async ({ page }) => {
  await open(page, "/?scenario=stale", 390);
  const main = page.locator("#main");
  const top = (await main.boundingBox())!.y;
  await page.waitForTimeout(2500);
  expect((await main.boundingBox())!.y).toBe(top);
});

test.describe("reduced motion", () => {
  test.use({ reducedMotion: "reduce" });
  test("the More sheet opens without a slide", async ({ page }) => {
    await open(page, "/", 390);
    const sheet = await openMore(page);
    expect(await sheet.evaluate((el) => getComputedStyle(el).transitionDuration.split(",").every((d) => parseFloat(d) === 0))).toBe(true);
  });
});
