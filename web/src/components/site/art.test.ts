// @vitest-environment node
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { DAY, NIGHT, OWL_SCROLL, WALLPAPERS, artwork } from "./art";

const PUBLIC = path.join(import.meta.dirname, "../../../public");

/** A JPEG's pixel size, from its first start-of-frame marker. */
function jpegSize(file: string): { width: number; height: number } {
  const b = readFileSync(file);
  for (let i = 2; i < b.length;) {
    const marker = b[i + 1];
    if (marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc) return { height: b.readUInt16BE(i + 5), width: b.readUInt16BE(i + 7) };
    i += 2 + b.readUInt16BE(i + 2);
  }
  throw new Error(`${file} has no frame header`);
}

describe("the desktop's pictures", () => {
  const all = [...WALLPAPERS, OWL_SCROLL];

  it("are each a committed file of the size recorded, credited to its page at the Met", () => {
    for (const art of all) {
      expect(jpegSize(path.join(PUBLIC, art.src)), art.id).toEqual({ width: art.width, height: art.height });
      expect(art.url, art.id).toMatch(/^https:\/\/www\.metmuseum\.org\/art\/collection\/search\/\d+$/);
    }
    expect(new Set(all.map((a) => a.id)).size).toBe(all.length);
  });

  it("leave no picture in public/art that the desktop does not show", () => {
    expect(readdirSync(path.join(PUBLIC, "art")).sort()).toEqual(all.map((a) => path.basename(a.src)).sort());
  });

  it("open on a wallpaper of the set by day and by night", () => {
    expect(artwork(DAY).id).toBe(DAY);
    expect(artwork(NIGHT).id).toBe(NIGHT);
    expect(DAY).not.toBe(NIGHT);
  });
});
