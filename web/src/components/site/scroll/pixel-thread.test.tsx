import { render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LongPage } from "./long-page";
import { type Knot, type Point, cellSize, gutterX, isStitch, rasterize, route } from "./pixel-thread";

/**
 * The long page's thread (DEC-907): it only goes down or across, so it never runs over itself, it
 * passes through every knot, it is drawn as a hand-drawn pixel line, it stays in the gutter, and it
 * is decoration only, all there at once with motion reduced.
 */

function seeded(seed: number) {
  let s = seed;
  return () => {
    s = (s * 1103515245 + 12345) % 2 ** 31;
    return s / 2 ** 31;
  };
}

/** A page of parts down a window `width` wide, each knot over a box on one side or the other, as the long page lays them out. */
function layout(seed: number, width = 1440): { knots: Knot[]; crossings: number[] } {
  const rand = seeded(seed);
  const knots: Knot[] = [];
  const crossings: number[] = [];
  let y = 200;
  for (let i = 0; i < 9; i++) {
    const top = y + 80 + Math.round(rand() * 200);
    const x = 200 + Math.round(rand() * (width - 400));
    const below = top + 120 + Math.round(rand() * 400);
    crossings.push(y + 24);
    knots.push({ x, y: top, below });
    y = below + 40 + Math.round(rand() * 160);
  }
  return { knots, crossings };
}

const key = (p: Point) => `${p.x},${p.y}`;

describe("the thread's route", () => {
  for (const seed of [1, 2, 3, 4, 5, 6, 7, 8]) {
    it(`only goes down or across, through every knot, and never over itself (layout ${seed})`, () => {
      const { knots, crossings } = layout(seed);
      const path = route(knots, crossings, 1440, gutterX(1440, 8));
      for (let i = 1; i < path.length; i++) {
        const [a, b] = [path[i - 1]!, path[i]!];
        expect(a.x === b.x || a.y === b.y, `a straight run from ${key(a)} to ${key(b)}`).toBe(true);
        expect(b.y, `never up, from ${key(a)} to ${key(b)}`).toBeGreaterThanOrEqual(a.y);
        expect(key(a)).not.toBe(key(b));
      }
      for (let i = 2; i < path.length; i++) {
        const [a, b, c] = [path[i - 2]!, path[i - 1]!, path[i]!];
        const doubles = a.y === b.y && b.y === c.y && Math.sign(b.x - a.x) !== Math.sign(c.x - b.x);
        expect(doubles, `doubles back at ${key(b)}`).toBe(false);
      }
      expect(new Set(path.map(key)).size).toBe(path.length);
      const seen = new Set(path.map(key));
      for (const k of knots) expect(seen.has(key(k)), `through the knot at ${key(k)}`).toBe(true);
    });
  }

  it("crosses a part above it, beside both pictures, when the next knot is on the other side", () => {
    const knots: Knot[] = [
      { x: 300, y: 100, below: 400 },
      { x: 1100, y: 700, below: 900 },
    ];
    const path = route(knots, [0, 500], 1440, 48);
    expect(path).toEqual([
      { x: 300, y: 100 },
      { x: 300, y: 400 },
      { x: 48, y: 400 },
      { x: 48, y: 500 },
      { x: 1392, y: 500 },
      { x: 1392, y: 700 },
      { x: 1100, y: 700 },
    ]);
  });

  it("crosses under the last picture, never back up to the part's top, when that picture reaches into the part", () => {
    const knots: Knot[] = [
      { x: 300, y: 100, below: 400 },
      { x: 1100, y: 700, below: 900 },
    ];
    const path = route(knots, [0, 300], 1440, 48);
    expect(path).toContainEqual({ x: 1392, y: 400 });
    for (let i = 1; i < path.length; i++) expect(path[i]!.y).toBeGreaterThanOrEqual(path[i - 1]!.y);
  });
});

