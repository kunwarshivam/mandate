import { type Page, expect, test } from "@playwright/test";
import { heroPhoneOrderClass, type HeroRow } from "../src/components/site/hero-order";

/**
 * C-2: the welcome hero's order on a phone. The buttons belong directly under the subhead, on the
 * phone's first screen, with the beta line visibly after them — all by the `max-sm:` classes the
 * hero's rows carry (DEC-207: CSS, not a script media query, and the served DOM order never
 * changes), scoped below the page's own `sm:` boundary, which this spec reads from the same seam
 * the rows do. From 40rem the header keeps today's order: the beta line between the subhead and
 * the actions. Both themes run — the light and dark projects each run this file.
 */

const SUBHEAD =
  "A trading agent for your own brokerage account. It does the reading and the watching, trades only inside rules you write, and writes down why it placed every order.";
const BETA = "Private beta, opening a few people at a time";
const BUTTONS = ["Sign the guestbook", "See why it traded", "Look inside the app", "Sign in"] as const;
const CLASSES: Record<HeroRow, string> = { subhead: heroPhoneOrderClass("subhead"), actions: heroPhoneOrderClass("actions"), "beta-line": heroPhoneOrderClass("beta-line") };
/** The place a row's class puts it in the header's grid, read out of the seam's own digits. */
const PLACE: Record<HeroRow, string> = {
  subhead: /order-(\d+)$/.exec(CLASSES.subhead)![1],
  actions: /order-(\d+)$/.exec(CLASSES.actions)![1],
  "beta-line": /order-(\d+)$/.exec(CLASSES["beta-line"])![1],
};

/** Waits out the desktop's power-on (DEC-905), so what is measured is where things rest. */
const settled = (page: Page) =>
  page.evaluate(() =>
    Promise.all(
      document
        .getAnimations()
        .filter((a) => a.timeline instanceof DocumentTimeline && a.effect?.getComputedTiming().endTime !== Infinity)
        .map((a) => a.finished.catch(() => undefined)),
    ),
  );

/** Opens the landing page with its home page in front; the app's tab opens first, as a visit opens (DEC-906). */
async function open(page: Page, width: number, height = 844) {
  await page.setViewportSize({ width, height });
  await page.goto("/welcome", { waitUntil: "load" });
  await settled(page);
  await page.getByRole("tab", { name: "Owlhead Home Page" }).click();
  await page.locator("[data-slot=landing]").waitFor();
}

interface HeroRead {
  /** Each header child in the order it is drawn, and in the order it is served, by its slot. */
  drawn: string[];
  documentOrder: string[];
  rows: Record<HeroRow, { top: number; bottom: number; left: number; right: number; order: string; carriesClass: boolean; inGrid: boolean; visible: boolean }>;
  buttons: { name: string; top: number; bottom: number; left: number; right: number; visible: boolean }[];
  sideways: boolean;
}

/**
 * The hero as the browser really draws it: the header's children in draw order and in document
 * order, each named row's box, its computed `order`, whether it carries the seam's `max-sm:`
 * class as a whole class token (never matched as a substring), whether it is a direct child of
 * the grid `order` orders, and each action button's box and visibility.
 */
function hero(page: Page) {
  return page.evaluate<HeroRead, { subhead: string; beta: string; buttons: string[]; classes: Record<HeroRow, string> }>(
    ({ subhead, beta, buttons, classes }) => {
      const header = document.querySelector<HTMLElement>("[data-slot=landing-page] > header")!;
      const box = (el: Element) => {
        const r = el.getBoundingClientRect();
        return { top: r.top, bottom: r.bottom, left: r.left, right: r.right };
      };
      const row = (el: HTMLElement, cls: string) => ({
        ...box(el),
        order: getComputedStyle(el).order,
        carriesClass: el.classList.contains(cls),
        inGrid: el.parentElement === header,
        visible: el.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true }),
      });
      const subheadRow = [...header.querySelectorAll<HTMLElement>("p")].find((p) => p.textContent === subhead)!;
      const betaRow = [...header.querySelectorAll<HTMLElement>("p")].find((p) => p.textContent?.includes(beta))!;
      const actionsRow = header.querySelector<HTMLElement>('[data-slot="hero-actions"]')!;
      const slotOf = (el: Element) =>
        el === subheadRow ? "subhead" : el === betaRow ? "beta-line" : el === actionsRow ? "actions" : (el.getAttribute("data-slot") ?? el.tagName.toLowerCase());
      const children = [...header.children];
      return {
        drawn: [...children].sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top).map(slotOf),
        documentOrder: children.map(slotOf),
        rows: {
          subhead: row(subheadRow, classes.subhead),
          actions: row(actionsRow, classes.actions),
          "beta-line": row(betaRow, classes["beta-line"]),
        },
        buttons: buttons.map((name) => {
          const el = [...actionsRow.querySelectorAll<HTMLElement>("button, a")].find((b) => b.textContent?.trim() === name)!;
          return { name, ...box(el), visible: el.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true }) };
        }),
        sideways: document.documentElement.scrollWidth > window.innerWidth,
      };
    },
    { subhead: SUBHEAD, beta: BETA, buttons: [...BUTTONS], classes: CLASSES },
  );
}

