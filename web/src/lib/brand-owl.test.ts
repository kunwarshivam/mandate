import { readFileSync } from "node:fs";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { FEATHERS } from "@/components/domain/owl";
import { BRAND_OWL_COLOURS, BRAND_OWL_SCRIPT } from "./brand-owl";

const CSS = readFileSync(path.resolve(__dirname, "../app/globals.css"), "utf8");

function pickWith(random: number): string | undefined {
  vi.spyOn(Math, "random").mockReturnValue(random);
  new Function(BRAND_OWL_SCRIPT)();
  return document.documentElement.dataset.owl;
}

afterEach(() => {
  vi.restoreAllMocks();
  delete document.documentElement.dataset.owl;
});

describe("the brand owl's colour (DEC-452)", () => {
  it("picks one of the feather colours an agent's owl may wear, from every draw of Math.random", () => {
    expect(BRAND_OWL_COLOURS).toBe(FEATHERS.length);
    const draws = [0, 0.2499, 0.25, 0.5, 0.74, 0.75, 0.9999];
    expect(draws.map(pickWith)).toEqual(["1", "1", "2", "3", "3", "4", "4"]);
  });

  it("maps every pick to its series colour in the stylesheet, the first by default", () => {
    expect(CSS).toMatch(/\[data-slot="owl"\] \{[^}]*--brand-owl: var\(--series-1\);[^}]*--brand-owl-beak: var\(--highlight\);/);
    for (let n = 2; n <= BRAND_OWL_COLOURS; n++) {
      expect(CSS).toMatch(new RegExp(`html\\[data-owl="${n}"\\] \\[data-slot="owl"\\] \\{\\s*--brand-owl: var\\(--series-${n}\\);`));
    }
    expect(CSS).toMatch(/html\[data-owl="2"\] \[data-slot="owl"\] \{[^}]*--brand-owl-beak: var\(--owl-pupil\);/);
  });

  it("never throws, so a failure cannot stop the theme script or the page", () => {
    vi.spyOn(Math, "random").mockImplementation(() => {
      throw new Error("no entropy");
    });
    expect(() => new Function(BRAND_OWL_SCRIPT)()).not.toThrow();
  });
});
