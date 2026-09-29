// @vitest-environment node
import { mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { inflateSync } from "node:zlib";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { INK, MARK_SOURCE, OFF_WHITE, OUTPUTS, PUBLIC_DIR, generateBrandAssets } from "../../../scripts/brand-assets.mjs";
import { MARK_PATH } from "./Logo";
import { brandHex, INK as PALETTE_INK, OFF_WHITE as PALETTE_OFF_WHITE } from "@/lib/brand-palette";

type Rgba = [number, number, number, number];

interface Image {
  width: number;
  height: number;
  at(x: number, y: number): Rgba;
}

/** Decodes the 8-bit RGBA, non-interlaced PNG files resvg writes. */
function decodePng(png: Buffer): Image {
  expect(png.subarray(0, 8).toString("hex")).toBe("89504e470d0a1a0a");
  let offset = 8;
  let width = 0;
  let height = 0;
  const idat: Buffer[] = [];
  while (offset < png.length) {
    const length = png.readUInt32BE(offset);
    const type = png.toString("ascii", offset + 4, offset + 8);
    const data = png.subarray(offset + 8, offset + 8 + length);
    if (type === "IHDR") {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      expect([data[8], data[9], data[12]]).toEqual([8, 6, 0]);
    }
    if (type === "IDAT") idat.push(data);
    offset += 12 + length;
  }
  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * 4;
  const pixels = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    for (let i = 0; i < stride; i++) {
      const a = i >= 4 ? pixels[y * stride + i - 4] : 0;
      const b = y > 0 ? pixels[(y - 1) * stride + i] : 0;
      const c = i >= 4 && y > 0 ? pixels[(y - 1) * stride + i - 4] : 0;
      const p = a + b - c;
      const [pa, pb, pc] = [Math.abs(p - a), Math.abs(p - b), Math.abs(p - c)];
      const nearest = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      const predictor = [0, a, b, (a + b) >> 1, nearest][filter];
      pixels[y * stride + i] = (line[i] + predictor) & 0xff;
    }
  }
  return {
    width,
    height,
    at: (x, y) => [...pixels.subarray(y * stride + x * 4, y * stride + x * 4 + 4)] as Rgba,
  };
}

const rgb = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
const opaque = (hex: string): Rgba => [...rgb(hex), 255] as Rgba;

function pixels(image: Image): Array<[number, number, Rgba]> {
  const out: Array<[number, number, Rgba]> = [];
  for (let y = 0; y < image.height; y++) for (let x = 0; x < image.width; x++) out.push([x, y, image.at(x, y)]);
  return out;
}

let fresh: string;
const committed = (name: string) => readFile(path.join(PUBLIC_DIR, name));
const generated = (name: string) => readFile(path.join(fresh, name));
const png = async (name: string) => decodePng(await committed(name));

beforeAll(async () => {
  fresh = await mkdtemp(path.join(tmpdir(), "owlhead-brand-"));
  await generateBrandAssets(fresh);
});

afterAll(() => rm(fresh, { recursive: true, force: true }));

