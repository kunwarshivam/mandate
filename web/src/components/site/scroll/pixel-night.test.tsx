import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { COLS, NIGHT_COLOURS, type NightInk, PixelNight, ROWS, STARS, nightRects } from "./pixel-night";
import { OWL_ROWS } from "./voxel-owl";

/**
 * The lock screen's pixel night (DEC-907): every pixel a flat token colour, the sky's fall into tide
 * dithered rather than blended, every star in the sky, and the brand owl's sun eyes awake in it.
 */

const HORIZON = 62;

/** The night cell by cell, as the picture paints it: sky and tide as two blocks, then the runs over them. */
function grid(): NightInk[][] {
  const cells = Array.from({ length: ROWS }, (_, y) => Array.from({ length: COLS }, (): NightInk => (y >= HORIZON ? "tide" : "sky")));
  for (const r of nightRects()) for (let x = r.x; x < r.x + r.w; x++) cells[r.y]![x] = r.ink;
  return cells;
}

describe("the pixel night", () => {
  it("paints only flat token colours", () => {
    for (const colour of Object.values(NIGHT_COLOURS)) expect(colour).toMatch(/^var\(--[a-z-]+\)$/);
  });

  it("paints every run inside the picture, one cell high, with no two runs on the same cell", () => {
    const taken = new Set<string>();
    for (const r of nightRects()) {
      expect(r.x).toBeGreaterThanOrEqual(0);
      expect(r.x + r.w).toBeLessThanOrEqual(COLS);
      expect(r.y).toBeGreaterThanOrEqual(0);
      expect(r.y).toBeLessThan(ROWS);
      for (let x = r.x; x < r.x + r.w; x++) {
        expect(taken.has(`${x},${r.y}`), `cell ${x},${r.y} painted twice`).toBe(false);
        taken.add(`${x},${r.y}`);
      }
    }
  });

  it("falls from sky into tide by dithering: rows of only sky, then rows of both, then only tide", () => {
    const rows = grid().map((row) => new Set(row.filter((c) => c === "sky" || c === "tide")));
    const mixed = rows.map((s, y) => (s.size === 2 ? y : -1)).filter((y) => y >= 0);
    expect(mixed.length).toBeGreaterThanOrEqual(8);
    expect(Math.max(...mixed)).toBeLessThan(HORIZON);
    expect(rows[0]).toEqual(new Set(["sky"]));
    expect(rows[HORIZON]!.has("sky")).toBe(false);
    const cells = grid();
    const tide = (from: number, to: number) => cells.slice(from, to).flat().filter((c) => c === "tide").length;
    const top = Math.min(...mixed);
    const third = Math.round((HORIZON - top) / 3);
    expect(tide(top, top + third)).toBeLessThan(tide(top + third, top + 2 * third));
    expect(tide(top + third, top + 2 * third)).toBeLessThan(tide(top + 2 * third, HORIZON));
  });

  it("has every star in the sky", () => {
    const cells = grid();
    for (const [x, y] of STARS) expect(cells[y]![x], `the star at ${x},${y}`).toBe("star");
  });

  it("has the owl awake, its eyes in sun with a glint each, and a moon", () => {
    const cells = grid().flat();
    const eyes = OWL_ROWS.join("").split("").filter((c) => c === "y").length;
    expect(cells.filter((c) => c === "eye")).toHaveLength(eyes);
    expect(cells.filter((c) => c === "glint")).toHaveLength(2);
    expect(cells.filter((c) => c === "moon").length).toBeGreaterThan(40);
  });

  it("is hidden from assistive technology and drawn on the pixel grid", () => {
    const { container } = render(<PixelNight />);
    const svg = container.querySelector("[data-slot=pixel-night]")!;
    expect(svg).toHaveAttribute("aria-hidden", "true");
    expect(svg).toHaveAttribute("shape-rendering", "crispEdges");
    expect(svg).toHaveAttribute("viewBox", `0 0 ${COLS} ${ROWS}`);
  });
});