test("390 px: the buttons stand directly under the subhead on the first screen, the beta line visibly after them", async ({ page }) => {
  await open(page, 390);
  const read = await hero(page);
  // All three rows are grid items of the header, which is what `order` orders.
  for (const name of ["subhead", "actions", "beta-line"] as const) expect(read.rows[name].inGrid, `${name} sits in the hero's grid`).toBe(true);
  // The classes are carried as whole tokens and the browser honours them below the sm boundary.
  for (const name of ["subhead", "actions", "beta-line"] as const) {
    expect(read.rows[name].carriesClass, `${name} carries ${CLASSES[name]} as a whole class token`).toBe(true);
    expect(read.rows[name].order, `${name} is drawn in the place its class gives it`).toBe(PLACE[name]);
  }
  // The phone draws the wordmark and the owls, then the subhead, the buttons, the beta line.
  expect(read.drawn).toEqual(["h1", "owl-perch", "subhead", "actions", "beta-line"]);
  // Directly under: the buttons' row is the next row under the subhead, with only grid gap between.
  expect(read.rows.actions.top).toBeGreaterThanOrEqual(read.rows.subhead.bottom - 0.5);
  // The beta line after them: drawn, not cut off at the phone's edge, and below the buttons.
  expect(read.rows["beta-line"].visible, "the beta line is drawn").toBe(true);
  expect(read.rows["beta-line"].top).toBeGreaterThanOrEqual(read.rows.actions.bottom - 0.5);
  expect(read.rows["beta-line"].left).toBeGreaterThanOrEqual(-0.5);
  expect(read.rows["beta-line"].right, "the beta line is not truncated").toBeLessThanOrEqual(390);
  expect(read.sideways, "the page does not scroll sideways").toBe(false);
  // The hero's four actions are drawn, and they are on the phone's first screen.
  for (const button of read.buttons) {
    expect(button.visible, `${button.name} is drawn`).toBe(true);
    expect(button.left).toBeGreaterThanOrEqual(-0.5);
    expect(button.right, `${button.name} fits the phone's width`).toBeLessThanOrEqual(390);
    expect(button.bottom, `${button.name} is above the fold`).toBeLessThanOrEqual(844);
  }
});

test("1440 px: the desktop's order is untouched — the beta line keeps its place between the subhead and the actions", async ({ page }) => {
  await open(page, 1440, 900);
  const read = await hero(page);
  // The served DOM order never changes on any width: subhead, beta line, actions.
  expect(read.documentOrder).toEqual(["h1", "owl-perch", "subhead", "beta-line", "actions"]);
  // Above the page's sm boundary the classes order nothing: all three compute order 0 …
  for (const name of ["subhead", "actions", "beta-line"] as const) {
    expect(read.rows[name].order, `${name}'s class stays strictly below the sm boundary`).toBe("0");
    expect(read.rows[name].carriesClass, `${name} still carries ${CLASSES[name]}, which only a phone honours`).toBe(true);
  }
  // … and the browser draws the served order: the beta line between the subhead and the actions.
  expect(read.drawn).toEqual(["h1", "owl-perch", "subhead", "beta-line", "actions"]);
  expect(read.rows["beta-line"].top).toBeGreaterThanOrEqual(read.rows.subhead.bottom - 0.5);
  expect(read.rows.actions.top).toBeGreaterThanOrEqual(read.rows["beta-line"].bottom - 0.5);
});

test("the order tracks the page's own sm boundary: one width below it, one width at and above it", async ({ page }) => {
  for (const [width, side] of [[390, "below"], [640, "at"]] as const) {
    await open(page, width);
    const read = await hero(page);
    const orders = [read.rows.subhead.order, read.rows.actions.order, read.rows["beta-line"].order];
    expect(orders, `${width} px, ${side} the boundary`).toEqual(side === "below" ? [PLACE.subhead, PLACE.actions, PLACE["beta-line"]] : ["0", "0", "0"]);
  }
});
