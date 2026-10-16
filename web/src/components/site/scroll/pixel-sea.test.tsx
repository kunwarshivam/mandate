import { readFileSync } from "node:fs";
import { join } from "node:path";
import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  FOAM_TILE,
  PixelSea,
  SEA_DEPTH,
  WAVES,
  runs,
  swell,
  waveTile,
} from "./pixel-sea";

/**
 * The opening's pixel sea (DEC-908, DEC-932): flat token colours on one pixel grid, rows of waves
 * that grow toward the viewer and glide and heave only with motion allowed, and nothing over the
 * water: no moon, no gulls, no islands.
 */

const CSS = readFileSync(join(__dirname, "scroll.module.css"), "utf8");
const MOTION_AT = CSS.indexOf("@media (prefers-reduced-motion: no-preference)");
const MOTION = CSS.slice(MOTION_AT);
const AT_REST = CSS.slice(0, MOTION_AT);

/** The `animation` a class is given with motion allowed, in one line. */
function animationOf(name: string): string {
  const rule = MOTION.match(new RegExp(`\\.${name} \\{([^}]*)\\}`));
  expect(rule, `.${name} moves only with motion allowed`).not.toBeNull();
  return rule![1]!.replace(/\s+/g, " ");
}

describe("the pixel sea", () => {
  it("is hidden from assistive technology and never in the pointer's way", () => {
    const { container } = render(<PixelSea />);
    const sea = container.querySelector<HTMLElement>("[data-slot=pixel-sea]")!;
    expect(sea).toHaveAttribute("aria-hidden", "true");
    expect(sea.className).toContain("pointer-events-none");
  });

  it("paints only flat token colours, on the pixel grid, at two sizes of cell", () => {
    const { container } = render(<PixelSea />);
    const fills = [...container.querySelectorAll("rect")].map((r) => r.getAttribute("fill")!);
    for (const fill of fills) expect(fill).toMatch(/^(var\(--[a-z-]+\)|url\(#sea-(wave-\d+|foam)-(6|8)\))$/);
    expect(new Set(fills.filter((f) => f.startsWith("var")))).toEqual(
      new Set(["var(--tide)", "var(--tide-muted)", "var(--tide-foreground)"]),
    );
    for (const svg of container.querySelectorAll("svg")) expect(svg).toHaveAttribute("shape-rendering", "crispEdges");
    const ids = [...container.querySelectorAll("pattern")].map((p) => p.id);
    expect(new Set(ids).size, "every pattern has its own id").toBe(ids.length);
    expect(ids.filter((id) => id.startsWith("sea-wave-"))).toHaveLength(swell().length * 2);
  });

  it("recedes to the horizon: the rows grow and quicken toward the viewer, and the nearest repeats to the bottom", () => {
    for (let i = 1; i < WAVES.length; i++) {
      const [far, near] = [WAVES[i - 1]!, WAVES[i]!];
      expect(near.length, `row ${i} is longer`).toBeGreaterThan(far.length);
      expect(near.height, `row ${i} is no lower`).toBeGreaterThanOrEqual(far.height);
      expect(near.gap, `row ${i} has no less water under it`).toBeGreaterThanOrEqual(far.gap);
      expect(near.speed, `row ${i} glides faster`).toBeGreaterThan(far.speed);
    }
    const rows = swell();
    expect(rows[0]!.from).toBe(FOAM_TILE.length);
    for (let i = 1; i < rows.length; i++) {
      const above = rows[i - 1]!;
      expect(rows[i]!.from, `row ${i} starts where the water under row ${i - 1} ends`).toBe(above.from + above.wave.height + 1 + above.wave.gap);
    }
    expect(rows.at(-1)!.from).toBeLessThan(SEA_DEPTH);
    expect(rows.slice(WAVES.length).every((r) => r.wave === WAVES.at(-1))).toBe(true);
    expect(new Set(rows.slice(WAVES.length).map((r) => r.shift)).size, "the repeated rows' crests do not line up").toBeGreaterThan(1);
  });

  it("draws each wave as one unbroken stroke over a sharp, leaning crest, open water in the trough, white only where a tall one breaks", () => {
    for (const wave of WAVES) {
      const tile = waveTile(wave);
      const name = JSON.stringify(wave);
      expect(tile).toHaveLength(wave.height + 1);
      const cells = (x: number) => tile.map((row, y) => (row[x] === "." ? -1 : y)).filter((y) => y >= 0);
      const drawn = Array.from({ length: wave.length }, (_, x) => cells(x).length > 0);
      const starts = drawn.filter((d, x) => d && !drawn[(x - 1 + wave.length) % wave.length]).length;
      expect(starts, `${name} is one stroke a wave`).toBe(1);
      expect(drawn.filter((d) => !d).length, `${name} leaves open water in its trough`).toBeGreaterThanOrEqual(Math.floor(wave.length / 4));
      for (let x = 0; x < wave.length; x++) {
        const next = (x + 1) % wave.length;
        if (!drawn[x] || !drawn[next]) continue;
        const gap = Math.min(...cells(x).map((a) => Math.min(...cells(next).map((b) => Math.abs(a - b)))));
        expect(gap, `column ${x} of ${name} touches the next`).toBeLessThanOrEqual(1);
      }
      const crest = tile[0]!;
      expect(crest.replace(/\./g, "").length, "the crest is sharp").toBeLessThanOrEqual(Math.ceil(wave.length / 5));
      expect(crest.search(/[^.]/), "the crest leans forward, to the left where the rows glide").toBeLessThan(wave.length / 2);
      expect(tile.join("").includes("w"), `${name} breaks white only if two or more cells tall`).toBe(wave.height >= 2);
      tile.slice(2).forEach((row) => expect(row).not.toContain("w"));
    }
  });

  it("glides each row one wave's length, slower the farther off, and heaves the rows one beat after another", () => {
    const { container } = render(<PixelSea />);
    for (const cell of [6, 8]) {
      const rows = [...container.querySelectorAll<HTMLElement>("[data-slot=sea-wave]")].filter((el) => el.querySelector("pattern")!.id.endsWith(`-${cell}`));
      expect(rows).toHaveLength(swell().length);
      rows.forEach((el, i) => {
        const { wave, from } = swell()[i]!;
        const tile = wave.length * cell;
        expect(el.style.getPropertyValue("--tile")).toBe(`${tile}px`);
        expect(el.style.width, "wide enough that a wave's glide never shows an end").toBe(`calc(100% + ${tile}px)`);
        expect(el.style.top).toBe(`${from * cell}px`);
        expect(el.style.getPropertyValue("--glide")).toBe(`${(wave.length / wave.speed).toFixed(2)}s`);
        expect(el.style.getPropertyValue("--tile-px"), "a pixel a step").toBe(String(tile));
        expect(el.style.getPropertyValue("--tile-dpx"), "a device pixel a step on a dense screen").toBe(String(tile * 2));
        const heave = parseFloat(el.style.getPropertyValue("--heave"));
        expect(heave).toBeGreaterThanOrEqual(1);
        expect(el.style.getPropertyValue("--heave-px"), "a pixel a step").toBe(String(heave));
        expect(el.style.getPropertyValue("--heave-dpx")).toBe(String(heave * 2));
        if (i > 0) expect(parseFloat(el.style.getPropertyValue("--beat")), "the swell reaches each row after the one above").toBeLessThan(parseFloat(rows[i - 1]!.style.getPropertyValue("--beat")));
      });
    }
    expect(animationOf("wave"), "glides a whole pixel a step").toContain("glide var(--glide) steps(var(--glide-steps)) infinite");
    expect(animationOf("wave"), "heaves a whole pixel a step").toMatch(/heave [\d.]+s steps\(var\(--heave-steps\)\) var\(--beat\) infinite alternate/);
    expect(animationOf("foam")).toContain("steps(var(--lap-steps))");
    expect(CSS).toMatch(/@keyframes glide \{\s*to \{\s*translate: calc\(-1 \* var\(--tile\)\) 0;/);
    expect(CSS).toMatch(/@keyframes heave \{\s*to \{\s*transform: translateY\(calc\(-1 \* var\(--heave\)\)\);/);
  });

  it("counts its steps in pixels, or in device pixels on a dense screen", () => {
    const steps = { wave: { "--glide-steps": "tile", "--heave-steps": "heave" }, foam: { "--lap-steps": "cell" } };
    const dense = AT_REST.slice(AT_REST.indexOf("@media (min-resolution: 2x)"));
    expect(dense.length).toBeLessThan(AT_REST.length);
    for (const [name, vars] of Object.entries(steps))
      for (const [prop, unit] of Object.entries(vars)) {
        expect(AT_REST, `.${name} steps a pixel`).toMatch(new RegExp(`\\.${name} \\{[^}]*${prop}: var\\(--${unit}-px\\);`));
        expect(dense, `.${name} steps a device pixel on a dense screen`).toMatch(new RegExp(`\\.${name} \\{[^}]*${prop}: var\\(--${unit}-dpx\\);`));
      }
  });

  it("moves only with motion allowed, a whole pixel or frame at a time, never between pixels", () => {
    for (const name of ["wave", "foam"]) {
      for (const one of animationOf(name).match(/animation: (.*?);/)![1]!.split(/,(?![^(]*\))/)) expect(one, `.${name} steps`).toMatch(/steps\(/);
      expect(AT_REST, `.${name} has no animation at rest`).not.toMatch(new RegExp(`\\.${name} \\{[^}]*animation`));
    }
  });

  it("gives each size of sea its cell in pixels and device pixels", () => {
    const { container } = render(<PixelSea />);
    for (const cell of [6, 8]) {
      const sea = [...container.querySelectorAll<HTMLElement>("[data-slot=pixel-sea] > div")].find((d) => d.style.getPropertyValue("--cell") === `${cell}px`)!;
      expect(sea.style.getPropertyValue("--cell-px")).toBe(String(cell));
      expect(sea.style.getPropertyValue("--cell-dpx")).toBe(String(cell * 2));
    }
  });

  it("draws every shape as whole runs inside its own rows", () => {
    for (const shape of [...WAVES.map(waveTile), FOAM_TILE]) {
      const width = shape[0]!.length;
      for (const row of shape) expect(row).toHaveLength(width);
      const cells = runs(shape, 1).reduce((n, r) => n + r.w, 0);
      expect(cells).toBe(shape.join("").replace(/\./g, "").length);
    }
  });

  it("draws no moon, gulls or islands over the water, and keeps no colour or motion for them", () => {
    const { container } = render(<PixelSea />);
    expect(container.querySelectorAll("[data-slot^=gull], [data-slot=sea-moon], [data-slot=glint]")).toHaveLength(0);
    expect(CSS).not.toMatch(/\.moon\b|\.glint[AB]|@keyframes (rise|glint-)/);
    expect(container.innerHTML).not.toContain("--sea-land");
    expect(CSS).not.toMatch(/--sea-land|\.wings(Up|Down)|@keyframes wings-/);
  });
});
