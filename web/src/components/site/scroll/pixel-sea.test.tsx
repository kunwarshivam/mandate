import { readFileSync } from "node:fs";
import { join } from "node:path";
import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { CREST_TILE, FOAM_TILE, GEESE, LEFT_ISLAND, MOON_RADIUS, PixelSea, REFLECTION, RIGHT_ISLAND, moonShape, runs } from "./pixel-sea";

/**
 * The opening's pixel sea (DEC-908): flat token colours on the thread's grid, crests that drift a
 * cell at a time only with motion allowed, a round moon, and the thread's band so its knot goes pale.
 */

const CSS = readFileSync(join(__dirname, "scroll.module.css"), "utf8");

describe("the pixel sea", () => {
  it("is hidden from assistive technology, never in the pointer's way, and is a band of its own for the thread", () => {
    const { container } = render(<PixelSea />);
    const sea = container.querySelector<HTMLElement>("[data-slot=pixel-sea]")!;
    expect(sea).toHaveAttribute("aria-hidden", "true");
    expect(sea).toHaveAttribute("data-thread-band");
    expect(sea.className).toContain("pointer-events-none");
    expect(CSS).toMatch(/\.sea \{\s*--thread: var\(--tide-muted\);/);
  });

  it("paints only flat token colours, on the pixel grid, at the thread's two sizes of cell", () => {
    const { container } = render(<PixelSea />);
    const fills = [...container.querySelectorAll("rect")].map((r) => r.getAttribute("fill")!);
    for (const fill of fills) expect(fill).toMatch(/^(var\(--[a-z-]+\)|url\(#sea-(crest|foam)-(6|8)\))$/);
    expect(new Set(fills.filter((f) => f.startsWith("var")))).toEqual(new Set(["var(--tide)", "var(--tide-muted)", "var(--highlight)", "var(--sea-land)"]));
    for (const svg of container.querySelectorAll("svg")) expect(svg).toHaveAttribute("shape-rendering", "crispEdges");
    expect([...container.querySelectorAll("pattern")].map((p) => p.id).sort()).toEqual(["sea-crest-6", "sea-crest-8", "sea-foam-6", "sea-foam-8"]);
  });

  it("drifts its crests one cell a step, a tile at a time, only with motion allowed", () => {
    const motion = CSS.slice(CSS.indexOf("@media (prefers-reduced-motion: no-preference)"));
    expect(CSS.indexOf(".crests")).toBeGreaterThan(CSS.indexOf("@media (prefers-reduced-motion: no-preference)"));
    expect(motion).toMatch(new RegExp(`\\.crests \\{\\s*animation: drift 48s steps\\(${CREST_TILE[0]!.length}\\) infinite;`));
    const { container } = render(<PixelSea />);
    const crests = [...container.querySelectorAll<SVGElement>("[data-slot=sea-crests]")];
    expect(crests).toHaveLength(2);
    for (const c of crests) {
      const cell = Number(c.getAttribute("fill")!.match(/(\d+)\)$/)![1]);
      expect(c.closest("svg")!.style.getPropertyValue("--tile")).toBe(`${CREST_TILE[0]!.length * cell}px`);
      expect(c.getAttribute("width"), "wide enough that a tile's drift never shows its end").toBe("200%");
    }
  });

  it("draws every shape as whole runs inside its own rows", () => {
    for (const shape of [CREST_TILE, FOAM_TILE, LEFT_ISLAND, RIGHT_ISLAND, GEESE, REFLECTION, moonShape()]) {
      const width = shape[0]!.length;
      for (const row of shape) expect(row).toHaveLength(width);
      const cells = runs(shape, 1).reduce((n, r) => n + r.w, 0);
      expect(cells).toBe(shape.join("").replace(/\./g, "").length);
    }
  });

  it("rises a round moon: as wide as it is tall, the same left to right and top to bottom", () => {
    const moon = moonShape();
    expect(moon).toHaveLength(MOON_RADIUS * 2);
    expect(moon.map((row) => [...row].reverse().join(""))).toEqual(moon);
    expect([...moon].reverse()).toEqual(moon);
    expect(moon[MOON_RADIUS]).toBe("x".repeat(MOON_RADIUS * 2));
  });

  it("lays the moon's reflection under it, as wide as the moon, in bars that never meet", () => {
    expect(REFLECTION[0]).toHaveLength(MOON_RADIUS * 2);
    REFLECTION.forEach((row, y) => {
      if (y % 2 === 0) expect(row, `row ${y} is water`).toMatch(/^\.+$/);
      else expect(row, `row ${y} is one bar`).toMatch(/^\.*x+\.*$/);
    });
  });

  it("keeps the land and geese visible in either theme: ink by day, a muted grey by night", () => {
    expect(CSS).toMatch(/\.sea \{[^}]*--sea-land: var\(--foreground\);/);
    expect(CSS).toMatch(/html\[data-mode="dark"\]\) \.sea \{\s*--sea-land: var\(--muted-foreground\);/);
  });
});
