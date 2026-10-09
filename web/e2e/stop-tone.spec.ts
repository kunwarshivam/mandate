import { type Page, expect, test } from "@playwright/test";

/**
 * The Stop control is quiet until something needs the owner (DEC-206): an ink outline at the end of
 * the dock or the phone tab bar (DEC-452), then the filled ink pill while a risk reason stands. Both
 * tones share one box at every width, keep a 44px hit area, hold their contrast in both themes, stay
 * bordered under forced colours, and change colour without any animation of their own.
 */

const QUIET = ["normal", "approvals", "loading"] as const;
const LOUD = ["stale", "drawdown", "reconciliation", "unknown-order", "unreachable"] as const;
const WIDTHS = [320, 390, 1024, 1440];
const DESKTOP = 1024;
const MIN_TARGET = 44;
/** From 64rem the visible pill is as tall as the dock's items, 3.125rem. */
const DESKTOP_PILL = 50;
/** WCAG 2.2: non-text contrast for the outline, and the Stop label's own bar (`STOP_CONTRAST`). */
const MARK_CONTRAST = 3;
const LABEL_CONTRAST = 7;

type Rgba = [number, number, number, number];

interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface Found {
  tone: string | null;
  button: Box;
  pill: Box;
  frameHeight: number;
  frameBg: Rgba;
  /** The solid card the frame's glass is mixed from. */
  cardBg: Rgba;
  pillBg: Rgba;
  border: Rgba;
  text: Rgba;
  borderWidth: number;
  borderStyle: string;
  transitionProperty: string;
  transitionDuration: string;
  animationName: string;
  animations: number;
}

/** The one Stop on screen: the dock's from 64rem, the tab bar's below it. */
function stopIn(page: Page) {
  return page.getByRole("button", { name: "Stop", exact: true });
}

async function load(page: Page, scenario: string, path = "/") {
  await page.goto(`${path}?scenario=${scenario}`);
  await page.waitForLoadState("networkidle");
}

async function inspect(page: Page): Promise<Found> {
  return stopIn(page).evaluate((el) => {
    const toRgba = (css: string): Rgba => {
      const canvas = document.createElement("canvas");
      canvas.width = canvas.height = 1;
      const g = canvas.getContext("2d")!;
      g.fillStyle = css;
      g.fillRect(0, 0, 1, 1);
      const [r, gr, b, a] = g.getImageData(0, 0, 1, 1).data;
      return [r, gr, b, a];
    };
    const box = (node: Element) => {
      const r = node.getBoundingClientRect();
      return { x: r.x, y: r.y, width: r.width, height: r.height };
    };
    const pill = el.querySelector("[data-slot=stop-pill]")!;
    const frame = el.closest("nav")!;
    const s = getComputedStyle(pill);
    return {
      tone: el.getAttribute("data-tone"),
      button: box(el),
      pill: box(pill),
      frameHeight: frame.getBoundingClientRect().height,
      frameBg: toRgba(getComputedStyle(frame).backgroundColor),
      cardBg: (() => {
        const probe = document.createElement("div");
        probe.style.backgroundColor = "var(--card)";
        document.body.append(probe);
        const value = toRgba(getComputedStyle(probe).backgroundColor);
        probe.remove();
        return value;
      })(),
      pillBg: toRgba(s.backgroundColor),
      border: toRgba(s.borderTopColor),
      text: toRgba(s.color),
      borderWidth: parseFloat(s.borderTopWidth),
      borderStyle: s.borderTopStyle,
      transitionProperty: s.transitionProperty,
      transitionDuration: s.transitionDuration,
      animationName: s.animationName,
      animations: [...el.getAnimations({ subtree: true })].length,
    };
  });
}