describe("the thread in pixels", () => {
  for (const seed of [1, 2, 3, 4]) {
    it(`is one square a step, each touching the last, with no square corners and no square twice (layout ${seed})`, () => {
      const { knots, crossings } = layout(seed);
      const cells = rasterize(route(knots, crossings, 1440, gutterX(1440, 8)), 8);
      expect(cells.length).toBeGreaterThan(100);
      for (let i = 1; i < cells.length; i++) {
        const [a, b] = [cells[i - 1]!, cells[i]!];
        expect(Math.max(Math.abs(a.x - b.x), Math.abs(a.y - b.y)), `a step from ${key(a)} to ${key(b)}`).toBe(1);
      }
      for (let i = 2; i < cells.length; i++) {
        const [a, c] = [cells[i - 2]!, cells[i]!];
        expect(Math.abs(a.x - c.x) === 1 && Math.abs(a.y - c.y) === 1, `a square corner at ${key(cells[i - 1]!)}`).toBe(false);
      }
      expect(new Set(cells.map(key)).size).toBe(cells.length);
    });
  }

  it("runs three stitches on and one off", () => {
    expect([0, 1, 2, 3, 4, 5, 6, 7].map(isStitch)).toEqual([true, true, true, false, true, true, true, false]);
  });

  it("has smaller stitches on a phone", () => {
    expect(cellSize(390)).toBeLessThan(cellSize(1440));
  });

  it("stays in the outer gutter, on the grid, at every width from 320", () => {
    for (const width of [320, 390, 640, 768, 1024, 1280, 1440, 1920, 2560]) {
      const cell = cellSize(width);
      const x = gutterX(width, cell);
      const pad = width < 640 ? 20 : width < 1024 ? 32 : 48;
      const column = Math.max(0, (width - 1280) / 2) + pad;
      expect(x % cell, `on the grid at ${width}`).toBe(0);
      expect(x, `not off the edge at ${width}`).toBeGreaterThanOrEqual(cell);
      expect(x + cell * 1.5, `clear of the column at ${width}`).toBeLessThanOrEqual(column);
    }
  });
});

describe("the thread's canvas", () => {
  function context() {
    return { setTransform: vi.fn(), clearRect: vi.fn(), fillRect: vi.fn(), save: vi.fn(), restore: vi.fn(), beginPath: vi.fn(), rect: vi.fn(), clip: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(), closePath: vi.fn(), fill: vi.fn(), stroke: vi.fn(), fillStyle: "", strokeStyle: "", lineWidth: 0, lineJoin: "" };
  }

  function motion(reduce: boolean) {
    vi.stubGlobal("matchMedia", (query: string) => ({ matches: reduce && query.includes("reduce"), media: query, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  }

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("is hidden from assistive technology", () => {
    render(<LongPage />);
    const canvas = document.querySelector("[data-slot=pixel-thread]")!;
    expect(canvas.tagName).toBe("CANVAS");
    expect(canvas).toHaveAttribute("aria-hidden", "true");
    expect(canvas).not.toHaveAttribute("tabindex");
  });

  it("with motion reduced, is there whole at once and asks for no frame", () => {
    motion(true);
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(context() as unknown as RenderingContext);
    const frames = vi.spyOn(window, "requestAnimationFrame");
    render(<LongPage />);
    expect(document.querySelector<HTMLElement>("[data-slot=pixel-thread]")!.dataset.thread).toBe("whole");
    expect(frames.mock.calls.filter(([cb]) => cb.name === "frame")).toHaveLength(0);
  });

  it("with motion allowed, is sewn on animation frames and stops when it leaves the page", () => {
    motion(false);
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(context() as unknown as RenderingContext);
    const frames = vi.spyOn(window, "requestAnimationFrame");
    const cancel = vi.spyOn(window, "cancelAnimationFrame");
    const { unmount } = render(<LongPage />);
    expect(frames.mock.calls.some(([cb]) => cb.name === "frame")).toBe(true);
    unmount();
    expect(cancel).toHaveBeenCalled();
  });
});
