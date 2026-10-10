import { expect, test } from "@playwright/test";

/**
 * Critique C-18: on Gate decisions at 1440px in dark mode, the Allowed chip's fill sat a step from
 * the card under it, and the chip disappeared beside three ringed verdicts. In a real browser, in
 * both themes (the two projects), the chip keeps an edge that clears WCAG 1.4.11's 3:1 against its
 * own fill and against what surrounds it, and its label keeps 4.5:1 on its fill.
 */

/** WCAG 2.2 non-text contrast for the edge, and body text for the label. */
const MARK_CONTRAST = 3;
const TEXT_CONTRAST = 4.5;

type Rgba = [number, number, number, number];

interface Found {
  /** The inset shadows the ring utility draws, as their colours, with a spread of at least 1px. */
  edges: Rgba[];
  fill: Rgba;
  text: Rgba;
  /** The first solid background behind the chip: the card field, or the page. */
  around: Rgba;
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

test("the Allowed chip keeps a visible edge on Gate decisions at 1440px", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/audit/decisions?scenario=normal");
  await page.waitForLoadState("networkidle");
  const chip = page.locator("[data-slot=verdict]", { hasText: /^Allowed$/ }).first();
  await expect(chip).toBeVisible();
  const f: Found = await chip.evaluate((el) => {
    const toRgba = (css: string): Rgba => {
      const canvas = document.createElement("canvas");
      canvas.width = canvas.height = 1;
      const g = canvas.getContext("2d")!;
      g.fillStyle = css;
      g.fillRect(0, 0, 1, 1);
      const [r, gr, b, a] = g.getImageData(0, 0, 1, 1).data;
      return [r, gr, b, a];
    };
    const topLevel = (value: string) => {
      const parts: string[] = [];
      let depth = 0;
      let start = 0;
      for (let i = 0; i < value.length; i++) {
        if (value[i] === "(") depth++;
        else if (value[i] === ")") depth--;
        else if (value[i] === "," && depth === 0) {
          parts.push(value.slice(start, i));
          start = i + 1;
        }
      }
      parts.push(value.slice(start));
      return parts.map((p) => p.trim());
    };
    const s = getComputedStyle(el);
    const edges = topLevel(s.boxShadow)
      .filter((p) => /\binset\b/.test(p))
      .flatMap((p) => {
        const lengths = [...p.matchAll(/(-?[\d.]+)px/g)].map((m) => Number(m[1]));
        const colour = p.replace(/\binset\b/, "").replace(/-?[\d.]+px/g, "").trim();
        const rgba = toRgba(colour);
        return lengths[3] >= 1 && rgba[3] === 255 ? [rgba] : [];
      });
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
    return { edges, fill: toRgba(s.backgroundColor), text: toRgba(s.color), around };
  });
  expect(f.edges.length, "the chip draws an inset edge").toBeGreaterThan(0);
  const edge = f.edges[f.edges.length - 1];
  expect(contrast(edge, f.fill), "edge against the chip's fill").toBeGreaterThanOrEqual(MARK_CONTRAST);
  expect(contrast(edge, f.around), "edge against the surface around the chip").toBeGreaterThanOrEqual(MARK_CONTRAST);
  expect(contrast(f.text, f.fill), "label on the chip's fill").toBeGreaterThanOrEqual(TEXT_CONTRAST);
});
