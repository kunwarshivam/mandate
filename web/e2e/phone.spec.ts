import { type Locator, type Page, expect, test } from "@playwright/test";
import { AGENT_IDS } from "../src/fixtures/workspace";
import { SCREENS, SECTION_INDEX } from "../src/lib/screens";

/**
 * The phone is a remote control (DEC-207): two things in the header, four tabs and Stop (DEC-452), one navigation,
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
      expect(await labels(controls)).toEqual(["Owlhead, dashboard"]);
      await expect(header(page).locator("[data-slot=environment-badge]")).toBeVisible();
      await expect(header(page).locator("[data-slot=environment-badge]")).toContainText("PAPER");
      expect(await smallTargets(header(page))).toEqual([]);
    });

    test("four tabs and then Stop sit at the bottom, 44 px or more, frosted, and the page never scrolls sideways", async ({ page }) => {
      await open(page, "/", width);
      const items = tabBar(page).locator(":scope > a, :scope > button");
      expect((await labels(items)).map((l) => l.replace(/\d+ open/, "").trim())).toEqual(["Home", "Messages", "Agents", "More", "Stop"]);
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
      await open(page, "/agents", width);
      await expect(page.locator("[data-sidebar]")).toHaveCount(0);
      expect(await page.getByRole("navigation").locator("visible=true").evaluateAll((els) => els.map((el) => el.getAttribute("aria-label")))).toEqual(["Main"]);
      await expect(header(page).getByRole("button", { name: /sidebar|menu|Go to/i })).toHaveCount(0);
    });

    test("More reaches every built screen and no screen still to come, with the account and Search (DEC-513)", async ({ page }) => {
      await open(page, "/", width);
      const reached = new Set(await tabBar(page).locator(":scope > a").evaluateAll((els) => els.map((el) => el.getAttribute("href"))));
      const sheet = await openMore(page);
      await expect(moreTab(page)).toHaveAttribute("aria-expanded", "true");
      const account = sheet.getByRole("region", { name: "Account and workspace" });
      await expect(account).toBeVisible();
      await expect(account.getByRole("button", { name: /^Workspace: / })).toBeVisible();
      for (const href of await sheet.getByRole("link").evaluateAll((els) => els.map((el) => el.getAttribute("href")))) if (href) reached.add(href);
      for (const s of SCREENS) expect(reached.has(s.href), s.href).toBe(s.built);
      for (const href of [SECTION_INDEX.audit.href, SECTION_INDEX.workspace.href]) expect(reached, href).toContain(href);
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
      await (await openMore(page)).getByRole("link", { name: "Positions" }).click();
      await expect(page).toHaveURL("/positions");
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
  test.describe(`${width} px, Home`, () => {
    test("Needs you comes first, the request with a static time, then the account, the agents and the decisions (DEC-468)", async ({ page }) => {
      await open(page, "/?scenario=stale", width);
      const headings = await page.locator("#main h2").locator("visible=true").evaluateAll((els) => els.map((el) => el.textContent?.replace(/\d+ items?$/, "").trim()));
      expect(headings).toEqual(["Needs you", "Account equity", "Agents", "Decisions"]);
      const rows = page.locator("[data-slot=needs-you] li");
      expect(await rows.evaluateAll((els) => els.map((el) => el.getAttribute("data-kind"))), "the request, then one line per agent and condition, and no feed (DEC-513 item 9)").toEqual(["request", "alert"]);
      await expect(rows.first()).toContainText(/Skipped at \d\d:\d\d:\d\d [A-Z]+ if you do nothing$/);
      await expect(rows.first()).not.toContainText("left");
      expect(await smallTargets(page.locator("[data-slot=needs-you]"))).toEqual([]);
      await expect(page.locator("#main").getByText(/asks to buy|asked you to buy/).locator("visible=true")).toHaveCount(1);
    });

    test("Needs you is one row of cards that scrolls sideways, however many wait, with each price in full", async ({ page }) => {
      await open(page, "/?scenario=approvals", width);
      const list = page.locator("[data-slot=needs-you] ul");
      const items = list.locator(":scope > li");
      expect(await items.count(), "the three open requests: more cards than a phone's width holds").toBeGreaterThan(2);
      const tops = await items.evaluateAll((els) => els.map((el) => Math.round(el.getBoundingClientRect().top)));
      expect(new Set(tops).size, "every card sits on the same row").toBe(1);
      const box = (await list.boundingBox())!;
      expect(box.height, "the strip stays one card tall").toBeLessThanOrEqual(110);
      expect(await list.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(true);
      const first = (await items.first().boundingBox())!;
      expect(first.x + first.width, "the next card shows at the edge").toBeLessThan(width);
      const request = items.first().getByRole("link");
      expect(await request.evaluate((el) => el.scrollWidth <= el.clientWidth && [...el.querySelectorAll("*")].every((s) => getComputedStyle(s).textOverflow !== "ellipsis"))).toBe(true);
      await expect(request).toContainText(/\$[\d,]+\.\d{2}/);
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    });

    test("the range picker sits wholly above the tab bar on the first screen", async ({ page }) => {
      await open(page, "/", width);
      const picker = (await page.locator("[data-slot=account-equity] [data-slot=range-picker]").boundingBox())!;
      const bar = (await tabBar(page).boundingBox())!;
      expect(picker.y).toBeGreaterThan(0);
      expect(picker.y + picker.height, "the range picker ends above the tab bar").toBeLessThanOrEqual(bar.y);
      expect(await smallTargets(page.locator("[data-slot=range-picker]"))).toEqual([]);
    });

    test("agent rows carry the name, the state and the headroom, and no P&L", async ({ page }) => {
      await open(page, "/", width);
      const rows = page.locator("[data-slot=phone-agent]");
      await expect(rows).toHaveCount(3);
      for (const row of await rows.all()) {
        await expect(row).toBeVisible();
        await expect(row.locator("[data-slot=headroom]")).toHaveText(/^\$[\d,]+\.\d{2} (above|below) its /);
        await expect(row.locator("[data-direction], [data-placeholder=performance], [data-slot=disclosure]")).toHaveCount(0);
        await expect(row).not.toContainText(/P&L|today|[+−]\$/);
      }
      await expect(page.locator("[data-slot=agent-band]").first()).toBeHidden();
      expect(await smallTargets(page.locator("[data-slot=phone-agents]"))).toEqual([]);
    });

    test("Home ends on three decisions, their tally and All decisions, and positions stay off Home", async ({ page }) => {
      await open(page, "/", width);
      const decisions = page.getByRole("region", { name: "Decisions" });
      await expect(decisions.locator("[data-slot=timeline-entry]").locator("visible=true")).toHaveCount(3);
      await expect(decisions.getByRole("link", { name: "All decisions" })).toBeVisible();
      await expect(decisions.locator("[data-slot=decision-tally]")).toHaveText(/^The latest 3 decisions: /);
      await expect(page.getByRole("region", { name: "Positions" })).toBeHidden();
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
      expect(await smallTargets(decisions)).toEqual([]);
    });
  });
}

const AGENT = `/agents/${AGENT_IDS.btc}`;
const SWING = `/agents/${AGENT_IDS.swing}`;

for (const width of PHONES) {
  test.describe(`${width} px, an agent`, () => {
    test("the name, its state and Stop this agent come first, then the equity and the headroom, with no tabs", async ({ page }) => {
      await open(page, SWING, width);
      const head = page.locator("[data-slot=page-header]");
      await expect(head.getByRole("heading", { level: 1 })).toBeVisible();
      await expect(head.locator("[data-slot=mode-badge]")).toBeVisible();
      await expect(head.getByRole("button", { name: "Stop this agent…" })).toBeVisible();
      await expect(head.getByRole("navigation", { name: "Agent sections" })).toBeHidden();
      const headings = await page.locator("#main h2").locator("visible=true").evaluateAll((els) => els.map((el) => el.textContent?.trim()));
      expect(headings).toEqual(["Waiting for you", "Equity against your mandate", "Headroom", "This agent"]);
      await expect(page.locator("[data-slot=phone-waiting] li").first()).toContainText(/Skipped at \d\d:\d\d:\d\d [A-Z]+ if you do nothing$/);
      await expect(page.locator("[data-layout=rail]")).toBeHidden();
      await expect(page.getByRole("region", { name: "Key figures" })).toBeHidden();
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
      expect(await smallTargets(page.locator("#main"))).toEqual([]);
    });

    test("Stop this agent shares the title's row, one paper badge shows, and the chart is 200 px", async ({ page }) => {
      await open(page, AGENT, width);
      const head = page.locator("[data-slot=page-header]");
      const title = (await head.getByRole("heading", { level: 1 }).boundingBox())!;
      const stop = head.getByRole("button", { name: "Stop this agent…" });
      expect((await stop.innerText()).trim()).toBe("Stop");
      const s = (await stop.boundingBox())!;
      expect(s.height).toBeGreaterThanOrEqual(MIN_TARGET);
      expect(s.y, "Stop starts within the title block's height, not under it").toBeLessThan(title.y + title.height);
      expect(s.x + s.width).toBeLessThanOrEqual(width);
      await expect(page.locator("[data-slot=environment-badge]").locator("visible=true")).toHaveCount(1);
      await expect(header(page).locator("[data-slot=environment-badge]")).toBeVisible();
      expect((await page.locator("[data-slot=agent-equity] [data-slot=chart-canvas]").boundingBox())!.height).toBe(200);
      await stop.click();
      await expect(page.getByRole("dialog", { name: /^Stop/ })).toContainText("Agent 1");
    });

    test("the level legend waits behind Levels", async ({ page }) => {
      await open(page, AGENT, width);
      const hero = page.locator("[data-slot=agent-equity]");
      const disclosure = hero.getByRole("button", { name: "Performance disclosure" });
      await expect(disclosure).toBeVisible();
      await expect(disclosure).toHaveAccessibleDescription("[[DISCLOSURE-PERFORMANCE]]");
      const toggle = hero.getByRole("button", { name: "Levels" });
      await expect(toggle).toHaveAttribute("aria-expanded", "false");
      await expect(hero.locator("[data-slot=level-legend]")).toBeHidden();
      await toggle.click();
      await expect(toggle).toHaveAttribute("aria-expanded", "true");
      await expect(hero.locator("[data-slot=level-legend]")).toBeVisible();
      expect(await hero.locator("[data-slot=level-legend] li").count()).toBeGreaterThanOrEqual(5);
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    });

    test("Headroom has a row per limit with a meter, in ink and the marker, and no gain or loss colour", async ({ page }) => {
      await open(page, AGENT, width);
      const rows = page.locator("[data-slot=headroom] [data-slot=headroom-row]");
      expect(await rows.count()).toBeGreaterThanOrEqual(3);
      for (const row of await rows.all()) {
        await expect(row).toBeVisible();
        const meter = row.locator("[data-slot=headroom-meter]");
        await expect(meter).toBeVisible();
        expect((await meter.boundingBox())!.height).toBeLessThanOrEqual(6);
      }
      const colours = await page.locator("[data-slot=headroom]").evaluate((el) => {
        const probe = (v: string) => {
          const s = document.createElement("span");
          s.style.color = `var(${v})`;
          document.body.append(s);
          const c = getComputedStyle(s).color;
          s.remove();
          return c;
        };
        const banned = new Set(["--gain", "--loss", "--gain-cvd", "--loss-cvd", "--crimson"].map(probe));
        return [...el.querySelectorAll("*")].flatMap((n) => {
          const cs = getComputedStyle(n);
          return [cs.color, cs.backgroundColor].filter((c) => banned.has(c));
        });
      });
      expect(colours).toEqual([]);
      await expect(page.locator("[data-slot=headroom]")).toContainText(/Daily loss limit\$[\d,]+\.\d{2} headroom/);
    });

    test("the section links at the foot open each section, and the list follows", async ({ page }) => {
      await open(page, AGENT, width);
      const links = page.getByRole("navigation", { name: "This agent" });
      await expect(links).toBeVisible();
      expect(await labels(links.getByRole("link"))).toEqual([
        "Overview",
        expect.stringMatching(/^Positions\d+$/),
        expect.stringMatching(/^Orders\d+$/),
        "Decisions",
        "Approvals",
        "Mandate",
        "Prove",
        "Activity",
      ]);
      expect(await smallTargets(links)).toEqual([]);
      await links.getByRole("link", { name: /^Orders/ }).click();
      await expect(page).toHaveURL(`${AGENT}/orders`);
      await expect(page.getByRole("navigation", { name: "This agent" }).getByRole("link", { name: /^Orders/ })).toHaveAttribute("aria-current", "page");
      await page.getByRole("navigation", { name: "This agent" }).getByRole("link", { name: "Mandate" }).click();
      await expect(page).toHaveURL(`${AGENT}/mandate`);
      await expect(page.locator("[data-slot=envelope]")).toBeVisible();
      await expect(tabStop(page)).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    });
  });
}

for (const width of PHONES) {
  test.describe(`${width} px, approvals`, () => {
    test("the inbox gives each request one hairline row with its static time", async ({ page }) => {
      await open(page, "/approvals?scenario=approvals", width);
      const rows = page.getByRole("region", { name: "Open, by deadline" }).getByRole("link");
      expect(await rows.count()).toBeGreaterThan(1);
      for (const row of await rows.all()) {
        await expect(row).toBeVisible();
        expect(await row.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe("rgba(0, 0, 0, 0)");
        expect(await row.evaluate((el) => getComputedStyle(el.parentElement!).borderBottomWidth)).toBe("1px");
        await expect(row.locator("[data-slot=deadline]")).toHaveText(/^Skipped at \d\d:\d\d:\d\d [A-Z]+ if you do nothing\s*$/, { useInnerText: true });
        await expect(row.locator("[data-slot=remaining]")).toBeHidden();
      }
      expect(await smallTargets(page.locator("#main"))).toEqual([]);
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    });

    test("a request fills the screen, with Approve and Skip equal and pinned above the tab bar", async ({ page }) => {
      await open(page, "/approvals?scenario=approvals", width);
      await page.getByRole("region", { name: "Open, by deadline" }).getByRole("link").first().click();
      await expect(page).toHaveURL(/\/approvals\/apr_/);
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

test("desktop agent keeps its tabs, its mandate card and the full overview, with the legend open", async ({ page }) => {
  await open(page, AGENT, 1440, 900);
  await expect(page.getByRole("navigation", { name: "Agent sections" })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "This agent" })).toBeHidden();
  await expect(page.locator("[data-slot=mandate-card]")).toBeVisible();
  for (const name of ["Key figures", "Positions", "Working orders", "Recent decisions", "Activity"]) await expect(page.getByRole("region", { name, exact: true })).toBeVisible();
  await expect(page.locator("[data-slot=level-legend]")).toBeVisible();
  await expect(page.getByRole("button", { name: "Levels" })).toBeHidden();
  await expect(page.locator("[data-slot=headroom]")).toBeHidden();
  await expect(page.locator("[data-slot=page-header] [data-slot=mode-badge]")).toBeHidden();
});

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

test("desktop Home leads with the account and agent bands with P&L and disclosure, then assets, beside a rail of Needs you and the latest decisions (DEC-468)", async ({ page }) => {
  await open(page, "/", 1440, 900);
  const rail = page.locator("main [data-layout=rail]");
  const column = page.locator("main [data-layout=main]");
  await expect(rail.locator("[data-slot=needs-you]")).toBeVisible();
  await expect(column.locator("[data-slot=account-equity]")).toBeVisible();
  await expect(rail.locator("[data-slot=account-equity]")).toHaveCount(0);
  const box = (await column.boundingBox())!;
  expect((await rail.boundingBox())!.x, "the rail sits right of the money").toBeGreaterThan(box.x + box.width);
  const bands = page.locator("[data-slot=agent-band]");
  await expect(bands).toHaveCount(3);
  for (const band of await bands.all()) {
    await expect(band).toBeVisible();
    await expect(band.getByRole("button", { name: "Performance disclosure" })).toBeVisible();
    await expect(band.locator("[data-direction]").first()).toBeVisible();
  }
  await expect(page.locator("[data-slot=phone-agent]").first()).toBeHidden();
  await expect(page.getByRole("region", { name: "Assets" })).toBeVisible();
  const decisions = rail.getByRole("region", { name: "Decisions" });
  await expect(decisions.locator("[data-slot=timeline-entry]")).toHaveCount(4);
  await expect(decisions.locator("[data-slot=decision-tally]")).toHaveText(/^The latest 4 decisions: /);
  await expect(decisions.getByRole("link", { name: "All decisions" })).toBeVisible();
  await expect(column.getByRole("region", { name: "Decisions" })).toHaveCount(0);
  await expect(page.locator("[data-slot=paper-note]")).toHaveCount(1);
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