describe("brand assets (npm run brand)", () => {
  it("writes exactly the files it lists", async () => {
    expect((await readdir(fresh)).sort()).toEqual([...OUTPUTS].sort());
  });

  it.each(OUTPUTS)("commits %s exactly as the generator writes it; rerun `npm run brand` if this fails", async (name) => {
    expect((await generated(name)).equals(await committed(name))).toBe(true);
  });

  it("uses the palette's ink and off-white, and the mark from the SVG source", async () => {
    expect(INK).toBe(PALETTE_INK);
    expect(OFF_WHITE).toBe(PALETTE_OFF_WHITE);
    expect(await readFile(MARK_SOURCE, "utf8")).toContain(`d="${MARK_PATH}"`);
  });

  it("draws the favicon SVG as the ink mark on an off-white square, in every colour scheme", async () => {
    const svg = (await committed("favicon.svg")).toString("utf8");
    expect(svg).toContain('viewBox="0 0 1000 1000"');
    expect(svg).toContain(`<rect width="1000" height="1000" fill="${OFF_WHITE}"/>`);
    expect(svg).toContain(`<path fill="${INK}"`);
    expect(svg).toContain(`d="${MARK_PATH}"`);
    expect(svg).not.toMatch(/<style|prefers-color-scheme/);
  });

  it.each([16, 32, 48])("renders favicon-%i.png as the ink mark, as large as fits, on an opaque off-white square", async (size) => {
    const image = await png(`favicon-${size}.png`);
    expect([image.width, image.height]).toEqual([size, size]);
    for (const [x, y] of [
      [0, 0],
      [size - 1, 0],
      [0, size - 1],
      [size - 1, size - 1],
    ])
      expect(image.at(x, y)).toEqual(opaque(OFF_WHITE));
    const all = pixels(image);
    const [bg, fg] = [rgb(OFF_WHITE), rgb(INK)];
    for (const [, , p] of all) {
      expect(p[3]).toBe(255);
      for (const c of [0, 1, 2]) expect(p[c]).toBeGreaterThanOrEqual(fg[c]);
      for (const c of [0, 1, 2]) expect(p[c]).toBeLessThanOrEqual(bg[c]);
    }
    expect(all.some(([, , p]) => p.join() === opaque(INK).join())).toBe(true);
    const ink = all.filter(([, , p]) => p.join() !== opaque(OFF_WHITE).join());
    const ys = ink.map(([, y]) => y);
    const xs = ink.map(([x]) => x);
    const [top, bottom, left, right] = [Math.min(...ys), Math.max(...ys), Math.min(...xs), Math.max(...xs)];
    const [below, beside] = [size - 1 - bottom, size - 1 - right];
    for (const margin of [top, below]) {
      expect(margin).toBeGreaterThanOrEqual(1);
      expect(margin).toBeLessThanOrEqual(size / 16);
    }
    expect(Math.abs(top - below)).toBeLessThanOrEqual(1);
    expect(Math.abs(left - beside)).toBeLessThanOrEqual(1);
  });

  it("packs the three favicon PNG files, smallest first, into favicon.ico", async () => {
    const ico = await committed("favicon.ico");
    expect([ico.readUInt16LE(0), ico.readUInt16LE(2), ico.readUInt16LE(4)]).toEqual([0, 1, 3]);
    for (const [i, size] of [16, 32, 48].entries()) {
      const entry = 6 + 16 * i;
      expect([ico[entry], ico[entry + 1]]).toEqual([size, size]);
      const length = ico.readUInt32LE(entry + 8);
      const offset = ico.readUInt32LE(entry + 12);
      expect(ico.subarray(offset, offset + length).equals(await committed(`favicon-${size}.png`))).toBe(true);
    }
  });

  it.each([
    ["apple-touch-icon.png", 180],
    ["pwa-192.png", 192],
    ["pwa-512.png", 512],
    ["pwa-maskable-512.png", 512],
  ])("renders %s as the ink mark centred on an off-white %ipx square", async (name, size) => {
    const image = await png(name);
    expect([image.width, image.height]).toEqual([size, size]);
    for (const [x, y] of [
      [0, 0],
      [size - 1, 0],
      [0, size - 1],
      [size - 1, size - 1],
    ])
      expect(image.at(x, y)).toEqual(opaque(OFF_WHITE));
    const mark = pixels(image).filter(([, , p]) => p.join() === opaque(INK).join());
    expect(mark.length).toBeGreaterThan(0);
    const ys = mark.map(([, y]) => y);
    const xs = mark.map(([x]) => x);
    const [top, bottom, left, right] = [Math.min(...ys), Math.max(...ys), Math.min(...xs), Math.max(...xs)];
    expect(Math.abs(top - (size - 1 - bottom))).toBeLessThanOrEqual(Math.ceil(size * 0.02));
    expect(Math.abs(left - (size - 1 - right))).toBeLessThanOrEqual(Math.ceil(size * 0.02));
    if (name !== "pwa-maskable-512.png") expect((bottom - top + 1) / size).toBeCloseTo(0.76, 1);
  });

  it("keeps the maskable icon's mark inside the 80% safe zone", async () => {
    const image = await png("pwa-maskable-512.png");
    const centre = (image.width - 1) / 2;
    const radius = image.width * 0.4;
    for (const [x, y, p] of pixels(image)) {
      if (Math.hypot(x - centre, y - centre) > radius) expect(p).toEqual(opaque(OFF_WHITE));
    }
  });

  it("renders og-image.png at 1200 by 630: off-white, the ink mark and wordmark, and nothing else", async () => {
    const image = await png("og-image.png");
    expect([image.width, image.height]).toEqual([1200, 630]);
    const all = pixels(image);
    const inkPixels = all.filter(([, , p]) => p.join() === opaque(INK).join());
    const xs = inkPixels.map(([x]) => x);
    const ys = inkPixels.map(([, y]) => y);
    const [left, right, top, bottom] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
    expect(Math.abs(left - (1199 - right))).toBeLessThanOrEqual(2);
    expect(Math.abs(top - (629 - bottom))).toBeLessThanOrEqual(2);
    expect(inkPixels.some(([x, y]) => x < 345 && y > 190 && y < 450)).toBe(true);
    expect(inkPixels.some(([x, y]) => x > 445 && y > 190 && y < 312)).toBe(true);
    const ink = all.filter(([, , p]) => p.join() !== opaque(OFF_WHITE).join());
    expect(ink.some(([x, y]) => x > 445 && y > 330)).toBe(false);
    for (const [, , p] of ink) expect(p[3]).toBe(255);
    const [bg, fg] = [rgb(OFF_WHITE), rgb(INK)];
    for (const [, , p] of ink) for (const c of [0, 1, 2]) expect(p[c]).toBeGreaterThanOrEqual(Math.min(bg[c], fg[c]));
    expect(all.some(([, , p]) => p.join() === opaque(brandHex("Volt")).join())).toBe(false);
  });

  it("describes the installed app in site.webmanifest", async () => {
    const manifest = JSON.parse((await committed("site.webmanifest")).toString("utf8"));
    expect(manifest).toMatchObject({ name: "Owlhead", short_name: "Owlhead", theme_color: OFF_WHITE, background_color: OFF_WHITE, display: "standalone" });
    expect(manifest.icons.map((i: { src: string }) => i.src)).toEqual(["/pwa-192.png", "/pwa-512.png", "/pwa-maskable-512.png"]);
  });

  it("commits the brand sources as outlines, with no font file anywhere", async () => {
    const og = await readFile(path.join(PUBLIC_DIR, "../brand/og-image.svg"), "utf8");
    expect(og).not.toMatch(/<text|@font-face|font-family|<image/);
    const files = [...(await readdir(PUBLIC_DIR)), ...(await readdir(path.join(PUBLIC_DIR, "../brand"))), ...(await readdir(path.dirname(MARK_SOURCE)))];
    expect(files.filter((f) => /\.(ttf|otf|woff2?|pfb|t1)$/i.test(f))).toEqual([]);
  });
});
