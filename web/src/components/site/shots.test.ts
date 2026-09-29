import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { SHOTS, type ShotName, shotSrc } from "./shots";

const WEB = resolve(__dirname, "../../..");
const PUBLIC = join(WEB, "public");
const SCRIPT = readFileSync(join(WEB, "scripts/site-shots.mjs"), "utf8");
const MODES = ["light", "dark"] as const;
const NAMES = Object.keys(SHOTS) as ShotName[];

/** A PNG's size, from its IHDR chunk. */
function pngSize(file: string): { width: number; height: number } {
  const head = readFileSync(file).subarray(0, 24);
  expect(head.subarray(1, 4).toString("ascii")).toBe("PNG");
  return { width: head.readUInt32BE(16), height: head.readUInt32BE(20) };
}

describe("the landing page's screenshots", () => {
  it.each(NAMES.flatMap((name) => MODES.map((mode) => [name, mode] as const)))("%s in %s is committed at 2x the size shots.ts gives", (name, mode) => {
    const file = join(PUBLIC, shotSrc(name, mode));
    expect(pngSize(file)).toEqual({ width: SHOTS[name].width * 2, height: SHOTS[name].height * 2 });
    expect(statSync(file).size).toBeLessThan(400 * 1024);
  });

  it("are exactly the shots the script makes, and nothing else is in public/site", () => {
    const scripted = [...SCRIPT.matchAll(/\{\s*name: "([a-z-]+)"/g)].map((m) => m[1]);
    expect(scripted.sort()).toEqual([...NAMES].sort());
    const files = readdirSync(join(PUBLIC, "site")).sort();
    expect(files).toEqual(NAMES.flatMap((name) => MODES.map((mode) => `${name}-${mode}.png`)).sort());
  });

  it("are taken at 2x, in both themes, with motion reduced, hiding every P&L before the shutter", () => {
    expect(SCRIPT).toMatch(/export const SCALE = 2;/);
    expect(SCRIPT).toMatch(/export const MODES = \["light", "dark"\];/);
    expect(SCRIPT).toMatch(/reducedMotion: "reduce"/);
    for (const hidden of ['[data-slot="key-figures"]', '[data-slot="hero-change"]', "[data-direction]", "P&L", "Loss today"]) expect(SCRIPT).toContain(hidden);
  });

  it("never touch the founder's preview port", () => {
    expect(SCRIPT).toMatch(/export const PORT = 4342;/);
    expect(SCRIPT).not.toContain("4317");
  });
});