function luminance([r, g, b]: Rgba): number {
  const lin = (v: number) => {
    const c = v / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

function contrast(a: Rgba, b: Rgba): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

function expectSameBox(a: Box, b: Box, what: string) {
  for (const k of ["x", "y", "width", "height"] as const) expect(Math.abs(a[k] - b[k]), `${what}: ${k} ${a[k]} vs ${b[k]}`).toBeLessThanOrEqual(0.5);
}

test.describe("Stop is quiet on a calm screen", () => {
  for (const scenario of QUIET) {
    test(`${scenario}: an ink outline on the dock or the tab bar, named Stop, with no description`, async ({ page }) => {
      await load(page, scenario);
      const stop = stopIn(page);
      await expect(stop).toBeEnabled();
      await expect(stop).toHaveAttribute("data-tone", "quiet");
      await expect(stop).toHaveAccessibleName("Stop");
      await expect(stop).not.toHaveAttribute("aria-describedby");
      await expect(stop).toHaveAccessibleDescription("");
      const f = await inspect(page);
      expect(f.frameBg[3], "the dock and the tab bar are glass").toBeLessThan(255);
      expect(f.pillBg, "the quiet pill is solid card").toEqual(f.cardBg);
      expect(f.border, "outline and label are the same ink").toEqual(f.text);
      expect(f.borderWidth, "a 2px outline").toBe(2);
      expect(contrast(f.border, f.cardBg), "outline against the card").toBeGreaterThanOrEqual(MARK_CONTRAST);
      expect(contrast(f.text, f.pillBg), "label on the quiet pill").toBeGreaterThanOrEqual(LABEL_CONTRAST);
    });
  }

  test("requests waiting for approval alone leave it quiet", async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await load(page, "approvals");
    await expect(page.getByRole("navigation", { name: "Primary" }).locator("[data-slot=approvals-count]")).toHaveText("3 open");
    await expect(stopIn(page)).toHaveAttribute("data-tone", "quiet");
  });
});

test.describe("Stop turns loud when something needs you", () => {
  for (const scenario of LOUD) {
    test(`${scenario}: the filled ink pill, still named Stop, with a reason for assistive technology`, async ({ page }) => {
      await load(page, scenario);
      const stop = stopIn(page);
      await expect(stop).toBeEnabled();
      await expect(stop).toHaveAttribute("data-tone", "loud");
      await expect(stop).toHaveAccessibleName("Stop");
      await expect(stop).toHaveAccessibleDescription(/^Needs attention: .+/);
      await expect(stop).toHaveText("Stop");
      const f = await inspect(page);
      expect(f.pillBg, "filled in the ink of its own outline").toEqual(f.border);
      expect(f.pillBg).not.toEqual(f.cardBg);
      expect(contrast(f.pillBg, f.cardBg), "the fill against the card").toBeGreaterThanOrEqual(MARK_CONTRAST);
      expect(contrast(f.text, f.pillBg), "label on the loud pill").toBeGreaterThanOrEqual(LABEL_CONTRAST);
    });
  }

  /** The unreachable deployment replaces Home with its own notice, which says why. */
  for (const scenario of LOUD.filter((s) => s !== "unreachable")) {
    test(`${scenario}: Home's Needs you says why, with an agent's condition (C-25)`, async ({ page }) => {
      await page.setViewportSize({ width: 1440, height: 900 });
      await load(page, scenario);
      await expect(stopIn(page)).toHaveAttribute("data-tone", "loud");
      const conditions = page.locator("[data-slot=needs-you] li[data-kind=alert]");
      await expect(conditions.first(), "a condition explains the loud Stop").toBeVisible();
      await expect(conditions.first().getByRole("link")).toHaveAttribute("href", /^\/agents\/agt_/);
    });
  }

  test("unknown-order: Needs you names the agent's unknown order, and opens its record", async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await load(page, "unknown-order");
    const row = page.locator("[data-slot=needs-you] li[data-kind=alert]").filter({ hasText: "Agent 2: an order's state is unknown" });
    await expect(row).toHaveCount(1);
    await expect(row).not.toContainText("QRS");
    await row.getByRole("link").click();
    await expect(page).toHaveURL(/^[^?]*\/agents\/agt_[0-9A-Z]+\/orders\/cid_[0-9A-Z]+/);
    await expect(page.getByRole("main")).toContainText("Unknown");
  });
});

