import { type Locator, type Page, expect, test } from "@playwright/test";
import { SCREENS, SECTION_INDEX } from "../src/lib/screens";

/**
 * The phone is a remote control (DEC-207): three things in the header, four tabs, one navigation,
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
    test("the header holds three things: the mark, the paper badge and Stop", async ({ page }) => {
      await open(page, "/", width);
      const controls = header(page).locator("a[href], button, [role=button], input").locator("visible=true");
      expect(await labels(controls)).toEqual(["Owlhead, dashboard", "Stop"]);
      await expect(header(page).locator("[data-slot=environment-badge]")).toBeVisible();
      await expect(header(page).locator("[data-slot=environment-badge]")).toContainText("PAPER");
      expect(await smallTargets(header(page))).toEqual([]);
      await expect(header(page).getByRole("button", { name: "Stop", exact: true })).toBeEnabled();
    });

    test("four tabs sit at the bottom, 44 px or more, frosted, and the page never scrolls sideways", async ({ page }) => {
      await open(page, "/", width);
      const items = tabBar(page).locator(":scope > a, :scope > button");
      expect((await labels(items)).map((l) => l.replace(/\d+ open/, "").trim())).toEqual(["Home", "Approvals", "Agents", "More"]);
      const bar = (await tabBar(page).boundingBox())!;
      expect(bar.y + bar.height).toBeCloseTo(HEIGHT, 0);
      expect(bar.width).toBe(width);
      expect(await smallTargets(tabBar(page))).toEqual([]);
      expect(await tabBar(page).evaluate((el) => getComputedStyle(el).backdropFilter)).not.toBe("none");
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    });

    test("there is one navigation: no sidebar, no menu button, no search button", async ({ page }) => {
      await open(page, "/agents", width);
      await expect(page.locator("[data-sidebar]")).toHaveCount(0);
      expect(await page.getByRole("navigation").locator("visible=true").evaluateAll((els) => els.map((el) => el.getAttribute("aria-label")))).toEqual(["Main"]);
      await expect(header(page).getByRole("button", { name: /sidebar|menu|Go to/i })).toHaveCount(0);
    });

    test("More reaches every screen, with the account at the top and Search", async ({ page }) => {
      await open(page, "/", width);
      const reached = new Set(await tabBar(page).locator(":scope > a").evaluateAll((els) => els.map((el) => el.getAttribute("href"))));
      const sheet = await openMore(page);
      await expect(moreTab(page)).toHaveAttribute("aria-expanded", "true");
      const account = sheet.getByRole("region", { name: "Account and workspace" });
      await expect(account).toBeVisible();
      await expect(account.getByRole("button", { name: /^Workspace: / })).toBeVisible();
      for (const href of await sheet.getByRole("link").evaluateAll((els) => els.map((el) => el.getAttribute("href")))) if (href) reached.add(href);
      for (const href of [...SCREENS.map((s) => s.href), SECTION_INDEX.audit.href, SECTION_INDEX.workspace.href]) expect(reached, href).toContain(href);
      await expect(sheet.getByRole("button", { name: /^Search/ })).toBeVisible();
      expect(await smallTargets(sheet)).toEqual([]);
      expect(await sheet.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
      const box = (await sheet.boundingBox())!;
      expect(box.y, "the sheet stops below the header").toBeGreaterThanOrEqual(65);
      const backdrop = (await page.locator("[data-slot=sheet-backdrop]").boundingBox())!;
      expect(backdrop.y, "so does its backdrop").toBeGreaterThanOrEqual(65);
      const stop = header(page).getByRole("button", { name: "Stop", exact: true });
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
      await (await openMore(page)).getByRole("link", { name: "Positions" }).click();
      await expect(page).toHaveURL("/positions");
      await expect(moreSheet(page)).toBeHidden();
      await expect(moreTab(page)).toHaveAttribute("aria-current", "page");
    });

    test("no strip and no banner while every feed answers", async ({ page }) => {
      await open(page, "/?scenario=normal", width);
      await expect(page.locator("[data-slot=status-strip]")).toBeHidden();
      await expect(page.locator("[data-slot=feed-banner]")).toHaveCount(0);
      await expect(page.locator("[data-slot=wire]")).toBeHidden();
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
  });
}

for (const width of PHONES) {
  test.describe(`${width} px, Home`, () => {
    test("Needs you comes first, the request with a static time, then the account, the agents and recent activity", async ({ page }) => {
      await open(page, "/?scenario=stale", width);
      const headings = await page.locator("#main h2").locator("visible=true").evaluateAll((els) => els.map((el) => el.textContent?.replace(/\d+ items?$/, "").trim()));
      expect(headings).toEqual(["Needs you", "Account equity", "Agents", "Recent activity"]);
      const rows = page.locator("[data-slot=needs-you] li");
      expect(await rows.evaluateAll((els) => els.map((el) => el.getAttribute("data-kind")))).toEqual(["request", "alert", "alert", "alert", "alert"]);
      await expect(rows.first()).toContainText(/Skipped at \d\d:\d\d:\d\d [A-Z]+ if you do nothing$/);
      await expect(rows.first()).not.toContainText("left");
      expect(await smallTargets(page.locator("[data-slot=needs-you]"))).toEqual([]);
      await expect(page.locator("#main").getByText(/asks to buy|asked you to buy/).locator("visible=true")).toHaveCount(1);
    });

    test("the range picker sits wholly above the tab bar on the first screen", async ({ page }) => {
      await open(page, "/", width);
      const picker = (await page.locator("[data-slot=account-equity] [data-slot=range-picker]").boundingBox())!;
      const bar = (await tabBar(page).boundingBox())!;
      expect(picker.y).toBeGreaterThan(0);
      expect(picker.y + picker.height, "the range picker ends above the tab bar").toBeLessThanOrEqual(bar.y);
      const canvas = (await page.locator("[data-slot=account-equity] [data-slot=chart-canvas]").boundingBox())!;
      expect(canvas.height).toBe(180);
      expect(await smallTargets(page.locator("[data-slot=range-picker]"))).toEqual([]);
    });

    test("agent rows carry the name, the state and the headroom, and no P&L", async ({ page }) => {
      await open(page, "/", width);
      const rows = page.locator("[data-slot=phone-agent]");
      await expect(rows).toHaveCount(3);
      for (const row of await rows.all()) {
        await expect(row).toBeVisible();
        await expect(row.locator("[data-slot=headroom]")).toHaveText(/^\$[\d,]+\.\d{2} (above|below) its /);
        await expect(row.locator("[data-direction], [data-placeholder=performance]")).toHaveCount(0);
        await expect(row).not.toContainText(/P&L|today|[+−]\$/);
      }
      await expect(page.locator("[data-slot=agent-band]").first()).toBeHidden();
      expect(await smallTargets(page.locator("[data-slot=phone-agents]"))).toEqual([]);
    });

    test("recent activity shows three entries and See all activity, and positions stay off Home", async ({ page }) => {
      await open(page, "/", width);
      const activity = page.getByRole("region", { name: "Recent activity" });
      await expect(activity.locator("ol > li").locator("visible=true")).toHaveCount(3);
      await expect(activity.getByRole("link", { name: "See all activity" })).toBeVisible();
      await expect(page.getByRole("region", { name: "Positions" })).toBeHidden();
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
      expect(await smallTargets(activity)).toEqual([]);
    });
  });
}

test("Home says all clear once the one request is answered", async ({ page }) => {
  await open(page, "/", 390);
  await page.locator("[data-slot=needs-you] li[data-kind=request] a").click();
  await expect(page).toHaveURL(/\/approvals\/apr_/);
  await page.getByRole("button", { name: "Skip", exact: true }).click();
  await expect(page.locator("[data-phase=sent], [data-phase=recorded], [data-status]").first()).toBeVisible();
  await expect(page.getByRole("region", { name: "Outcome" })).toBeVisible({ timeout: 10_000 });
  await expect(async () => {
    await tabBar(page).getByRole("link", { name: "Home" }).click();
    await expect(page).toHaveURL("/", { timeout: 1000 });
  }).toPass({ timeout: 15_000 });
  const clear = page.locator("[data-slot=all-clear]");
  await expect(clear).toBeVisible();
  await expect(clear).toHaveText("All clear. Nothing needs you.");
});

test("desktop Home keeps its rail, its agent bands with P&L and disclosure, and positions", async ({ page }) => {
  await open(page, "/", 1440, 900);
  await expect(page.locator("[data-slot=needs-you]")).toBeHidden();
  await expect(page.locator("[data-slot=waiting]")).toBeVisible();
  await expect(page.locator("[data-slot=alerts-summary]")).toBeVisible();
  const bands = page.locator("[data-slot=agent-band]");
  await expect(bands).toHaveCount(3);
  for (const band of await bands.all()) {
    await expect(band).toBeVisible();
    await expect(band.locator("[data-placeholder=performance]")).toBeVisible();
    await expect(band.locator("[data-direction]").first()).toBeVisible();
  }
  await expect(page.locator("[data-slot=phone-agent]").first()).toBeHidden();
  await expect(page.getByRole("region", { name: "Positions" })).toBeVisible();
  await expect(page.getByRole("region", { name: "Recent activity" }).locator("ol > li").locator("visible=true")).toHaveCount(6);
  expect((await page.locator("[data-slot=account-equity] [data-slot=chart-canvas]").boundingBox())!.height).toBe(260);
});

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
