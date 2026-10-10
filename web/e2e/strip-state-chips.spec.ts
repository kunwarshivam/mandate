import { expect, test } from "@playwright/test";

/**
 * Critique C-26 and the Meaning Rule (web/DESIGN.md): system states carry no meaning colour, and ink
 * means the account's fill, a stopped agent and Stop. So the status strip's "Stale" and "Down"
 * chips, like the chart footer's "Stale", are outlined, never filled with ink. In a real browser, in
 * both themes (the two projects), each chip's fill is the strip's own surface, its edge clears
 * WCAG 1.4.11's 3:1 against that surface, its label reads at 4.5:1, and its word reaches a screen
 * reader. The phone banner draws the same chips, so it is checked too.
 */

const MARK_CONTRAST = 3;
const TEXT_CONTRAST = 4.5;

type Rgba = [number, number, number, number];

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

interface Chip {
  fill: Rgba;
  text: Rgba;
  edge: Rgba;
  edgeWidth: number;
  /** The first opaque background behind the chip: the strip's surface. */
  around: Rgba;
  hiddenFromReaders: boolean;
  label: string;
}

const CASES = [
  { scenario: "stale", state: "stale", label: "Stale", slot: "status-strip", widths: [1024, 1440] },
  { scenario: "unreachable", state: "down", label: "Down", slot: "status-strip", widths: [1024, 1440] },
  { scenario: "stale", state: "stale", label: "Stale", slot: "feed-banner", widths: [390] },
  { scenario: "unreachable", state: "down", label: "Down", slot: "feed-banner", widths: [390] },
] as const;

for (const c of CASES) {
  for (const width of c.widths) {
    test(`${c.scenario}, ${c.slot}, ${width} px: the "${c.label}" chip is outlined, not ink, and reads`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(`/?scenario=${c.scenario}`);
      const region = page.locator(`[data-slot=${c.slot}]`);
      await expect(region).toBeVisible();
      const chip = region.locator(`[data-state=${c.state}] > span`).first();
      await expect(chip).toBeVisible();

      const f: Chip = await chip.evaluate((el) => {
        const toRgba = (css: string): Rgba => {
          const canvas = document.createElement("canvas");
          canvas.width = canvas.height = 1;
          const g = canvas.getContext("2d")!;
          g.clearRect(0, 0, 1, 1);
          g.fillStyle = css;
          g.fillRect(0, 0, 1, 1);
          const [r, gr, b, a] = g.getImageData(0, 0, 1, 1).data;
          return [r, gr, b, a];
        };
        const s = getComputedStyle(el);
        let node = el.parentElement;
        let around = toRgba(getComputedStyle(document.body).backgroundColor);
        while (node) {
          const bg = toRgba(getComputedStyle(node).backgroundColor);
          if (bg[3] === 255) {
            around = bg;
            break;
          }
          node = node.parentElement;
        }
        return {
          fill: toRgba(s.backgroundColor),
          text: toRgba(s.color),
          edge: toRgba(s.borderTopColor),
          edgeWidth: Math.min(...[s.borderTopWidth, s.borderRightWidth, s.borderBottomWidth, s.borderLeftWidth].map((w) => parseFloat(w))),
          around,
          hiddenFromReaders: el.closest("[aria-hidden=true]") !== null,
          label: el.textContent ?? "",
        };
      });

      const noInk = f.fill[3] === 0 || f.fill.slice(0, 3).join() === f.around.slice(0, 3).join();
      expect(noInk, `the chip's fill rgba(${f.fill}) is transparent or the strip's surface rgba(${f.around}), not ink`).toBe(true);
      expect(f.edgeWidth, "the chip draws an edge on every side").toBeGreaterThanOrEqual(1);
      expect(contrast(f.edge, f.around), "edge against the strip's surface").toBeGreaterThanOrEqual(MARK_CONTRAST);
      expect(contrast(f.text, f.around), "label on the strip's surface").toBeGreaterThanOrEqual(TEXT_CONTRAST);

      expect(f.label.startsWith(c.label), `the chip says "${c.label}"`).toBe(true);
      expect(f.hiddenFromReaders, "the chip's word is not hidden from screen readers").toBe(false);
      expect(await region.ariaSnapshot()).toMatch(new RegExp(`\\b${c.label}\\b`));
    });
  }
}