test.describe("both tones share one box (no layout shift)", () => {
  for (const width of WIDTHS) {
    test(`${width} px`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 });
      await load(page, "normal");
      const quiet = await inspect(page);
      await load(page, "stale");
      const loud = await inspect(page);
      expect(quiet.tone).toBe("quiet");
      expect(loud.tone).toBe("loud");
      expectSameBox(quiet.button, loud.button, "hit area");
      expectSameBox(quiet.pill, loud.pill, "pill");
      expect(loud.frameHeight).toBe(quiet.frameHeight);
      for (const f of [quiet, loud]) {
        expect(f.button.x + f.button.width, "right edge on screen").toBeLessThanOrEqual(width);
        expect(f.button.x).toBeGreaterThanOrEqual(0);
        expect(f.button.height, "hit area height").toBeGreaterThanOrEqual(MIN_TARGET);
        expect(f.button.width, "hit area width").toBeGreaterThanOrEqual(MIN_TARGET);
        if (width < DESKTOP) expect(f.pill.height, "phone pill").toBeGreaterThanOrEqual(MIN_TARGET);
        else expect(f.pill.height, "desktop pill").toBeCloseTo(DESKTOP_PILL, 0);
      }
      if (width >= DESKTOP) {
        const home = await page.getByRole("navigation", { name: "Primary" }).getByRole("link", { name: "Home" }).boundingBox();
        expect(home!.height, "the dock's items").toBeCloseTo(DESKTOP_PILL, 0);
        expect(home!.y + home!.height / 2, "centred on the same line as Stop").toBeCloseTo(loud.pill.y + loud.pill.height / 2, 0);
      }
    });
  }

  test("on a phone the tab bar's full height presses Stop, past its 44px pill", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await load(page, "normal");
    const hits = await stopIn(page).evaluate((el) => {
      const pill = el.querySelector("[data-slot=stop-pill]")!.getBoundingClientRect();
      const r = el.getBoundingClientRect();
      const x = r.left + r.width / 2;
      return [r.top + 1, pill.top - 1, pill.bottom + 1, r.bottom - 1].map((y) => {
        const hit = document.elementFromPoint(x, y);
        return hit !== null && (hit === el || el.contains(hit));
      });
    });
    expect(hits).toEqual([true, true, true, true]);
  });
});

test.describe("Stop never flickers or disables on a page change", () => {
  for (const [scenario, tone] of [
    ["normal", "quiet"],
    ["stale", "loud"],
  ] as const) {
    test(`${scenario}: ${tone} in every frame between home and approvals`, async ({ page }) => {
      await page.setViewportSize({ width: 1280, height: 800 });
      await load(page, scenario);
      for (const href of ["/approvals", "/"]) {
        const sampled = page.evaluate(
          () =>
            new Promise<{ tones: string[]; disabled: number }>((done) => {
              const result = { tones: [] as string[], disabled: 0 };
              const end = performance.now() + 1200;
              const tick = () => {
                const el = document.querySelector("nav[aria-label=Primary] [data-slot=stop-control]");
                result.tones.push(el?.getAttribute("data-tone") ?? "missing");
                if (el?.hasAttribute("disabled") || el?.getAttribute("aria-disabled") === "true") result.disabled++;
                if (performance.now() < end) requestAnimationFrame(tick);
                else done(result);
              };
              requestAnimationFrame(tick);
            }),
        );
        await page.getByRole("navigation", { name: "Primary" }).locator(`a[href="${href}"]`).first().click();
        const { tones, disabled } = await sampled;
        await expect(page).toHaveURL(href);
        expect(new Set(tones), `${href}: tones seen`).toEqual(new Set([tone]));
        expect(disabled).toBe(0);
      }
    });
  }
});

test.describe("forced colours", () => {
  for (const scenario of ["normal", "stale"]) {
    test(`${scenario}: still a visible bordered button`, async ({ page }) => {
      await page.emulateMedia({ forcedColors: "active" });
      await load(page, scenario);
      await expect(stopIn(page)).toBeVisible();
      const f = await inspect(page);
      expect(f.borderStyle).toBe("solid");
      expect(f.borderWidth).toBeGreaterThanOrEqual(1);
      expect(contrast(f.border, f.pillBg), "the border against the button's own fill").toBeGreaterThanOrEqual(MARK_CONTRAST);
    });
  }
});

test.describe("motion", () => {
  test("the swap is a colour change at the hover duration, and nothing animates", async ({ page }) => {
    await load(page, "stale");
    const f = await inspect(page);
    expect(f.transitionProperty).toContain("background-color");
    expect(f.transitionProperty).not.toMatch(/\b(all|transform|scale|translate|box-shadow)\b/);
    expect(f.transitionDuration.split(", ").every((d) => d === "0.16s")).toBe(true);
    expect(f.animationName).toBe("none");
    expect(f.animations).toBe(0);
  });

  test("instant under reduced motion", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await load(page, "stale");
    const f = await inspect(page);
    expect(f.transitionProperty).toBe("none");
    expect(f.animationName).toBe("none");
    expect(f.animations).toBe(0);
  });
});
